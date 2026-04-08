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


@router.get("/automations", response_model=list[AutomationPlanOut])
def list_automations(
    limit: int = Query(20, ge=1, le=200),
    db: Session = Depends(get_db),
) -> list[AutomationPlanOut]:
    rows = (
        db.query(AutomationPlan)
        .order_by(AutomationPlan.created_at.desc())
        .limit(limit)
        .all()
    )

    out: list[AutomationPlanOut] = []
    for p in rows:
        steps = (
            db.query(AutomationStep)
            .filter(AutomationStep.plan_id == p.id)
            .order_by(AutomationStep.step_order.asc())
            .all()
        )
        out.append(
            AutomationPlanOut(
                automation_id=p.id,
                task_id=p.task_id,
                name=p.name,
                risk_level=p.risk_level,
                plan_text=p.plan_text,
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
        )
    return out


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

    for s_in in payload.steps:
        step_row = (
            db.query(AutomationStep)
            .filter(AutomationStep.id == s_in.step_id, AutomationStep.plan_id == automation_id)
            .one_or_none()
        )
        if step_row is None:
            raise HTTPException(status_code=404, detail=f"Unknown step_id={s_in.step_id} for this automation")

        step_row.step_order = s_in.step_order
        step_row.description = s_in.description
        step_row.action_type = s_in.action_type
        step_row.target = s_in.target
        step_row.value = s_in.value
        step_row.retry_count = s_in.retry_count

    db.commit()

    # Return updated plan view
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

