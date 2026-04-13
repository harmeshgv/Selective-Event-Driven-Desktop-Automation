from __future__ import annotations

from ai.task_explainer import (
    TaskExplanationInput,
    TaskExplanationResult,
    explain_repeated_task as explain_repeated_task_impl,
    explain_repeated_task_details as explain_repeated_task_details_impl,
)


def explain_repeated_task(task: TaskExplanationInput) -> str:
    return explain_repeated_task_impl(task=task)


def explain_repeated_task_details(task: TaskExplanationInput) -> TaskExplanationResult:
    return explain_repeated_task_details_impl(task=task)
