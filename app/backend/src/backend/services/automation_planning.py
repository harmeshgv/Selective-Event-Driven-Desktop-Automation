from __future__ import annotations

import json
from datetime import datetime, timezone

from sqlalchemy.orm import Session

from backend.db.models import AutomationPlan, AutomationStep, TaskPattern, TaskStep

from ai.planner import get_planner


def create_plan_for_task(
    *,
    db: Session,
    task_id: int,
) -> AutomationPlan:
    task = db.query(TaskPattern).filter(TaskPattern.id == task_id).one_or_none()
    if task is None:
        raise ValueError(f"Unknown task_id={task_id}")

    # Load steps from TaskStep table.
    steps_rows = (
        db.query(TaskStep)
        .filter(TaskStep.task_id == task_id)
        .order_by(TaskStep.step_order.asc())
        .all()
    )

    steps = [r.step for r in steps_rows]

    planner = get_planner()
    ai_plan = planner.plan_from_task_steps(steps=steps, task_name=task.name)

    plan = AutomationPlan(
        task_id=task_id,
        name=ai_plan.name,
        risk_level=ai_plan.risk_level,
        plan_text=json.dumps(
            {
                "rationale": ai_plan.rationale,
                "steps": [s.__dict__ for s in ai_plan.steps],
            },
            ensure_ascii=True,
        ),
        created_at=datetime.now(timezone.utc),
    )
    db.add(plan)
    db.flush()  # get plan.id

    for s in ai_plan.steps:
        db.add(
            AutomationStep(
                plan_id=plan.id,
                step_order=s.step_order,
                description=s.description,
                action_type=s.action_type,
                target=s.target,
                value=s.value,
                retry_count=s.retry_count,
            )
        )

    db.commit()
    db.refresh(plan)
    return plan

