from __future__ import annotations

from typing import Any, List

from PySide6.QtCore import Qt
from PySide6.QtGui import QStandardItem, QStandardItemModel

from .db import Repository
from .mining import build_repeated_task_bundles


class BundlesModel(QStandardItemModel):
    """
    Simple view-model wrapper for repeated-task bundles.
    Each row stores the original bundle dict in Qt.UserRole.
    """

    def __init__(self, parent: Any | None = None) -> None:
        super().__init__(parent)
        self.bundles: List[dict[str, Any]] = []

    def set_bundles(self, bundles: List[dict[str, Any]]) -> None:
        self.clear()
        self.bundles = bundles

        if not bundles:
            item = QStandardItem(
                "No repeated tasks yet. Record more and repeat a workflow 2+ times."
            )
            item.setEnabled(False)
            self.appendRow(item)
            return

        for b in bundles:
            steps = len(b.get("sequence") or [])
            freq = b.get("frequency")
            label = b.get("sequence_label") or ""
            title = f"{steps} steps · x{freq}"
            subtitle = str(label)
            item = QStandardItem(title)
            item.setData(b, role=Qt.ItemDataRole.UserRole)
            item.setData(title, role=Qt.ItemDataRole.UserRole + 1)
            item.setData(subtitle, role=Qt.ItemDataRole.UserRole + 2)
            self.appendRow(item)


class ActionsModel(QStandardItemModel):
    """
    View-model for actions in the selected bundle.
    Each row stores the original action dict in Qt.UserRole.
    """

    def __init__(self, parent: Any | None = None) -> None:
        super().__init__(parent)
        self.actions: List[dict[str, Any]] = []

    def set_actions(self, actions: List[dict[str, Any]]) -> None:
        self.clear()
        self.actions = actions

        if not actions:
            item = QStandardItem("No actions captured for this bundle.")
            item.setEnabled(False)
            self.appendRow(item)
            return

        for i, action in enumerate(actions, start=1):
            app = action.get("target_app") or action.get("source_app") or "unknown"
            title = f"{i}. {action.get('action_type')}"
            subtitle = f"App: {app}"
            item = QStandardItem(title)
            item.setData(action, role=Qt.ItemDataRole.UserRole)
            item.setData(title, role=Qt.ItemDataRole.UserRole + 1)
            item.setData(subtitle, role=Qt.ItemDataRole.UserRole + 2)
            self.appendRow(item)


def load_bundles(repo: Repository, min_steps: int, max_steps: int) -> List[dict[str, Any]]:
    """
    Helper function to load and mine bundles for the Qt UI.
    """
    actions = repo.get_recent_actions_chronological(8000)
    min_steps = max(2, int(min_steps))
    max_steps = max(min_steps, int(max_steps))
    bundles = build_repeated_task_bundles(
        actions=actions,
        min_pattern_length=min_steps,
        min_occurrences=2,
        limit=25,
        max_pattern_length=min(64, max_steps),
    )
    return bundles

