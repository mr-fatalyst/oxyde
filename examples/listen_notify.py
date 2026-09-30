"""Run with DATABASE_URL=postgres://... python examples/listen_notify.py.

In another session: SELECT pg_notify('run_changed', 'run-123');
Replace reconcile() with reads of your authoritative application state.
"""

from __future__ import annotations

import asyncio
import os

from oxyde import db, execute_raw


async def reconcile() -> None:
    rows = await execute_raw("SELECT current_timestamp AS checked_at")
    print("Reconciled state:", rows[0]["checked_at"])


async def main() -> None:
    async with (
        db.connect(
            os.environ["DATABASE_URL"], settings=db.PoolSettings(max_connections=4)
        ),
        db.listen("run_changed") as listener,
    ):
        await reconcile()
        while True:
            try:
                notification = await asyncio.wait_for(listener.recv(), timeout=30)
                print("Wake-up hint:", notification.payload)
            except asyncio.TimeoutError:
                pass
            await reconcile()


if __name__ == "__main__":
    asyncio.run(main())
