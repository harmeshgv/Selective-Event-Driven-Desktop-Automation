from __future__ import annotations

from datetime import datetime, timezone
from pathlib import Path

from pydantic import BaseModel, Field
from sqlalchemy.orm import Session

from fastapi import APIRouter, Depends, HTTPException

from backend.api.routes.logs import get_db
from backend.core.config import get_config
from backend.db.models import (
    AutomationPlan,
    AutomationRun,
    AutomationRunStep,
    AutomationStep,
    ObservedLog,
    ObserverSettings,
    TaskPattern,
    TaskStep,
)

router = APIRouter(prefix="/observer", tags=["observer"])


class ObserverSettingsOut(BaseModel):
    tracking_enabled: bool
    privacy_mode: bool
    screenshots_enabled: bool
    screenshot_every_seconds: int


class ObserverSettingsIn(BaseModel):
    tracking_enabled: bool
    privacy_mode: bool
    screenshots_enabled: bool
    screenshot_every_seconds: int = Field(ge=5, le=3600)


class ObserverResetOut(BaseModel):
    settings: ObserverSettingsOut
    deleted_logs: int
    deleted_tasks: int
    deleted_task_steps: int
    deleted_automations: int
    deleted_automation_steps: int
    deleted_runs: int
    deleted_run_steps: int
    deleted_screenshots: int


def _get_singleton(db: Session) -> ObserverSettings:
    row = db.query(ObserverSettings).filter(ObserverSettings.id == 1).one_or_none()
    if row is None:
        cfg = get_config()
        row = ObserverSettings(
            id=1,
            tracking_enabled=cfg.default_tracking_enabled,
            privacy_mode=cfg.default_privacy_mode,
            screenshots_enabled=cfg.default_screenshots_enabled,
            screenshot_every_seconds=30,
        )
        db.add(row)
        db.commit()
        db.refresh(row)
    return row


def _serialize_settings(row: ObserverSettings) -> ObserverSettingsOut:
    return ObserverSettingsOut(
        tracking_enabled=row.tracking_enabled,
        privacy_mode=row.privacy_mode,
        screenshots_enabled=row.screenshots_enabled,
        screenshot_every_seconds=row.screenshot_every_seconds,
    )


def _apply_settings(row: ObserverSettings, payload: ObserverSettingsIn) -> None:
    row.tracking_enabled = payload.tracking_enabled
    row.privacy_mode = payload.privacy_mode
    row.screenshots_enabled = False if payload.privacy_mode else payload.screenshots_enabled
    row.screenshot_every_seconds = payload.screenshot_every_seconds
    row.updated_at = datetime.now(timezone.utc)


def _find_repo_root() -> Path:
    cur = Path(__file__).resolve()
    for parent in [cur] + list(cur.parents):
        if (parent / "docker-compose.yml").exists():
            return parent
    return Path(__file__).resolve().parents[6]


def _safe_delete_file(candidate: Path, *, repo_root: Path) -> bool:
    try:
        resolved = candidate.expanduser().resolve()
    except Exception:
        return False

    if resolved != repo_root and repo_root not in resolved.parents:
        return False
    if not resolved.is_file():
        return False

    try:
        resolved.unlink()
        return True
    except Exception:
        return False


def _delete_workspace_screenshots(paths: list[str]) -> int:
    repo_root = _find_repo_root()
    deleted = 0
    seen: set[str] = set()

    def _delete_once(candidate: Path) -> None:
        nonlocal deleted
        try:
            resolved = str(candidate.expanduser().resolve())
        except Exception:
            return
        if resolved in seen:
            return
        seen.add(resolved)
        if _safe_delete_file(Path(resolved), repo_root=repo_root):
            deleted += 1

    for raw in paths:
        if raw and raw.strip():
            _delete_once(Path(raw.strip()))

    for directory in (repo_root / "app" / "observer" / "screenshots", repo_root / "screenshots"):
        if not directory.exists() or not directory.is_dir():
            continue
        for candidate in directory.glob("screenshot_*.png"):
            _delete_once(candidate)

    return deleted


@router.get("/settings", response_model=ObserverSettingsOut)
def get_observer_settings(db: Session = Depends(get_db)) -> ObserverSettingsOut:
    row = _get_singleton(db)
    return _serialize_settings(row)


@router.put("/settings", response_model=ObserverSettingsOut)
def set_observer_settings(payload: ObserverSettingsIn, db: Session = Depends(get_db)) -> ObserverSettingsOut:
    row = _get_singleton(db)
    _apply_settings(row, payload)
    try:
        db.add(row)
        db.commit()
        db.refresh(row)
    except Exception as e:
        db.rollback()
        raise HTTPException(status_code=400, detail=str(e))

    return _serialize_settings(row)


@router.post("/reset", response_model=ObserverResetOut)
def reset_observer_workspace(db: Session = Depends(get_db)) -> ObserverResetOut:
    row = _get_singleton(db)
    cfg = get_config()
    screenshot_paths = [
        path
        for (path,) in db.query(ObservedLog.screenshot_path).filter(ObservedLog.screenshot_path != "").all()
        if path
    ]

    try:
        deleted_run_steps = db.query(AutomationRunStep).delete(synchronize_session=False)
        deleted_runs = db.query(AutomationRun).delete(synchronize_session=False)
        deleted_automation_steps = db.query(AutomationStep).delete(synchronize_session=False)
        deleted_automations = db.query(AutomationPlan).delete(synchronize_session=False)
        deleted_task_steps = db.query(TaskStep).delete(synchronize_session=False)
        deleted_tasks = db.query(TaskPattern).delete(synchronize_session=False)
        deleted_logs = db.query(ObservedLog).delete(synchronize_session=False)

        row.tracking_enabled = False
        row.privacy_mode = cfg.default_privacy_mode
        row.screenshots_enabled = cfg.default_screenshots_enabled
        row.screenshot_every_seconds = 30
        row.updated_at = datetime.now(timezone.utc)
        db.add(row)
        db.commit()
        db.refresh(row)
    except Exception as e:
        db.rollback()
        raise HTTPException(status_code=400, detail=str(e))

    deleted_screenshots = _delete_workspace_screenshots(screenshot_paths)

    return ObserverResetOut(
        settings=_serialize_settings(row),
        deleted_logs=int(deleted_logs),
        deleted_tasks=int(deleted_tasks),
        deleted_task_steps=int(deleted_task_steps),
        deleted_automations=int(deleted_automations),
        deleted_automation_steps=int(deleted_automation_steps),
        deleted_runs=int(deleted_runs),
        deleted_run_steps=int(deleted_run_steps),
        deleted_screenshots=int(deleted_screenshots),
    )

