from __future__ import annotations

import json
import threading
import time
from dataclasses import dataclass
from typing import Optional

import psutil

# pywin32
import win32clipboard
import win32gui
import win32process

from .db import Repository


@dataclass
class CollectorEvent:
    action_type: str
    source_app: Optional[str] = None
    target_app: Optional[str] = None
    duration_ms: Optional[int] = None
    payload: dict = None  # type: ignore[assignment]


EXCLUDED_APPS = {
    # SEDA itself / Python wrapper processes
    "seda.exe",
    "seda_app.exe",
    "seda_ui.exe",
    "seda-agent.exe",
    "python.exe",
    "py.exe",
}

SYSTEM_NOISE_APPS = {
    # Common Windows shell / UX processes that briefly take focus
    "searchapp.exe",
    "shellexperiencehost.exe",
    "textinputhost.exe",
    "applicationframehost.exe",
    "systemsettings.exe",
    "lockapp.exe",
}


class WindowsCollector:
    def __init__(self, repo: Repository, poll_interval_ms: int = 300) -> None:
        self._repo = repo
        self._poll_interval_ms = max(100, int(poll_interval_ms))
        self._thread: Optional[threading.Thread] = None
        self._stop = threading.Event()
        self._session_id: Optional[str] = None

        self._last_hwnd: Optional[int] = None
        self._last_app: Optional[str] = None
        self._last_focus_ts_ms: Optional[int] = None
        self._last_clip_seq: Optional[int] = None

    def start(self, session_id: str) -> None:
        if self._thread and self._thread.is_alive():
            return
        self._session_id = session_id
        self._stop.clear()
        self._thread = threading.Thread(target=self._run, name="seda-windows-collector", daemon=True)
        self._thread.start()

    def stop(self) -> None:
        self._stop.set()
        if self._thread and self._thread.is_alive():
            self._thread.join(timeout=2.0)
        self._thread = None
        self._flush_focus_duration()
        self._session_id = None

    def _now_ms(self) -> int:
        return int(time.time() * 1000)

    def _get_foreground_app(self) -> tuple[Optional[int], Optional[str]]:
        try:
            hwnd = win32gui.GetForegroundWindow()
            if not hwnd:
                return None, None
            _, pid = win32process.GetWindowThreadProcessId(hwnd)
            if not pid:
                return int(hwnd), None
            try:
                name = psutil.Process(pid).name()
            except Exception:
                name = None
            proc = (name.lower() if isinstance(name, str) else None)
            if proc in EXCLUDED_APPS or proc in SYSTEM_NOISE_APPS:
                # Treat these as "no meaningful foreground app" for our purposes.
                return None, None
            return int(hwnd), proc
        except Exception:
            return None, None

    def _clipboard_signature(self) -> tuple[Optional[int], Optional[str]]:
        # Use Windows clipboard sequence number (cheap) + best-effort format hint.
        try:
            seq = win32clipboard.GetClipboardSequenceNumber()
        except Exception:
            seq = None

        fmt = None
        try:
            if win32clipboard.IsClipboardFormatAvailable(win32clipboard.CF_UNICODETEXT):
                fmt = "text"
            elif win32clipboard.IsClipboardFormatAvailable(win32clipboard.CF_HDROP):
                fmt = "files"
        except Exception:
            fmt = None

        return seq, fmt

    def _store(self, evt: CollectorEvent) -> None:
        if not self._session_id:
            return
        payload = evt.payload or {}
        payload.setdefault("v", 1)
        payload.setdefault("action_type", evt.action_type)
        self._repo.store_action(
            action_type=evt.action_type,
            action_payload=payload,
            timestamp_ms=self._now_ms(),
            session_id=self._session_id,
            duration_ms=evt.duration_ms,
            source_app=evt.source_app,
            target_app=evt.target_app,
        )

    def _flush_focus_duration(self) -> None:
        # We only approximate duration by time between focus changes.
        if not self._last_app or self._last_focus_ts_ms is None:
            return
        duration = max(0, self._now_ms() - self._last_focus_ts_ms)
        self._store(
            CollectorEvent(
                action_type="FOCUS_DURATION",
                target_app=self._last_app,
                duration_ms=duration,
                payload={"target_app": self._last_app, "duration_ms": duration},
            )
        )

    def _run(self) -> None:
        # Seed state
        hwnd, app = self._get_foreground_app()
        self._last_hwnd = hwnd
        self._last_app = app
        self._last_focus_ts_ms = self._now_ms()
        self._store(
            CollectorEvent(
                action_type="OPEN_APP" if app else "FOCUS_UNKNOWN",
                target_app=app,
                payload={"target_app": app, "hwnd": str(hwnd) if hwnd else None},
            )
        )

        self._last_clip_seq, clip_fmt = self._clipboard_signature()
        if self._last_clip_seq is not None:
            self._store(
                CollectorEvent(
                    action_type="CLIPBOARD_STATUS",
                    payload={"sequence": self._last_clip_seq, "format": clip_fmt},
                )
            )

        while not self._stop.is_set():
            now = self._now_ms()
            hwnd, app = self._get_foreground_app()
            if hwnd and (hwnd != self._last_hwnd or app != self._last_app):
                duration = None
                if self._last_focus_ts_ms is not None:
                    duration = max(0, now - self._last_focus_ts_ms)

                self._store(
                    CollectorEvent(
                        action_type="SWITCH_APP",
                        source_app=self._last_app,
                        target_app=app,
                        duration_ms=duration,
                        payload={
                            "from_app": self._last_app,
                            "to_app": app,
                            "from_hwnd": str(self._last_hwnd) if self._last_hwnd else None,
                            "to_hwnd": str(hwnd) if hwnd else None,
                            "duration_ms": duration,
                        },
                    )
                )
                self._last_hwnd = hwnd
                self._last_app = app
                self._last_focus_ts_ms = now

            seq, fmt = self._clipboard_signature()
            if seq is not None and self._last_clip_seq is not None and seq != self._last_clip_seq:
                # We can't reliably distinguish copy vs programmatic set without hooks,
                # so store privacy-safe metadata only.
                self._store(
                    CollectorEvent(
                        action_type="COPY_TEXT" if fmt == "text" else "CLIPBOARD_CHANGED",
                        source_app=self._last_app,
                        payload={"sequence": seq, "format": fmt, "source_app": self._last_app},
                    )
                )
                self._last_clip_seq = seq

            time.sleep(self._poll_interval_ms / 1000.0)


def debug_dump_action(action_data: str) -> str:
    try:
        return json.dumps(json.loads(action_data), indent=2)
    except Exception:
        return action_data

