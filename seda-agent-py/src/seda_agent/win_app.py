from __future__ import annotations

import json
import threading
import time
import tkinter as tk
from dataclasses import dataclass
from tkinter import ttk

from .collector_windows import WindowsCollector
from .config import Config
from .db import Repository, connect, migrate
from .mining import build_repeated_task_bundles


@dataclass
class UiState:
    collecting: bool = False
    session_id: str | None = None
    started_ms: int | None = None


class SedaWindowsApp:
    def __init__(self, root: tk.Tk, cfg: Config) -> None:
        self.root = root
        self.cfg = cfg

        conn = connect(cfg.database_path)
        migrate(conn)
        self.repo = Repository(conn)
        self.collector = WindowsCollector(self.repo)

        self.state = UiState()
        self._bundles: list[dict] = []
        self._actions_for_selected: list[dict] = []
        self._detail_after_id: str | None = None
        self._lock = threading.RLock()

        self.root.title("SEDA – Repeated Task Explorer")
        self.root.geometry("960x540")
        self.root.minsize(840, 420)

        self._build_ui()
        self._set_status("Idle")
        self._refresh_loop()

        self.root.protocol("WM_DELETE_WINDOW", self._on_close)

    def _build_ui(self) -> None:
        frm = ttk.Frame(self.root, padding=14)
        frm.pack(fill=tk.BOTH, expand=True)

        # Top bar: title + session status
        header = ttk.Frame(frm)
        header.pack(fill=tk.X)

        title_label = ttk.Label(header, text="SEDA", font=("Segoe UI", 14, "bold"))
        title_label.pack(side=tk.LEFT)

        subtitle = ttk.Label(
            header,
            text="Local repeated-task discovery for your desktop",
            foreground="#555",
        )
        subtitle.pack(side=tk.LEFT, padx=(10, 0))

        self.status_var = tk.StringVar(value="Idle")
        status_label = ttk.Label(
            header,
            textvariable=self.status_var,
            foreground="#006400",
            font=("Segoe UI", 10, "bold"),
        )
        status_label.pack(side=tk.RIGHT)

        # Session controls
        buttons = ttk.Frame(frm)
        buttons.pack(fill=tk.X, pady=(10, 6))

        self.start_btn = ttk.Button(buttons, text="Start Session", command=self.start_session)
        self.stop_btn = ttk.Button(buttons, text="Stop Session", command=self.stop_session)
        self.refresh_btn = ttk.Button(buttons, text="Refresh Suggestions", command=self.refresh_suggestions)
        self.clear_btn = ttk.Button(buttons, text="Clear Data", command=self.clear_data)

        self.start_btn.pack(side=tk.LEFT)
        self.stop_btn.pack(side=tk.LEFT, padx=(8, 0))
        ttk.Separator(buttons, orient=tk.VERTICAL).pack(side=tk.LEFT, padx=10, fill=tk.Y)
        self.refresh_btn.pack(side=tk.LEFT, padx=(0, 0))
        self.clear_btn.pack(side=tk.LEFT, padx=(8, 0))

        sep = ttk.Separator(frm)
        sep.pack(fill=tk.X, pady=(8, 10))

        ttk.Label(frm, text="Repeated Tasks (Suggestions)", font=("Segoe UI", 11, "bold")).pack(anchor="w")

        filters = ttk.Frame(frm)
        filters.pack(fill=tk.X, pady=(4, 2))

        self.min_steps_var = tk.IntVar(value=2)
        self.max_steps_var = tk.IntVar(value=12)

        ttk.Label(filters, text="Min steps").pack(side=tk.LEFT)
        self.min_steps_spin = ttk.Spinbox(filters, from_=2, to=64, width=5, textvariable=self.min_steps_var)
        self.min_steps_spin.pack(side=tk.LEFT, padx=(6, 14))

        ttk.Label(filters, text="Max steps").pack(side=tk.LEFT)
        self.max_steps_spin = ttk.Spinbox(filters, from_=2, to=64, width=5, textvariable=self.max_steps_var)
        self.max_steps_spin.pack(side=tk.LEFT, padx=(6, 14))

        ttk.Button(filters, text="Apply", command=self.refresh_suggestions).pack(side=tk.LEFT)

        # Three-pane layout using a PanedWindow so the user can resize columns.
        paned = ttk.Panedwindow(frm, orient=tk.HORIZONTAL)
        paned.pack(fill=tk.BOTH, expand=True, pady=(8, 0))

        # Left: bundles
        bundles_frame = ttk.Frame(paned, padding=(0, 0, 6, 0))
        paned.add(bundles_frame, weight=2)

        ttk.Label(bundles_frame, text="Bundles", font=("Segoe UI", 10, "bold")).pack(anchor="w")
        self.bundle_list = tk.Listbox(bundles_frame, height=14)
        self.bundle_list.pack(side=tk.LEFT, fill=tk.BOTH, expand=True)
        bundle_scroll = ttk.Scrollbar(bundles_frame, orient=tk.VERTICAL, command=self.bundle_list.yview)
        bundle_scroll.pack(side=tk.RIGHT, fill=tk.Y)
        self.bundle_list.configure(yscrollcommand=bundle_scroll.set)

        # Middle: actions in selected bundle
        actions_frame = ttk.Frame(paned, padding=(0, 0, 6, 0))
        paned.add(actions_frame, weight=2)

        ttk.Label(actions_frame, text="Actions in bundle", font=("Segoe UI", 10, "bold")).pack(anchor="w")
        self.action_list = tk.Listbox(actions_frame, height=14)
        self.action_list.pack(side=tk.LEFT, fill=tk.BOTH, expand=True)
        action_scroll = ttk.Scrollbar(actions_frame, orient=tk.VERTICAL, command=self.action_list.yview)
        action_scroll.pack(side=tk.RIGHT, fill=tk.Y)
        self.action_list.configure(yscrollcommand=action_scroll.set)

        # Right: details of selected action
        detail_frame = ttk.Frame(paned)
        paned.add(detail_frame, weight=3)

        ttk.Label(detail_frame, text="Action details", font=("Segoe UI", 10, "bold")).pack(anchor="w")
        self.detail_text = tk.Text(detail_frame, height=14, wrap="word")
        self.detail_text.pack(side=tk.LEFT, fill=tk.BOTH, expand=True)
        detail_scroll = ttk.Scrollbar(detail_frame, orient=tk.VERTICAL, command=self.detail_text.yview)
        detail_scroll.pack(side=tk.RIGHT, fill=tk.Y)
        self.detail_text.configure(yscrollcommand=detail_scroll.set, state="disabled")

        self.bundle_list.bind("<<ListboxSelect>>", self._on_bundle_select)
        self.action_list.bind("<<ListboxSelect>>", self._on_action_select)

        self.hint_var = tk.StringVar(
            value="Tip: Repeat the same small workflow a few times, then adjust step filters to focus suggestions."
        )
        ttk.Label(frm, textvariable=self.hint_var, foreground="#555").pack(anchor="w", pady=(8, 0))

        self._sync_buttons()

    def _set_status(self, text: str) -> None:
        self.status_var.set(text)

    def _sync_buttons(self) -> None:
        if self.state.collecting:
            self.start_btn.state(["disabled"])
            self.stop_btn.state(["!disabled"])
        else:
            self.start_btn.state(["!disabled"])
            self.stop_btn.state(["disabled"])

    def start_session(self) -> None:
        with self._lock:
            if self.state.collecting:
                return
            session_id = self.repo.open_session()
            self.state.collecting = True
            self.state.session_id = session_id
            self.state.started_ms = int(time.time() * 1000)
            self.collector.start(session_id)
            self._set_status(f"Collecting (session {session_id[:8]})")
            self._sync_buttons()

    def stop_session(self) -> None:
        with self._lock:
            if not self.state.collecting:
                return
            self.collector.stop()
            if self.state.session_id:
                self.repo.close_session(self.state.session_id)
            self.state.collecting = False
            self.state.session_id = None
            self.state.started_ms = None
            self._set_status("Idle")
            self._sync_buttons()
        self.refresh_suggestions()

    def clear_data(self) -> None:
        with self._lock:
            self.collector.stop()
            self.repo.clear_collected()
            self.state = UiState()
            self._set_status("Idle (cleared)")
            self._sync_buttons()
        self.refresh_suggestions()

    def refresh_suggestions(self) -> None:
        # Mining can be a bit heavy; do it off the UI thread.
        def worker() -> None:
            with self._lock:
                actions = self.repo.get_recent_actions_chronological(8000)
            try:
                min_steps = int(self.min_steps_var.get())
            except Exception:
                min_steps = 2
            try:
                max_steps = int(self.max_steps_var.get())
            except Exception:
                max_steps = 12
            min_steps = max(2, min_steps)
            max_steps = max(min_steps, max_steps)
            bundles = build_repeated_task_bundles(
                actions=actions,
                min_pattern_length=min_steps,
                min_occurrences=2,
                limit=25,
                max_pattern_length=min(64, max_steps),
            )
            items = [
                f"{len(b.get('sequence') or [])} steps  |  x{b['frequency']}  |  {b['sequence_label']}"
                for b in bundles
                if b.get("sequence_label")
            ]

            def update() -> None:
                self._bundles = bundles
                self._actions_for_selected = []
                self.bundle_list.delete(0, tk.END)
                self.action_list.delete(0, tk.END)
                self.detail_text.configure(state="normal")
                self.detail_text.delete("1.0", tk.END)
                self.detail_text.configure(state="disabled")

                if not items:
                    self.bundle_list.insert(
                        tk.END,
                        "No repeated tasks yet. Record a bit more and repeat a workflow 2+ times.",
                    )
                else:
                    for item in items[:100]:
                        self.bundle_list.insert(tk.END, item)

            self.root.after(0, update)

        threading.Thread(target=worker, daemon=True).start()

    def _on_bundle_select(self, event: object) -> None:
        if not self._bundles:
            return
        try:
            idx = self.bundle_list.curselection()[0]
        except IndexError:
            return
        if idx >= len(self._bundles):
            return

        bundle = self._bundles[idx]
        run = bundle.get("sample_run") or []
        self._actions_for_selected = run

        self.action_list.delete(0, tk.END)
        self.detail_text.configure(state="normal")
        self.detail_text.delete("1.0", tk.END)
        self.detail_text.configure(state="disabled")

        if not run:
            self.action_list.insert(tk.END, "No actions captured for this bundle.")
            return

        for i, action in enumerate(run, start=1):
            app = action.get("target_app") or action.get("source_app") or "unknown"
            label = f"{i}. {action.get('action_type')} @ {app}"
            self.action_list.insert(tk.END, label)

    def _on_action_select(self, event: object) -> None:
        if not self._actions_for_selected:
            return
        try:
            idx = self.action_list.curselection()[0]
        except IndexError:
            return
        if idx >= len(self._actions_for_selected):
            return

        data = self._actions_for_selected[idx]
        # Build a human-friendly sidebar view: key fields first, then raw JSON.
        header_lines: list[str] = []
        header_lines.append(f"Action: {data.get('action_type')}")
        app = data.get("target_app") or data.get("source_app") or "unknown"
        header_lines.append(f"App: {app}")
        if data.get("website_domain"):
            header_lines.append(f"Domain: {data.get('website_domain')}")
        if data.get("search_query"):
            header_lines.append(f"Query: {data.get('search_query')}")
        header_lines.append(f"Session: {data.get('session_id')}")
        header_lines.append(f"Timestamp: {data.get('timestamp_iso')}")
        header_lines.append("")  # spacer
        header_lines.append("Raw:")

        pretty_json = json.dumps(data, indent=2, ensure_ascii=False)
        all_lines = header_lines + pretty_json.splitlines()

        # Cancel previous animation if any
        if self._detail_after_id is not None:
            try:
                self.root.after_cancel(self._detail_after_id)
            except Exception:
                pass
            self._detail_after_id = None

        self.detail_text.configure(state="normal")
        self.detail_text.delete("1.0", tk.END)
        self.detail_text.configure(state="disabled")

        def step_insert(index: int) -> None:
            if index >= len(all_lines):
                self._detail_after_id = None
                return
            self.detail_text.configure(state="normal")
            self.detail_text.insert(tk.END, all_lines[index] + "\n")
            self.detail_text.see(tk.END)
            self.detail_text.configure(state="disabled")
            self._detail_after_id = self.root.after(18, step_insert, index + 1)

        step_insert(0)

        threading.Thread(target=worker, daemon=True).start()

    def _refresh_loop(self) -> None:
        # Light periodic refresh so suggestions appear without extra clicks.
        if not self.state.collecting:
            self.refresh_suggestions()
        self.root.after(6000, self._refresh_loop)

    def _on_close(self) -> None:
        with self._lock:
            self.collector.stop()
        self.root.destroy()


def main() -> None:
    cfg = Config.from_env()
    root = tk.Tk()
    try:
        style = ttk.Style()
        if "vista" in style.theme_names():
            style.theme_use("vista")
    except Exception:
        pass
    SedaWindowsApp(root, cfg)
    root.mainloop()

