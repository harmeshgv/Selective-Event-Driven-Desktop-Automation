from __future__ import annotations

from datetime import datetime
from typing import List, Optional

from fastapi import APIRouter, Depends, HTTPException, Query
from pydantic import BaseModel, Field
from sqlalchemy.orm import Session

from backend.db.models import ObservedLog
from backend.db.session import SessionLocal


router = APIRouter()


class StructuredLogIn(BaseModel):
    timestamp: datetime
    app: str = Field(min_length=1, max_length=256)
    action: str = Field(min_length=1, max_length=256)
    coordinates: str = ""
    text: str = ""
    screenshot_path: str = ""


class ClearLogsIn(BaseModel):
    confirm: bool
    limit: int = 2000  # delete most recent logs


def get_db() -> Session:
    db = SessionLocal()
    try:
        yield db
    finally:
        db.close()


@router.post("/logs", status_code=201)
def create_log(payload: StructuredLogIn, db: Session = Depends(get_db)) -> dict:
    try:
        row = ObservedLog(
            timestamp=payload.timestamp,
            app=payload.app,
            action=payload.action,
            coordinates=payload.coordinates,
            text=payload.text,
            screenshot_path=payload.screenshot_path,
        )
        db.add(row)
        db.commit()
        db.refresh(row)
        return {"id": row.id}
    except Exception as e:
        db.rollback()
        raise HTTPException(status_code=400, detail=str(e))


@router.get("/logs", response_model=List[dict])
def list_logs(
    limit: int = Query(200, ge=1, le=1000),
    db: Session = Depends(get_db),
) -> List[dict]:
    rows = (
        db.query(ObservedLog)
        .order_by(ObservedLog.timestamp.desc())
        .limit(limit)
        .all()
    )
    return [
        {
            "id": r.id,
            "timestamp": r.timestamp.isoformat(),
            "app": r.app,
            "action": r.action,
            "coordinates": r.coordinates,
            "text": r.text,
            "screenshot_path": r.screenshot_path,
        }
        for r in rows
    ]


@router.delete("/logs")
def clear_logs(payload: ClearLogsIn, db: Session = Depends(get_db)) -> dict:
    if not payload.confirm:
        raise HTTPException(status_code=400, detail="confirm=true required")

    limit = payload.limit
    if limit < 1:
        raise HTTPException(status_code=400, detail="limit must be >= 1")

    try:
        ids = [
            log_id
            for (log_id,) in (
                db.query(ObservedLog.id)
                .order_by(ObservedLog.id.desc())
                .limit(limit)
                .all()
            )
        ]
        if not ids:
            return {"deleted": 0}

        deleted = (
            db.query(ObservedLog)
            .filter(ObservedLog.id.in_(ids))
            .delete(synchronize_session=False)
        )
        db.commit()
        return {"deleted": int(deleted)}
    except Exception as e:
        db.rollback()
        raise HTTPException(status_code=400, detail=str(e))

