from __future__ import annotations

import json
import sys
import threading
from typing import Any, List

from PySide6.QtCore import QEasingCurve, QEvent, QPropertyAnimation, QRect, QSize, Qt
from PySide6.QtGui import QColor, QFont, QFontMetrics, QPainter, QPen
from PySide6.QtWidgets import (
    QApplication,
    QAbstractItemView,
    QFormLayout,
    QGraphicsOpacityEffect,
    QHBoxLayout,
    QLabel,
    QListView,
    QMainWindow,
    QPushButton,
    QStyledItemDelegate,
    QStyle,
    QSpinBox,
    QSplitter,
    QToolButton,
    QVBoxLayout,
    QWidget,
    QTextEdit,
)

from .collector_windows import WindowsCollector
from .config import Config
from .db import Repository, connect, migrate
from .qt_models import ActionsModel, BundlesModel, load_bundles


class _CardListDelegate(QStyledItemDelegate):
    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self._padding_x = 12
        self._padding_y = 10
        self._gap = 4
        self._radius = 10
        self._row_height = 62

    def sizeHint(self, option, index):  # type: ignore[override]
        size = super().sizeHint(option, index)
        size.setHeight(max(size.height(), self._row_height))
        return size

    def paint(self, painter: QPainter, option, index) -> None:  # type: ignore[override]
        painter.save()
        painter.setRenderHint(QPainter.RenderHint.Antialiasing, True)

        title = str(index.data(Qt.ItemDataRole.UserRole + 1) or index.data(Qt.ItemDataRole.DisplayRole) or "")
        subtitle = str(index.data(Qt.ItemDataRole.UserRole + 2) or "")

        enabled = bool(index.flags() & Qt.ItemFlag.ItemIsEnabled)
        selected = bool(option.state & QStyle.StateFlag.State_Selected)  # type: ignore[name-defined]
        hovered = bool(option.state & QStyle.StateFlag.State_MouseOver)  # type: ignore[name-defined]

        # Theme colors (must match stylesheet)
        panel = QColor("#0f172a")
        panel_hover = QColor("#121f3a")
        panel_selected = QColor("#1d4ed8")
        border = QColor("#24304a")
        text = QColor("#e5e7eb")
        muted = QColor("#a6b0c3")

        rect = option.rect.adjusted(8, 6, -8, -6)

        fill = panel_selected if selected else (panel_hover if hovered else panel)
        painter.setPen(QPen(border, 1))
        painter.setBrush(fill)
        painter.drawRoundedRect(rect, self._radius, self._radius)

        if not enabled:
            text = QColor("#9ca3af")
            muted = QColor("#6b7280")

        x = rect.x() + self._padding_x
        y = rect.y() + self._padding_y
        w = rect.width() - self._padding_x * 2

        # Title
        title_font = painter.font()
        title_font.setPointSizeF(max(9.5, title_font.pointSizeF()))
        title_font.setBold(True)
        painter.setFont(title_font)
        painter.setPen(text if not selected else QColor("#f8fafc"))
        fm_title = QFontMetrics(title_font)
        title_elided = fm_title.elidedText(title, Qt.TextElideMode.ElideRight, w)
        painter.drawText(QRect(x, y, w, fm_title.height()), Qt.AlignmentFlag.AlignLeft | Qt.AlignmentFlag.AlignVCenter, title_elided)

        # Subtitle
        sub_font = painter.font()
        sub_font.setBold(False)
        sub_font.setPointSizeF(max(8.5, sub_font.pointSizeF() - 1.0))
        painter.setFont(sub_font)
        painter.setPen(muted if not selected else QColor("#dbeafe"))
        fm_sub = QFontMetrics(sub_font)
        sub_y = y + fm_title.height() + self._gap
        subtitle_elided = fm_sub.elidedText(subtitle, Qt.TextElideMode.ElideRight, w)
        painter.drawText(QRect(x, sub_y, w, fm_sub.height()), Qt.AlignmentFlag.AlignLeft | Qt.AlignmentFlag.AlignVCenter, subtitle_elided)

        painter.restore()


