from __future__ import annotations

import json
import logging

from fastapi import APIRouter, Depends, HTTPException
from fastapi.responses import StreamingResponse
from pydantic import BaseModel
from sqlalchemy.orm import Session

from backend.api.routes.logs import get_db
from backend.db.models import AutomationPlan, AutomationRun, AutomationRunStep, AutomationStep, ObservedLog, TaskPattern, TaskStep

router = APIRouter()
_log = logging.getLogger("run")


class RunAutomationIn(BaseModel):
    automation_id: int
    preview: bool = True
    approved: bool = False


class RunStepOut(BaseModel):
    step_order: int
    description: str
    status: str
    attempts: int
    error: str = ""


class RunAutomationOut(BaseModel):
    automation_id: int
    plan_name: str
    preview: bool
    risk_level: str
    status: str
    steps: list[RunStepOut]
    error: str = ""


@router.post("/run", response_model=RunAutomationOut)
def run_automation(payload: RunAutomationIn, db: Session = Depends(get_db)) -> RunAutomationOut:
    plan = db.query(AutomationPlan).filter(AutomationPlan.id == payload.automation_id).one_or_none()
    if plan is None:
        raise HTTPException(status_code=404, detail="Unknown automation_id")

    steps_rows = (
        db.query(AutomationStep)
        .filter(AutomationStep.plan_id == plan.id)
        .order_by(AutomationStep.step_order.asc())
        .all()
    )

    # Always persist run attempts for observability.
    run = AutomationRun(
        plan_id=plan.id,
        status="queued" if payload.preview else "blocked" if not payload.approved else "running",
        preview="true" if payload.preview else "false",
        error="",
    )
    db.add(run)
    db.commit()
    db.refresh(run)

    # Preview path: never execute side effects.
    if payload.preview:
        return RunAutomationOut(
            automation_id=plan.id,
            plan_name=plan.name,
            preview=True,
            risk_level=plan.risk_level,
            status="previewed",
            steps=[
                RunStepOut(
                    step_order=s.step_order,
                    description=s.description,
                    status="would_execute",
                    attempts=0,
                    error="",
                )
                for s in steps_rows
            ],
        )

    # Execution path: require explicit approval.
    if not payload.approved:
        db.query(AutomationRun).filter(AutomationRun.id == run.id).update(
            {"status": "blocked", "error": "User approval required before execution"}
        )
        db.commit()
        return RunAutomationOut(
            automation_id=plan.id,
            plan_name=plan.name,
            preview=False,
            risk_level=plan.risk_level,
            status="blocked",
            steps=[],
            error="User approval required before execution",
        )

    # Execute via automation engine.
    from automation.engine import execute_automation

    # Store per-step results for observability/auditing.
    exec_results = execute_automation(steps_rows)

    run_steps_rows: list[AutomationRunStep] = []
    for r in exec_results:
        run_steps_rows.append(
            AutomationRunStep(
                run_id=run.id,
                step_order=r.step_order,
                description=r.description,
                status=r.status,
                attempts=r.attempts,
                error=r.error,
            )
        )

    run_status = "success" if all(r.status == "success" for r in exec_results) else "failed"
    run_row = db.query(AutomationRun).filter(AutomationRun.id == run.id).one()
    run_row.status = run_status
    run_row.error = "" if run_status == "success" else "One or more steps failed."
    db.add_all(run_steps_rows)
    db.commit()

    return RunAutomationOut(
        automation_id=plan.id,
        plan_name=plan.name,
        preview=False,
        risk_level=plan.risk_level,
        status=run_status,
        steps=[
            RunStepOut(
                step_order=r.step_order,
                description=r.description,
                status=r.status,
                attempts=r.attempts,
                error=r.error,
            )
            for r in exec_results
        ],
        error="" if run_status == "success" else run_row.error,
    )


# ---------------------------------------------------------------------------
# Smart (LLM-powered) execution with SSE streaming
# ---------------------------------------------------------------------------


class SmartRunIn(BaseModel):
    automation_id: int
    approved: bool = False


def _execute_single_step(step_dict: dict) -> tuple[bool, str]:
    """Bridge between LLM executor step dicts and the automation engine."""
    from dataclasses import dataclass

    @dataclass
    class _Step:
        step_order: int
        description: str
        action_type: str
        target: str
        value: str
        retry_count: int

    s = _Step(
        step_order=step_dict.get("step_order", 1),
        description=step_dict.get("description", ""),
        action_type=step_dict.get("action_type", "noop"),
        target=step_dict.get("target", ""),
        value=step_dict.get("value", ""),
        retry_count=1,
    )

    from automation.engine import execute_automation

    results = execute_automation([s])
    if not results:
        return False, "Engine returned no results"
    r = results[0]
    return r.status == "success", r.error


def _sse_event(event_type: str, data: dict) -> str:
    payload = json.dumps(data, ensure_ascii=True)
    return f"event: {event_type}\ndata: {payload}\n\n"


