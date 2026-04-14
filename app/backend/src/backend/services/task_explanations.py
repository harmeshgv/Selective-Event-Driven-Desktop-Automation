from __future__ import annotations

from ai.task_explainer import (
    TaskExplanationInput,
    TaskExplanationResult,
    explain_repeated_task as _explain_impl,
    explain_repeated_task_details as _details_impl,
)


def explain_repeated_task(task: TaskExplanationInput) -> str:
    return _explain_impl(task=task)


def explain_repeated_task_details(task: TaskExplanationInput) -> TaskExplanationResult:
    return _details_impl(task=task)