class QtMainWindow(QMainWindow):
    def __init__(self, cfg: Config) -> None:
        super().__init__()
        self.cfg = cfg

        conn = connect(cfg.database_path)
        migrate(conn)
        self.repo = Repository(conn)
        self.collector = WindowsCollector(self.repo)

        self.session_id: str | None = None
        self.bundles: List[dict[str, Any]] = []
        self.actions_for_selected: List[dict[str, Any]] = []

        self.bundle_model: BundlesModel | None = None
        self.actions_model: ActionsModel | None = None

        # Details widgets / animation
        self.details_container: QWidget | None = None
        self._details_effect: QGraphicsOpacityEffect | None = None
        self._details_anim: QPropertyAnimation | None = None

        self._build_ui()
        self._apply_style()

        self._sync_session_state()
        self.refresh_suggestions()

    def _build_ui(self) -> None:
        self.setWindowTitle("SEDA – Repeated Task Explorer (Qt)")
        self.resize(1100, 650)

        central = QWidget(self)
        self.setCentralWidget(central)

        root_layout = QVBoxLayout(central)
        root_layout.setContentsMargins(12, 10, 12, 12)
        root_layout.setSpacing(8)

        # Top bar
        top = QHBoxLayout()
        root_layout.addLayout(top)

        title = QLabel("SEDA")
        title_font = QFont("Segoe UI", 16, QFont.Weight.Bold)
        title.setFont(title_font)
        top.addWidget(title, 0, Qt.AlignmentFlag.AlignLeft)

        subtitle = QLabel("Local repeated-task discovery for your desktop")
        subtitle.setObjectName("subtitleLabel")
        top.addWidget(subtitle, 0, Qt.AlignmentFlag.AlignLeft)

        top.addStretch(1)

        self.status_label = QLabel("Idle")
        self.status_label.setObjectName("statusLabel")
        top.addWidget(self.status_label, 0, Qt.AlignmentFlag.AlignRight)

        # Controls row
        controls = QHBoxLayout()
        controls.setSpacing(8)
        root_layout.addLayout(controls)

        self.start_btn = QPushButton("Start Session")
        self.stop_btn = QPushButton("Stop Session")
        self.clear_btn = QPushButton("Clear Data")
        self.refresh_btn = QPushButton("Refresh Suggestions")
        self.start_btn.setObjectName("primaryButton")
        self.stop_btn.setObjectName("primaryButton")
        self.refresh_btn.setObjectName("secondaryButton")
        self.filter_btn = QPushButton("Apply filter")
        self.filter_btn.setObjectName("secondaryButton")
        self.clear_btn.setObjectName("dangerButton")

        controls.addWidget(self.start_btn)
        controls.addWidget(self.stop_btn)

        controls.addSpacing(16)

        controls.addWidget(self.refresh_btn)
        controls.addWidget(self.clear_btn)
        controls.addStretch(1)

        # Filters
        filters = QHBoxLayout()
        filters.setSpacing(8)
        root_layout.addLayout(filters)

        self.min_steps_spin = QSpinBox()
        self.min_steps_spin.setRange(2, 64)
        self.min_steps_spin.setValue(2)
        self.max_steps_spin = QSpinBox()
        self.max_steps_spin.setRange(2, 64)
        self.max_steps_spin.setValue(12)

        filters.addWidget(QLabel("Min steps"))
        filters.addWidget(self.min_steps_spin)
        filters.addSpacing(8)
        filters.addWidget(QLabel("Max steps"))
        filters.addWidget(self.max_steps_spin)

        filters.addWidget(self.filter_btn)
        filters.addStretch(1)

        # Splitter with three panes
        splitter = QSplitter(Qt.Orientation.Horizontal)
        root_layout.addWidget(splitter, 1)

        # Bundles list
        bundles_panel = QWidget()
        bundles_layout = QVBoxLayout(bundles_panel)
        bundles_layout.setContentsMargins(0, 0, 6, 0)
        bundles_layout.setSpacing(4)

        bundles_label = QLabel("Bundles")
        bundles_label.setFont(QFont("Segoe UI", 10, QFont.Weight.Bold))
        bundles_layout.addWidget(bundles_label)

        self.bundle_list = QListView()
        self.bundle_list.setObjectName("bundleList")
        self.bundle_list.setMouseTracking(True)
        self.bundle_list.setSelectionMode(QAbstractItemView.SelectionMode.SingleSelection)
        self.bundle_list.setUniformItemSizes(True)
        self.bundle_list.setItemDelegate(_CardListDelegate(self.bundle_list))
        bundles_layout.addWidget(self.bundle_list, 1)

        splitter.addWidget(bundles_panel)

        # Actions list
        actions_panel = QWidget()
        actions_layout = QVBoxLayout(actions_panel)
        actions_layout.setContentsMargins(0, 0, 6, 0)
        actions_layout.setSpacing(4)

        actions_label = QLabel("Actions in bundle")
        actions_label.setFont(QFont("Segoe UI", 10, QFont.Weight.Bold))
        actions_layout.addWidget(actions_label)

        self.action_list = QListView()
        self.action_list.setObjectName("actionList")
        self.action_list.setMouseTracking(True)
        self.action_list.setSelectionMode(QAbstractItemView.SelectionMode.SingleSelection)
        self.action_list.setUniformItemSizes(True)
        self.action_list.setItemDelegate(_CardListDelegate(self.action_list))
        actions_layout.addWidget(self.action_list, 1)

        splitter.addWidget(actions_panel)

        # Details panel
        details_panel = QWidget()
        details_layout = QVBoxLayout(details_panel)
        details_layout.setContentsMargins(0, 0, 0, 0)
        details_layout.setSpacing(6)

        details_label = QLabel("Action details")
        details_label.setFont(QFont("Segoe UI", 10, QFont.Weight.Bold))
        details_layout.addWidget(details_label)

        # Summary + raw JSON sidebar
        self.details_container = QWidget()
        details_layout.addWidget(self.details_container, 1)

        sidebar_layout = QVBoxLayout(self.details_container)
        sidebar_layout.setContentsMargins(0, 0, 0, 0)
        sidebar_layout.setSpacing(8)

        self.summary_form = QFormLayout()
        self.summary_form.setLabelAlignment(Qt.AlignmentFlag.AlignRight)
        sidebar_layout.addLayout(self.summary_form)

        # Key/value summary labels
        self.action_label = QLabel("–")
        self.app_label = QLabel("–")
        self.domain_label = QLabel("–")
        self.query_label = QLabel("–")
        self.session_label = QLabel("–")
        self.timestamp_label = QLabel("–")

        for lbl in (
            self.action_label,
            self.app_label,
            self.domain_label,
            self.query_label,
            self.session_label,
            self.timestamp_label,
        ):
            lbl.setObjectName("detailValue")

        self.summary_form.addRow("Action", self.action_label)
        self.summary_form.addRow("App", self.app_label)
        self.summary_form.addRow("Domain", self.domain_label)
        self.summary_form.addRow("Query", self.query_label)
        self.summary_form.addRow("Session", self.session_label)
        self.summary_form.addRow("Timestamp", self.timestamp_label)

        # Raw JSON toggle + text
        self.raw_toggle = QToolButton()
        self.raw_toggle.setText("Show raw JSON")
        self.raw_toggle.setCheckable(True)
        sidebar_layout.addWidget(self.raw_toggle, 0, Qt.AlignmentFlag.AlignLeft)

        self.raw_text = QTextEdit()
        self.raw_text.setObjectName("rawJsonText")
        self.raw_text.setReadOnly(True)
        self.raw_text.setVisible(False)
        sidebar_layout.addWidget(self.raw_text, 1)

        splitter.addWidget(details_panel)

        splitter.setSizes([260, 260, 380])

        # Models
        self.bundle_model = BundlesModel(self)
        self.bundle_list.setModel(self.bundle_model)
        self.actions_model = ActionsModel(self)
        self.action_list.setModel(self.actions_model)

        # Hint / footer
        self.hint_label = QLabel(
            "Tip: Repeat a small workflow a few times, then adjust step filters to focus suggestions."
        )
        self.hint_label.setObjectName("hintLabel")
        root_layout.addWidget(self.hint_label)

        # Subtle fade animation for details
        self._details_effect = QGraphicsOpacityEffect(self.details_container)
        self.details_container.setGraphicsEffect(self._details_effect)
        self._details_effect.setOpacity(1.0)

        # Wire signals
        self.start_btn.clicked.connect(self._on_start_clicked)
        self.stop_btn.clicked.connect(self._on_stop_clicked)
        self.clear_btn.clicked.connect(self._on_clear_clicked)
        self.refresh_btn.clicked.connect(self.refresh_suggestions)
        self.filter_btn.clicked.connect(self.refresh_suggestions)
        self.bundle_list.selectionModel().currentRowChanged.connect(
            lambda current, previous: self._on_bundle_selected(current.row())
        )
        self.action_list.selectionModel().currentRowChanged.connect(
            lambda current, previous: self._on_action_selected(current.row())
        )
        self.raw_toggle.toggled.connect(self._on_raw_toggled)

    def _apply_style(self) -> None:
        self.setStyleSheet(
            """
            QMainWindow {
                background: #020617;
            }
            QLabel {
                color: #e5e7eb;
            }
            QLabel#statusLabel {
                padding: 4px 10px;
                border-radius: 10px;
                background-color: #064e3b;
                color: #bbf7d0;
            }
            QLabel#subtitleLabel {
                color: #a6b0c3;
                padding-left: 8px;
            }
            QLabel#hintLabel {
                color: #9ca3af;
            }

            QPushButton {
                padding: 5px 12px;
                border-radius: 4px;
                background: #0f172a;
                border: 1px solid #1f2937;
                color: #e5e7eb;
            }

            QPushButton#primaryButton {
                background: #1d4ed8;
                border-color: #1d4ed8;
                color: #f8fafc;
                font-weight: 600;
            }
            QPushButton#primaryButton:hover {
                background: #2563eb;
                border-color: #2563eb;
            }
            QPushButton#primaryButton:pressed {
                background: #1e40af;
                border-color: #1e40af;
            }

            QPushButton#secondaryButton:hover {
                background: #121f3a;
                border-color: #24304a;
            }
            QPushButton#secondaryButton:pressed {
                background: #0b1220;
                border-color: #334155;
            }

            QPushButton#dangerButton {
                background: #0f172a;
                border-color: #7f1d1d;
                color: #fecaca;
            }
            QPushButton#dangerButton:hover {
                background: #7f1d1d;
                border-color: #7f1d1d;
                color: #fff1f2;
            }
            QPushButton#dangerButton:pressed {
                background: #991b1b;
                border-color: #991b1b;
            }

            QSpinBox {
                padding: 4px 10px;
                border-radius: 8px;
                background: #0f172a;
                border: 1px solid #1f2937;
                color: #e5e7eb;
            }
            QSpinBox::up-button, QSpinBox::down-button {
                width: 16px;
                border: none;
                background: transparent;
            }

            QListView#bundleList,
            QListView#actionList,
            QTextEdit#rawJsonText {
                background: transparent;
                border: 1px solid #1f2937;
                border-radius: 4px;
            }
            QLabel#detailValue {
                color: #e5e7eb;
            }
            QToolButton {
                color: #38bdf8;
            }
            QToolButton:checked {
                color: #0ea5e9;
            }
            """
        )

    # Session management
    def _sync_session_state(self) -> None:
        if self.session_id:
            self.status_label.setText(f"Collecting (session {self.session_id[:8]})")
            self.start_btn.setEnabled(False)
            self.stop_btn.setEnabled(True)
        else:
            self.status_label.setText("Idle")
            self.start_btn.setEnabled(True)
            self.stop_btn.setEnabled(False)

    def _on_start_clicked(self) -> None:
        if self.session_id:
            return
        self.session_id = self.repo.open_session()
        self.collector.start(self.session_id)
        self._sync_session_state()

    def _on_stop_clicked(self) -> None:
        if not self.session_id:
            return
        self.collector.stop()
        self.repo.close_session(self.session_id)
        self.session_id = None
        self._sync_session_state()
        self.refresh_suggestions()

    def _on_clear_clicked(self) -> None:
        if self.session_id:
            self.collector.stop()
            self.session_id = None
        self.repo.clear_collected()
        self._sync_session_state()
        if self.bundle_model is not None:
            self.bundle_model.set_bundles([])
        if self.actions_model is not None:
            self.actions_model.set_actions([])
        self._clear_details()

    # Data loading & selection
    def refresh_suggestions(self) -> None:
        def worker() -> None:
            min_steps = int(self.min_steps_spin.value())
            max_steps = int(self.max_steps_spin.value())
            bundles = load_bundles(self.repo, min_steps, max_steps)
            self._update_bundles(bundles)

        threading.Thread(target=worker, daemon=True).start()

    def _update_bundles(self, bundles: List[dict[str, Any]]) -> None:
        def apply() -> None:
            self.bundles = bundles
            self.actions_for_selected = []
            if self.bundle_model is not None:
                self.bundle_model.set_bundles(bundles)
            if self.actions_model is not None:
                self.actions_model.set_actions([])
            self._clear_details()

        # Ensure UI updates on main thread
        self.call_in_main_thread(apply)

    def _on_bundle_selected(self, row: int) -> None:
        if row < 0 or row >= len(self.bundles):
            return
        bundle = self.bundles[row]
        run = bundle.get("sample_run") or []
        self.actions_for_selected = run
        self._clear_details()

        if self.actions_model is not None:
            self.actions_model.set_actions(run)

    def _on_action_selected(self, row: int) -> None:
        if row < 0 or row >= len(self.actions_for_selected):
            return
        data = self.actions_for_selected[row]

        def apply_update() -> None:
            self._set_details_content(data)
            self._fade_details_in()

        # Fade out current content, then swap and fade in.
        if self._details_effect is None:
            self._set_details_content(data)
            return

        if self._details_anim is not None and self._details_anim.state() == QPropertyAnimation.Running:
            self._details_anim.stop()

        self._details_anim = QPropertyAnimation(self._details_effect, b"opacity", self)
        self._details_anim.setDuration(150)
        self._details_anim.setStartValue(self._details_effect.opacity())
        self._details_anim.setEndValue(0.0)
        self._details_anim.setEasingCurve(QEasingCurve.InOutQuad)
        self._details_anim.finished.connect(apply_update)
        self._details_anim.start()

    def _set_details_content(self, data: dict[str, Any]) -> None:
        action = str(data.get("action_type") or "–")
        app = data.get("target_app") or data.get("source_app") or "unknown"
        domain = str(data.get("website_domain") or "–")
        query = str(data.get("search_query") or "–")
        session = str(data.get("session_id") or "–")
        timestamp = str(data.get("timestamp_iso") or "–")

        self.action_label.setText(action)
        self.app_label.setText(app)
        self.domain_label.setText(domain)
        self.query_label.setText(query)
        self.session_label.setText(session)
        self.timestamp_label.setText(timestamp)

        pretty_json = json.dumps(data, indent=2, ensure_ascii=False)
        self.raw_text.setPlainText(pretty_json)
        # Reset toggle to hidden each time
        self.raw_toggle.setChecked(False)
        self.raw_text.setVisible(False)

    def _clear_details(self) -> None:
        self.action_label.setText("–")
        self.app_label.setText("–")
        self.domain_label.setText("–")
        self.query_label.setText("–")
        self.session_label.setText("–")
        self.timestamp_label.setText("–")
        self.raw_text.clear()
        self.raw_text.setVisible(False)
        self.raw_toggle.setChecked(False)
        if self._details_effect is not None:
            self._details_effect.setOpacity(1.0)

    def _fade_details_in(self) -> None:
        if self._details_effect is None:
            return
        if self._details_anim is not None and self._details_anim.state() == QPropertyAnimation.Running:
            self._details_anim.stop()
        self._details_anim = QPropertyAnimation(self._details_effect, b"opacity", self)
        self._details_anim.setDuration(200)
        self._details_anim.setStartValue(0.0)
        self._details_anim.setEndValue(1.0)
        self._details_anim.setEasingCurve(QEasingCurve.InOutQuad)
        self._details_anim.start()

    def _on_raw_toggled(self, checked: bool) -> None:
        self.raw_text.setVisible(checked)
        self.raw_toggle.setText("Hide raw JSON" if checked else "Show raw JSON")

    # Utility: safe UI calls from worker threads
    def call_in_main_thread(self, fn: Any) -> None:
        # Simple helper; Qt will run this soon in the event loop.
        QApplication.instance().postEvent(self, _FunctionEvent(fn))

    def customEvent(self, event: QEvent) -> None:  # type: ignore[override]
        if isinstance(event, _FunctionEvent):
            event.fn()


class _FunctionEvent(QEvent):
    def __init__(self, fn: Any) -> None:
        super().__init__(QEvent.Type(QEvent.User + 1))
        self.fn = fn


def main() -> None:
    cfg = Config.from_env()
    app = QApplication(sys.argv)
    window = QtMainWindow(cfg)
    window.show()
    sys.exit(app.exec())
