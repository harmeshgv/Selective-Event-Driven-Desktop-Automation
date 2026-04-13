from __future__ import annotations

from fastapi import APIRouter, Depends, HTTPException, Query
from pydantic import BaseModel
from sqlalchemy.orm import Session

from backend.api.routes.logs import get_db
from backend.db.models import AutomationPlan, AutomationStep


router = APIRouter()


class AutomationStepOut(BaseModel):
    step_id: int
    step_order: int
    description: str
    action_type: str
    target: str
    value: str = ""
    retry_count: int


class AutomationPlanOut(BaseModel):
    automation_id: int
    task_id: int
    name: str
    risk_level: str
    plan_text: str
    steps: list[AutomationStepOut]


def _plan_to_out(db: Session, plan: AutomationPlan) -> AutomationPlanOut:
    steps = (
        db.query(AutomationStep)
        .filter(AutomationStep.plan_id == plan.id)
        .order_by(AutomationStep.step_order.asc())
        .all()
    )
    return AutomationPlanOut(
        automation_id=plan.id,
        task_id=plan.task_id,
        name=plan.name,
        risk_level=plan.risk_level,
        plan_text=plan.plan_text,
        steps=[
            AutomationStepOut(
                step_id=s.id,
                step_order=s.step_order,
                description=s.description,
                action_type=s.action_type,
                target=s.target,
                value=s.value,
                retry_count=s.retry_count,
            )
            for s in steps
        ],
    )


@router.get("/automations", response_model=list[AutomationPlanOut])
def list_automations(
    task_id: int | None = Query(None),
    limit: int = Query(20, ge=1, le=200),
    db: Session = Depends(get_db),
) -> list[AutomationPlanOut]:
    q = db.query(AutomationPlan)
    if task_id is not None:
        q = q.filter(AutomationPlan.task_id == task_id)
    rows = q.order_by(AutomationPlan.created_at.desc()).limit(limit).all()

    return [_plan_to_out(db, p) for p in rows]


@router.get("/automations/{automation_id}", response_model=AutomationPlanOut)
def get_automation(automation_id: int, db: Session = Depends(get_db)) -> AutomationPlanOut:
    plan = db.query(AutomationPlan).filter(AutomationPlan.id == automation_id).one_or_none()
    if plan is None:
        raise HTTPException(status_code=404, detail="Unknown automation_id")
    return _plan_to_out(db, plan)


class CreateAutomationIn(BaseModel):
    task_id: int


@router.post("/automations", response_model=AutomationPlanOut)
def create_automation(
    payload: CreateAutomationIn,
    db: Session = Depends(get_db),
) -> AutomationPlanOut:
    from backend.services.automation_planning import create_plan_for_task

    try:
        plan = create_plan_for_task(db=db, task_id=payload.task_id)
    except ValueError as e:
        raise HTTPException(status_code=404, detail=str(e))
    except Exception as e:  # pragma: no cover
        raise HTTPException(status_code=400, detail=str(e))

    return _plan_to_out(db, plan)


class UpdateAutomationStepIn(BaseModel):
    step_id: int
    step_order: int
    description: str
    action_type: str
    target: str
    value: str = ""
    retry_count: int = 1


class UpdateAutomationStepsIn(BaseModel):
    steps: list[UpdateAutomationStepIn]


@router.put("/automations/{automation_id}/steps")
def update_automation_steps(
    automation_id: int,
    payload: UpdateAutomationStepsIn,
    db: Session = Depends(get_db),
) -> AutomationPlanOut:
    plan = db.query(AutomationPlan).filter(AutomationPlan.id == automation_id).one_or_none()
    if plan is None:
        raise HTTPException(status_code=404, detail="Unknown automation_id")

    if len(payload.steps) == 0:
        raise HTTPException(status_code=400, detail="Automation must contain at least one step")

    existing_steps = (
        db.query(AutomationStep)
        .filter(AutomationStep.plan_id == automation_id)
        .order_by(AutomationStep.step_order.asc())
        .all()
    )
    existing_ids = {s.id for s in existing_steps}
    requested_ids = {s.step_id for s in payload.steps}
    unknown_ids = [step_id for step_id in requested_ids if step_id not in existing_ids]
    if unknown_ids:
        raise HTTPException(status_code=404, detail=f"Unknown step_id(s) for this automation: {unknown_ids}")

    # Delete steps that were removed in the UI.
    removed_ids = [sid for sid in existing_ids if sid not in requested_ids]
    if removed_ids:
        db.query(AutomationStep).filter(AutomationStep.id.in_(removed_ids)).delete(synchronize_session=False)

    # Re-order by the payload list order (source of truth from UI).
    for idx, s_in in enumerate(payload.steps, start=1):
        step_row = db.query(AutomationStep).filter(AutomationStep.id == s_in.step_id).one()
        step_row.step_order = idx
        step_row.description = s_in.description
        step_row.action_type = s_in.action_type
        step_row.target = s_in.target
        step_row.value = s_in.value
        step_row.retry_count = s_in.retry_count

    db.commit()

    return _plan_to_out(db, plan)

