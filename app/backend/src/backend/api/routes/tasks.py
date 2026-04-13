from __future__ import annotations

from fastapi import APIRouter, Depends, Query
from sqlalchemy.orm import Session

from pydantic import BaseModel, Field

from backend.api.routes.logs import get_db
from backend.services.task_discovery import (
    TaskGroupResult,
    discover_and_persist_tasks,
    load_persisted_tasks_snapshot,
)
from backend.services.task_explanations import explain_repeated_task_details
from ai.task_explainer import TaskExplanationInput

router = APIRouter()


class TaskOut(BaseModel):
    task_id: int
    signature: str
    name: str
    frequency: int
    last_used: str
    steps: list[str]
    confidence_score: float


class ExplainTaskIn(BaseModel):
    task_id: int | None = Field(default=None, ge=1)
    task_name: str = Field(default="", max_length=256)
    signature: str = Field(default="", max_length=2048)
    actions: list[str] = Field(min_length=1, max_length=32)
    repeat_count: int = Field(ge=1, le=100000)
    last_used: str = Field(default="", max_length=128)
    confidence_score: float | None = Field(default=None, ge=0.0, le=1.0)


class ExplainTaskOut(BaseModel):
    explanation: str
    provider: str
    cached: bool
    used_fallback: bool
    is_repeated: bool
    repeated_confidence: float
    repeated_reason: str


def _task_group_to_out(r: TaskGroupResult) -> TaskOut:
    return TaskOut(
        task_id=r.task_id,
        signature=r.signature,
        name=r.name,
        frequency=r.frequency,
        last_used=r.last_used,
        steps=list(r.steps),
        confidence_score=r.confidence_score,
    )


@router.get("/tasks", response_model=list[TaskOut])
def list_tasks(
    limit_logs: int = Query(1000, ge=100, le=5000),
    segment_gap_seconds: int = Query(15, ge=5, le=120),
    min_steps: int = Query(4, ge=2, le=20),
    max_steps: int = Query(12, ge=3, le=30),
    discover: bool = Query(
        False,
        description="If false, return persisted tasks from DB only (fast). If true, run pattern discovery over logs (heavier).",
    ),
    db: Session = Depends(get_db),
) -> list[TaskOut]:
    if discover:
        results = discover_and_persist_tasks(
            db=db,
            limit_logs=limit_logs,
            segment_gap_seconds=segment_gap_seconds,
            min_steps=min_steps,
            max_steps=max_steps,
        )
    else:
        results = load_persisted_tasks_snapshot(db)
    ordered = sorted(results, key=lambda r: r.frequency, reverse=True)
    return [_task_group_to_out(r) for r in ordered]


@router.post("/tasks/explain", response_model=ExplainTaskOut)
def explain_task(payload: ExplainTaskIn) -> ExplainTaskOut:
    result = explain_repeated_task_details(
        TaskExplanationInput(
            task_id=payload.task_id,
            task_name=payload.task_name,
            signature=payload.signature,
            repeat_count=payload.repeat_count,
            last_used=payload.last_used,
            confidence_score=payload.confidence_score,
            actions=payload.actions,
        )
    )
    return ExplainTaskOut(
        explanation=result.explanation,
        provider=result.provider,
        cached=result.cached,
        used_fallback=result.used_fallback,
        is_repeated=result.is_repeated,
        repeated_confidence=result.repeated_confidence,
        repeated_reason=result.repeated_reason,
    )

