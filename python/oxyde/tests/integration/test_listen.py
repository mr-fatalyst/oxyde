"""PostgreSQL LISTEN/NOTIFY integration tests."""

from __future__ import annotations

import asyncio
import gc
import uuid

import pytest
import pytest_asyncio

from oxyde import db, execute_raw
from oxyde.core import wrapper

pytestmark = pytest.mark.asyncio


@pytest.fixture
def pg_url(_pg_container):
    if _pg_container is None:
        pytest.skip("PostgreSQL container not available")
    host = _pg_container.get_container_host_ip()
    port = _pg_container.get_exposed_port(5432)
    return f"postgres://test:test@{host}:{port}/test"


@pytest_asyncio.fixture
async def database(pg_url):
    name = f"listen_{uuid.uuid4().hex}"
    async with db.connect(
        pg_url, name=name, settings=db.PoolSettings(max_connections=4)
    ) as database:
        yield database


async def publish(database, channel, payload="hint"):
    return await execute_raw(
        "SELECT pg_notify($1, $2), pg_backend_pid() AS pid",
        [channel, payload],
        client=database,
    )


async def receive(listener):
    return await asyncio.wait_for(listener.recv(), 5)


async def test_multiple_channels_and_broadcast(database):
    channels = ['quoted"name', "更新", "a" * 63, "é" * 31]
    async with db.listen(*channels, using=database.name) as first:
        async with db.listen(*channels, using=database.name) as second:
            for channel in channels:
                pid = (await publish(database, channel, "run-1"))[0]["pid"]
                for listener in (first, second):
                    notification = await receive(listener)
                    assert notification == db.Notification(channel, "run-1", pid)


async def test_notifications_follow_transaction_commit(database):
    async with db.listen("committed", using=database.name) as listener:
        async with db.atomic(using=database.name):
            await execute_raw(
                "SELECT pg_notify($1, $2)",
                ["committed", "commit"],
                using=database.name,
            )
            with pytest.raises(asyncio.TimeoutError):
                await asyncio.wait_for(listener.recv(), 0.1)
        assert (await receive(listener)).payload == "commit"

        with pytest.raises(ValueError):
            async with db.atomic(using=database.name):
                await execute_raw(
                    "SELECT pg_notify($1, $2)",
                    ["committed", "rollback"],
                    using=database.name,
                )
                raise ValueError("rollback")
        await publish(database, "committed", "barrier")
        assert (await receive(listener)).payload == "barrier"


async def test_close_interrupts_receive_and_iteration(database):
    async with db.listen("close", using=database.name) as listener:
        pending = asyncio.create_task(listener.__anext__())
        await asyncio.sleep(0)
        with pytest.raises(RuntimeError, match="already pending"):
            await listener.recv()

        await asyncio.wait_for(listener.close(), 5)
        with pytest.raises(StopAsyncIteration):
            await asyncio.wait_for(pending, 5)
        await listener.close()
        with pytest.raises(db.ListenerClosedError):
            await listener.recv()
        assert [item async for item in listener] == []


@pytest.mark.parametrize("operation", ["close", "replace", "close_all"])
async def test_named_database_shutdown_closes_listener(database, pg_url, operation):
    async with db.listen("shutdown", using=database.name) as listener:
        pending = asyncio.create_task(listener.__anext__())
        await asyncio.sleep(0)
        if operation == "replace":
            await asyncio.wait_for(
                wrapper.init_pool_overwrite(database.name, pg_url, None), 5
            )
        elif operation == "close_all":
            await asyncio.wait_for(db.close(), 5)
        else:
            await asyncio.wait_for(database.disconnect(), 5)

        with pytest.raises(StopAsyncIteration):
            await asyncio.wait_for(pending, 5)
        with pytest.raises(db.ListenerClosedError):
            await listener.recv()


async def test_listener_does_not_consume_application_pool_capacity(pg_url):
    name = f"capacity_{uuid.uuid4().hex}"
    async with db.connect(
        pg_url, name=name, settings=db.PoolSettings(max_connections=1)
    ) as database:
        async with db.listen("dedicated", using=name) as listener:
            rows = await asyncio.wait_for(
                execute_raw("SELECT 1 AS value", client=database), 5
            )
            assert rows[0]["value"] == 1
            await publish(database, "dedicated")
            assert (await receive(listener)).payload == "hint"

        handle = await wrapper.listen(name, ["abandoned"])
        del handle
        gc.collect()
        await asyncio.wait_for(execute_raw("SELECT 1", client=database), 5)


async def test_cancelled_receive_can_receive_again(database):
    async with db.listen("cancel", using=database.name) as listener:
        for _ in range(20):
            pending = asyncio.create_task(listener.recv())
            await asyncio.sleep(0)
            pending.cancel()
            with pytest.raises(asyncio.CancelledError):
                await pending
        await publish(database, "cancel", "still-usable")
        assert (await receive(listener)).payload == "still-usable"


async def wait_for_resubscription(database, channel, old_pid):
    async def ready():
        while True:
            rows = await execute_raw(
                "SELECT pid FROM pg_stat_activity "
                "WHERE query = $1 AND pid <> $2 AND state = 'idle'",
                [f'LISTEN "{channel}";', old_pid],
                client=database,
            )
            if rows:
                return
            await asyncio.sleep(0.01)

    await asyncio.wait_for(ready(), 5)


async def test_backend_termination_reconnects(database):
    channel = f"terminate_{uuid.uuid4().hex}"
    async with db.listen(channel, using=database.name) as listener:
        query = f'LISTEN "{channel}";'
        rows = await execute_raw(
            "SELECT pid FROM pg_stat_activity WHERE query = $1",
            [query],
            client=database,
        )
        assert len(rows) == 1
        old_pid = rows[0]["pid"]
        await execute_raw(
            "SELECT pg_terminate_backend($1::int)", [old_pid], client=database
        )

        pending = asyncio.create_task(listener.recv())
        await wait_for_resubscription(database, channel, old_pid)
        await publish(database, channel, "restored")
        assert (await asyncio.wait_for(pending, 5)).payload == "restored"


@pytest.mark.parametrize("backend", ["sqlite", "mysql"])
async def test_unsupported_backend(backend, request):
    url = "sqlite::memory:"
    if backend == "mysql":
        container = request.getfixturevalue("_mysql_container")
        if container is None:
            pytest.skip("MySQL container not available")
        host = container.get_container_host_ip()
        port = container.get_exposed_port(3306)
        url = f"mysql://root:test@{host}:{port}/test"

    name = f"unsupported_{backend}_{uuid.uuid4().hex}"
    async with db.connect(url, name=name):
        with pytest.raises(NotImplementedError, match="not supported"):
            async with db.listen("channel", using=name):
                pytest.fail("unsupported listener entered")
