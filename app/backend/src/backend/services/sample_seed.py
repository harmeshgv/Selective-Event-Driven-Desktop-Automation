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
        name="Sample: DuckDuckGo search + click",
        risk_level="medium",
        plan_text="{}",
        created_at=now,
    )
    db.add(plan)
    db.flush()

    search_query = "flowpilot desktop assistant"
    steps = [
        AutomationStep(
            plan_id=plan.id,
            step_order=1,
            description="Open DuckDuckGo in the browser.",
            action_type="playwright_navigate",
            target="https://duckduckgo.com/",
            value="",
            retry_count=2,
        ),
        AutomationStep(
            plan_id=plan.id,
            step_order=2,
            description="Type the search query.",
            action_type="playwright_fill",
            target="input[name=q]",
            value=search_query,
            retry_count=2,
        ),
        AutomationStep(
            plan_id=plan.id,
            step_order=3,
            description="Press Enter to run the search.",
            action_type="playwright_press",
            target="input[name=q]",
            value="Enter",
            retry_count=2,
        ),
        AutomationStep(
            plan_id=plan.id,
            step_order=4,
            description="Click the first visible search result.",
            action_type="playwright_click_first_result",
            target="a.result__a, a[data-testid='result-title-a'], h2 a",
            value="",
            retry_count=2,
        ),
    ]
    for s in steps:
        db.add(s)

    db.commit()

