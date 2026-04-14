from __future__ import annotations

import logging

from fastapi import FastAPI
from fastapi.middleware.cors import CORSMiddleware

from backend.api.routes.automations import router as automations_router
from backend.api.routes.observer_settings import router as observer_settings_router
from backend.api.routes.logs import router as logs_router
from backend.api.routes.run import router as run_router
from backend.api.routes.tasks import router as tasks_router
from backend.core.config import get_config
from backend.db.models import Base
from backend.db.session import engine
from backend.services.sample_seed import seed_sample_automation

logging.basicConfig(level=logging.INFO)
log = logging.getLogger("backend.main")

cfg = get_config()

app = FastAPI(title="FlowPilot Backend", version="0.0.1")

app.add_middleware(
    CORSMiddleware,
    allow_origins=cfg.cors_origins,
    allow_credentials=True,
    allow_methods=["*"],
    allow_headers=["*"],
)


def _run_migrations(eng) -> None:
    """Add columns that were introduced after initial create_all."""
    from sqlalchemy import inspect, text

    inspector = inspect(eng)
    if "automation_plans" in inspector.get_table_names():
        columns = {c["name"] for c in inspector.get_columns("automation_plans")}
        if "cached_llm_steps" not in columns:
            with eng.begin() as conn:
                conn.execute(text("ALTER TABLE automation_plans ADD COLUMN cached_llm_steps TEXT DEFAULT ''"))
            log.info("Migrated: added cached_llm_steps to automation_plans")


@app.on_event("startup")
def on_startup() -> None:
    Base.metadata.create_all(bind=engine)
    _run_migrations(engine)
    log.info("DB tables ensured")
    # Seed demo content for immediate preview/run testing.
    from backend.db.session import SessionLocal

    db = SessionLocal()
    try:
        seed_sample_automation(db)
    finally:
        db.close()


@app.get("/health")
def health() -> dict:
    return {"ok": True}


app.include_router(logs_router)
app.include_router(tasks_router)
app.include_router(automations_router)
app.include_router(run_router)
app.include_router(observer_settings_router)

