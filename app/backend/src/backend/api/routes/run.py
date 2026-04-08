from __future__ import annotations

from fastapi import APIRouter, Depends, HTTPException
from pydantic import BaseModel
from sqlalchemy.orm import Session

from backend.api.routes.logs import get_db
from backend.db.models import AutomationPlan, AutomationRun, AutomationRunStep, AutomationStep

router = APIRouter()


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


