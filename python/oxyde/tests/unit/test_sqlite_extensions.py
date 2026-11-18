"""Real SQLite extension loading through the compiled driver; no optional DB service."""

import asyncio
import shutil
import subprocess
import sys
import uuid

import pytest

from oxyde import AsyncDatabase, PoolSettings, atomic, execute_raw


@pytest.fixture
def extension(tmp_path):
    compiler = shutil.which("cc")
    if not compiler:
        pytest.skip("A C compiler is needed for the SQLite test extension")
    source = tmp_path / "oxyde_marker.c"
    source.write_text(r"""
#include <sqlite3ext.h>
// Use the supplied API table directly, including with Apple headers that disable
// the usual extension macros for their system SQLite. No SQLite library link.
static const sqlite3_api_routines *sqlite_api;
static void marker(sqlite3_context *ctx, int argc, sqlite3_value **argv) {
    (void)argc; (void)argv;
    sqlite_api->result_int(ctx, 42);
}
#ifdef _WIN32
__declspec(dllexport)
#endif
int sqlite3_extension_init(sqlite3 *db, char **err, const sqlite3_api_routines *api) {
    (void)err;
    sqlite_api = api;
    return api->create_function(db, "oxyde_extension_marker", 0,
        SQLITE_UTF8 | SQLITE_DETERMINISTIC, 0, marker, 0, 0);
}
""")
    output = tmp_path / (
        "oxyde_marker.dylib" if sys.platform == "darwin" else "oxyde_marker.so"
    )
    flag = "-dynamiclib" if sys.platform == "darwin" else "-shared"
    subprocess.run(
        [compiler, flag, "-fPIC", str(source), "-o", str(output)],
        check=True,
        capture_output=True,
    )
    return str(output)


@pytest.mark.asyncio
async def test_extension_loaded_on_every_pool_connection(extension, tmp_path):
    database = AsyncDatabase(
        f"sqlite://{tmp_path / 'extension.sqlite'}",
        name=f"extensions_{uuid.uuid4().hex}",
        settings=PoolSettings(
            max_connections=2, min_connections=1, sqlite_extensions=[extension]
        ),
    )
    await database.connect()
    ready = asyncio.Event()
    count = 0

    async def hold_connection():
        nonlocal count
        async with atomic(using=database.name):
            rows = await execute_raw(
                "SELECT oxyde_extension_marker() AS marker", using=database.name
            )
            count += 1
            if count == 2:
                ready.set()
            await asyncio.wait_for(ready.wait(), timeout=10)
            return rows[0]["marker"]

    try:
        assert await asyncio.gather(hold_connection(), hold_connection()) == [42, 42]
        # Loading native code at startup must not enable runtime SQL loading.
        with pytest.raises(Exception, match="not authorized"):
            await execute_raw("SELECT load_extension(?)", [extension], client=database)
    finally:
        await database.disconnect()


@pytest.mark.asyncio
async def test_invalid_extension_fails_pool_initialization(tmp_path):
    database = AsyncDatabase(
        "sqlite::memory:",
        name=f"invalid_extensions_{uuid.uuid4().hex}",
        settings=PoolSettings(
            sqlite_extensions=[str(tmp_path / "missing.so")], acquire_timeout=1
        ),
    )
    try:
        with pytest.raises(Exception):
            await database.connect()
        assert not database.connected
    finally:
        await database.disconnect()