@router.post("/run/smart")
def run_smart(payload: SmartRunIn, db: Session = Depends(get_db)):
    plan = db.query(AutomationPlan).filter(AutomationPlan.id == payload.automation_id).one_or_none()
    if plan is None:
        raise HTTPException(status_code=404, detail="Unknown automation_id")

    if not payload.approved:
        raise HTTPException(status_code=400, detail="User approval required before smart execution")

    # Fetch the ORIGINAL task and its raw actions — this is what the user actually did.
    task = db.query(TaskPattern).filter(TaskPattern.id == plan.task_id).one_or_none()
    if task is None:
        raise HTTPException(status_code=404, detail="Original task not found for this automation")

    raw_task_steps = (
        db.query(TaskStep)
        .filter(TaskStep.task_id == task.id)
        .order_by(TaskStep.step_order.asc())
        .all()
    )
    raw_actions = [row.step for row in raw_task_steps]

    if not raw_actions:
        raise HTTPException(status_code=400, detail="No raw task actions found — nothing for AI to work with")

    # Pull raw browser window titles from ObservedLog for URL hints.
    # Query recent logs around the task's last_used timestamp.
    from datetime import timedelta
    raw_window_titles: list[str] = []
    if task.last_used:
        window_start = task.last_used - timedelta(hours=2)
        window_end = task.last_used + timedelta(minutes=5)
        browser_logs = (
            db.query(ObservedLog.app)
            .filter(
                ObservedLog.timestamp.between(window_start, window_end),
                ObservedLog.app.isnot(None),
                ObservedLog.app != "",
            )
            .distinct()
            .limit(50)
            .all()
        )
        raw_window_titles = list({row.app for row in browser_logs if row.app})

    run = AutomationRun(
        plan_id=plan.id,
        status="running",
        preview="false",
        error="",
    )
    db.add(run)
    db.commit()
    db.refresh(run)
    # Check for cached LLM plan from a previous successful run.
    cached_plan_json = plan.cached_llm_steps or ""
    cached_steps: list[dict] | None = None
    if cached_plan_json.strip():
        try:
            parsed = json.loads(cached_plan_json)
            if isinstance(parsed, dict) and isinstance(parsed.get("steps"), list):
                cached_steps = parsed["steps"]
                _log.info("Found cached LLM plan with %d steps for automation %d", len(cached_steps), plan.id)
        except (json.JSONDecodeError, TypeError):
            pass

    run_id = run.id
    plan_name = plan.name
    risk_level = plan.risk_level
    plan_id = plan.id
    task_name = task.name or plan.name
    task_frequency = task.frequency
    task_signature = task.signature
    raw_titles = raw_window_titles

    def stream():
        from ai.llm_executor import ExecutionDoneEvent, ExecutionStepEvent, TaskContext, execute_with_llm, execute_cached_steps

        use_cache = cached_steps is not None

        if use_cache:
            yield _sse_event("start", {
                "run_id": run_id,
                "automation_id": plan_id,
                "plan_name": plan_name,
                "risk_level": risk_level,
                "total_steps": len(cached_steps),
                "task_name": task_name,
                "raw_action_count": len(raw_actions),
                "using_cache": True,
            })
            event_gen = execute_cached_steps(cached_steps, _execute_single_step)
        else:
            task_ctx = TaskContext(
                task_name=task_name,
                raw_actions=raw_actions,
                frequency=task_frequency,
                signature=task_signature,
                raw_window_titles=raw_titles,
            )
            yield _sse_event("start", {
                "run_id": run_id,
                "automation_id": plan_id,
                "plan_name": plan_name,
                "risk_level": risk_level,
                "total_steps": len(raw_actions),
                "task_name": task_name,
                "raw_action_count": len(raw_actions),
                "using_cache": False,
            })
            event_gen = execute_with_llm(task_ctx, _execute_single_step)

        final_status = "failed"
        final_error = ""
        run_step_rows = []
        successful_steps = []

        for event in event_gen:
            if isinstance(event, ExecutionStepEvent):
                yield _sse_event("step", {
                    "step_order": event.step_order,
                    "description": event.description,
                    "action_type": event.action_type,
                    "target": event.target,
                    "value": event.value,
                    "status": event.status,
                    "attempts": event.attempts,
                    "error": event.error,
                    "llm_reasoning": event.llm_reasoning,
                })
                if event.status == "success" and event.action_type != "planning":
                    successful_steps.append({
                        "step_order": event.step_order,
                        "description": event.description,
                        "action_type": event.action_type,
                        "target": event.target,
                        "value": event.value,
                    })
                if event.status in ("success", "failed"):
                    run_step_rows.append(
                        AutomationRunStep(
                            run_id=run_id,
                            step_order=event.step_order,
                            description=event.description,
                            status=event.status,
                            attempts=event.attempts,
                            error=event.error,
                        )
                    )
            elif isinstance(event, ExecutionDoneEvent):
                final_status = event.status
                final_error = event.error
                yield _sse_event("done", {
                    "status": event.status,
                    "total_steps": event.total_steps,
                    "completed_steps": event.completed_steps,
                    "error": event.error,
                })

        from backend.db.session import SessionLocal

        persist_db = SessionLocal()
        try:
            run_row = persist_db.query(AutomationRun).filter(AutomationRun.id == run_id).one()
            run_row.status = final_status
            run_row.error = final_error
            for rs in run_step_rows:
                persist_db.add(AutomationRunStep(
                    run_id=run_id,
                    step_order=rs.step_order,
                    description=rs.description,
                    status=rs.status,
                    attempts=rs.attempts,
                    error=rs.error,
                ))

            # Cache successful steps for reuse
            if final_status == "success" and successful_steps:
                plan_row = persist_db.query(AutomationPlan).filter(AutomationPlan.id == plan_id).one()
                plan_row.cached_llm_steps = json.dumps({
                    "intent": task_name,
                    "steps": successful_steps,
                }, ensure_ascii=True)
                _log.info("Cached %d working steps for automation %d", len(successful_steps), plan_id)

            persist_db.commit()
        except Exception:
            _log.exception("Failed to persist smart run results")
        finally:
            persist_db.close()

    return StreamingResponse(
        stream(),
        media_type="text/event-stream",
        headers={
            "Cache-Control": "no-cache",
            "Connection": "keep-alive",
            "X-Accel-Buffering": "no",
        },
    )


