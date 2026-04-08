from __future__ import annotations

import os
from dataclasses import asdict
from typing import Protocol

from ai.models import AutomationPlan, AutomationPlanStep, RiskLevel


class Planner(Protocol):
    def plan_from_task_steps(self, *, steps: list[str], task_name: str = "") -> AutomationPlan: ...


def _heuristic_risk(steps: list[str]) -> RiskLevel:
    # Very conservative MVP heuristics:
    # - typing/clicking => medium (can have side effects)
    # - pure navigation/open => medium
    # - screenshot-only => low
    joined = " ".join(steps).lower()
    if "screenshot" in joined:
        return "low"
    if "click" in joined or "type" in joined or "key:" in joined:
        return "medium"
    return "medium"


class HeuristicPlanner:
    def plan_from_task_steps(self, *, steps: list[str], task_name: str = "") -> AutomationPlan:
        risk = _heuristic_risk(steps)
        name = task_name or "Suggested automation"

        plan_steps: list[AutomationPlanStep] = []
        for i, s in enumerate(steps, start=1):
            # Translate normalized task signatures into UI-friendly instructions.
            if s.startswith("CLICK:"):
                btn = s.split(":", 1)[1]
                plan_steps.append(
                    AutomationPlanStep(
                        step_order=i,
                        description=f"Click the `{btn}` button (identified heuristically).",
                        # Executor currently can't map UI labels to coordinates/selectors.
                        # Keep as noop until the user edits the step with a concrete target.
                        action_type="noop",
                        target=btn,
                        value="",
                        retry_count=2,
                    )
                )
            elif s == "MOVE":
                plan_steps.append(
                    AutomationPlanStep(
                        step_order=i,
                        description="Move mouse to the relevant UI element (dry-run only in MVP).",
                        action_type="move",
                        target="",
                        value="",
                        retry_count=1,
                    )
                )
            elif s.startswith("KEY:<") or s.startswith("KEY:"):
                plan_steps.append(
                    AutomationPlanStep(
                        step_order=i,
                        description=f"Press keyboard key `{s.replace('KEY:', '')}`.",
                        action_type="key_press",
                        target=s,
                        value="",
                        retry_count=1,
                    )
                )
            elif s == "SPACE":
                plan_steps.append(
                    AutomationPlanStep(
                        step_order=i,
                        description="Press the spacebar.",
                        action_type="type",
                        target="text",
                        value=" ",
                        retry_count=1,
                    )
                )
            elif s.startswith("TYPE_CHAR:"):
                ch = s.split(":", 1)[1]
                plan_steps.append(
                    AutomationPlanStep(
                        step_order=i,
                        description=f"Type character `{ch}`.",
                        action_type="type",
                        target="text",
                        value=ch,
                        retry_count=1,
                    )
                )
            elif s.startswith("TYPE_TEXT:"):
                txt = s.split(":", 1)[1]
                plan_steps.append(
                    AutomationPlanStep(
                        step_order=i,
                        description="Type the required text content.",
                        action_type="type",
                        target="text",
                        value=txt,
                        retry_count=1,
                    )
                )
            else:
                plan_steps.append(
                    AutomationPlanStep(
                        step_order=i,
                        description=f"Perform step: `{s}` (heuristic).",
                        action_type="noop",
                        target=s,
                        value="",
                        retry_count=1,
                    )
                )

        rationale = "Generated with heuristic planning for MVP; swap in LLM provider later."
        return AutomationPlan(name=name, risk_level=risk, steps=plan_steps, rationale=rationale)


def get_planner() -> Planner:
    provider = os.getenv("LLM_PROVIDER", "heuristic").lower()
    if provider == "heuristic":
        return HeuristicPlanner()
    return HeuristicPlanner()

