from __future__ import annotations

from datetime import datetime, timezone

from fastapi import APIRouter, Depends
from pydantic import BaseModel
from sqlalchemy.orm import Session

from backend.api.routes.logs import get_db
from backend.db.models import UserConfig

router = APIRouter(prefix="/settings", tags=["settings"])


def _mask_key(key: str) -> str:
    """Show only the last 4 characters of a secret, rest replaced with bullets."""
    if not key or len(key) <= 4:
        return key
    return "\u2022" * (len(key) - 4) + key[-4:]


class UserConfigOut(BaseModel):
    task_explainer_api_key: str
    task_explainer_api_key_set: bool
    automation_llm_api_key: str
    automation_llm_api_key_set: bool
    chrome_path: str


class UserConfigIn(BaseModel):
    task_explainer_api_key: str | None = None
    automation_llm_api_key: str | None = None
    chrome_path: str = ""


def _get_singleton(db: Session) -> UserConfig:
    row = db.query(UserConfig).filter(UserConfig.id == 1).one_or_none()
    if row is None:
        row = UserConfig(id=1)
        db.add(row)
        db.commit()
        db.refresh(row)
    return row


def _serialize(row: UserConfig) -> UserConfigOut:
    return UserConfigOut(
        task_explainer_api_key=_mask_key(row.task_explainer_api_key or ""),
        task_explainer_api_key_set=bool(row.task_explainer_api_key and row.task_explainer_api_key.strip()),
        automation_llm_api_key=_mask_key(row.automation_llm_api_key or ""),
        automation_llm_api_key_set=bool(row.automation_llm_api_key and row.automation_llm_api_key.strip()),
        chrome_path=row.chrome_path or "",
    )


def _is_masked(value: str | None) -> bool:
    """Return True if the value looks like our masked placeholder."""
    if not value:
        return False
    return "\u2022" in value


@router.get("/config", response_model=UserConfigOut)
def get_user_config(db: Session = Depends(get_db)) -> UserConfigOut:
    return _serialize(_get_singleton(db))


@router.put("/config", response_model=UserConfigOut)
def update_user_config(payload: UserConfigIn, db: Session = Depends(get_db)) -> UserConfigOut:
    row = _get_singleton(db)

    if payload.task_explainer_api_key is not None and not _is_masked(payload.task_explainer_api_key):
        row.task_explainer_api_key = payload.task_explainer_api_key.strip()

    if payload.automation_llm_api_key is not None and not _is_masked(payload.automation_llm_api_key):
        row.automation_llm_api_key = payload.automation_llm_api_key.strip()

    row.chrome_path = payload.chrome_path.strip()
    row.updated_at = datetime.now(timezone.utc)

    db.commit()
    db.refresh(row)

    _invalidate_ai_config_caches()

    return _serialize(row)


def _invalidate_ai_config_caches() -> None:
    """Reset cached env/config in the AI modules so they re-read from DB."""
    try:
        import ai.llm_executor as _llm_mod
        _llm_mod._ENV = None
        _llm_mod._DB_CONFIG = None
    except Exception:
        pass
    try:
        import ai.task_explainer as _exp_mod
        _exp_mod._DOTENV_CACHE = None
        _exp_mod._DB_CONFIG = None
    except Exception:
        pass


def get_user_config_dict(db: Session) -> dict[str, str]:
    """Return raw (unmasked) user-specific config as a dict. Used by AI modules."""
    row = _get_singleton(db)
    return {
        "TASK_EXPLAINER_API_KEY": row.task_explainer_api_key or "",
        "AUTOMATION_LLM_API_KEY": row.automation_llm_api_key or "",
        "CHROME_PATH": row.chrome_path or "",
    }
