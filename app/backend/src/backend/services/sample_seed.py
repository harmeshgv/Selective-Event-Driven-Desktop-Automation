from __future__ import annotations

from datetime import datetime, timezone

from sqlalchemy.orm import Session

from backend.db.models import AutomationPlan, AutomationStep, TaskPattern


SAMPLE_SIGNATURE = "SAMPLE|BROWSER_SEARCH_CLICK_DDG"


def seed_sample_automation(db: Session) -> None:
    now = datetime.now(timezone.utc)
    task = db.query(TaskPattern).filter(TaskPattern.signature == SAMPLE_SIGNATURE).one_or_none()
    if task is None:
        task = TaskPattern(
            signature=SAMPLE_SIGNATURE,
        name="Open browser, search, click result (DuckDuckGo)",
            frequency=1,
            last_used=now,
        )
        db.add(task)
        db.flush()

    existing_plan = db.query(AutomationPlan).filter(AutomationPlan.task_id == task.id).one_or_none()
    if existing_plan is not None:
        return

    plan = AutomationPlan(
        task_id=task.id,
        name="Sample: DuckDuckGo search",
        risk_level="low",
        plan_text="{}",
        created_at=now,
    )
    db.add(plan)
    db.flush()

    search_query = "flowpilot+desktop+assistant"
    steps = [
        AutomationStep(
            plan_id=plan.id,
            step_order=1,
            description="Open Chrome profile picker so user can choose their account.",
            action_type="chrome_profile_picker",
            target="",
            value="",
            retry_count=1,
        ),
        AutomationStep(
            plan_id=plan.id,
            step_order=2,
            description="Wait for user to pick their Chrome profile.",
            action_type="wait",
            target="",
            value="5",
            retry_count=1,
        ),
        AutomationStep(
            plan_id=plan.id,
            step_order=3,
            description="Open DuckDuckGo search for 'flowpilot desktop assistant'.",
            action_type="chrome_open_url",
            target=f"https://duckduckgo.com/?q={search_query}",
            value="",
            retry_count=2,
        ),
    ]
    for s in steps:
        db.add(s)

    db.commit()

