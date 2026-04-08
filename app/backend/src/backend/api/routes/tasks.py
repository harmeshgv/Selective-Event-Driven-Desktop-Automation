from __future__ import annotations

from fastapi import APIRouter, Query, Depends
from sqlalchemy.orm import Session

from pydantic import BaseModel

from backend.api.routes.logs import get_db
from backend.services.task_discovery import discover_and_persist_tasks

router = APIRouter()


class TaskOut(BaseModel):
    task_id: int
    signature: str
    name: str
    frequency: int
    last_used: str
    steps: list[str]
    confidence_score: float


@router.get("/tasks", response_model=list[TaskOut])
def list_tasks(
    limit_logs: int = Query(1000, ge=100, le=5000),
    segment_gap_seconds: int = Query(15, ge=5, le=120),
    min_steps: int = Query(4, ge=2, le=20),
    max_steps: int = Query(12, ge=3, le=30),
    db: Session = Depends(get_db),
) -> list[TaskOut]:
    results = discover_and_persist_tasks(
        db=db,
        limit_logs=limit_logs,
        segment_gap_seconds=segment_gap_seconds,
        min_steps=min_steps,
        max_steps=max_steps,
    )
    return sorted(results, key=lambda r: r.frequency, reverse=True)

