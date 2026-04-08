from __future__ import annotations

from dataclasses import dataclass
from typing import Literal


RiskLevel = Literal["low", "medium", "high"]


@dataclass(frozen=True)
class AutomationPlanStep:
    step_order: int
    description: str
    action_type: str
    target: str
    value: str = ""
    retry_count: int = 1


@dataclass(frozen=True)
class AutomationPlan:
    name: str
    risk_level: RiskLevel
    steps: list[AutomationPlanStep]
    # For MVP visibility/debug.
    rationale: str = ""

