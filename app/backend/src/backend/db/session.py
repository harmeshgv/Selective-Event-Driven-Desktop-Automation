from __future__ import annotations

from sqlalchemy import create_engine, event
from sqlalchemy.orm import sessionmaker

from backend.core.config import get_config

cfg = get_config()

_engine_kwargs: dict = {"pool_pre_ping": True}
if cfg.database_url.startswith("sqlite"):
    # Allow concurrent API workers / observer POSTs + task discovery without immediate "database is locked".
    _engine_kwargs["connect_args"] = {"check_same_thread": False, "timeout": 30.0}

engine = create_engine(cfg.database_url, **_engine_kwargs)


@event.listens_for(engine, "connect")
def _sqlite_concurrency_pragmas(dbapi_connection, connection_record) -> None:
    if engine.dialect.name != "sqlite":
        return
    cursor = dbapi_connection.cursor()
    cursor.execute("PRAGMA journal_mode=WAL")
    cursor.execute("PRAGMA synchronous=NORMAL")
    cursor.execute("PRAGMA busy_timeout=30000")
    cursor.close()


SessionLocal = sessionmaker(bind=engine, autoflush=False, autocommit=False)

