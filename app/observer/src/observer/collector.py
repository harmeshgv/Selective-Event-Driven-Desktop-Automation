from __future__ import annotations

import logging
import queue
import threading
import time
from dataclasses import dataclass
from typing import Callable, Optional

import pyautogui
import mss
from pynput import keyboard, mouse

from observer.config import ObserverConfig
from observer.http_client import LogHttpClient, SettingsHttpClient
from observer.models import StructuredLog, build_log


def _try_get_active_window_title() -> str:
    try:
        import win32gui  # type: ignore

        hwnd = win32gui.GetForegroundWindow()
        title = win32gui.GetWindowText(hwnd)
        title = (title or "").strip()
        return title or "Unknown"
    except Exception:
        return "Unknown"


def _window_matches_whitelist(cfg: ObserverConfig, window_title: str) -> bool:
    if not cfg.capture_window_title_keywords:
        return True
    lower = window_title.lower()
    return any(kw.lower() in lower for kw in cfg.capture_window_title_keywords)


def _is_sensitive_window(cfg: ObserverConfig, window_title: str) -> bool:
    if not cfg.sensitive_window_keywords:
        return False
    lower = window_title.lower()
    return any(kw.lower() in lower for kw in cfg.sensitive_window_keywords)


def _mask_text_if_needed(cfg: ObserverConfig, *, privacy_mode: bool, window_title: str, raw: str) -> str:
    if privacy_mode:
        return "<masked>"

    sensitive = _is_sensitive_window(cfg, window_title)
    if sensitive:
        return "<masked>"

    # If explicitly requested, mask everything when we have no sensitive keywords configured.
    if cfg.mask_text_by_default and not cfg.sensitive_window_keywords:
        return "<masked>"

    if len(raw) > cfg.max_text_len:
        return raw[: cfg.max_text_len]
    return raw


def _key_to_text(k: keyboard.Key | keyboard.KeyCode) -> str:
    if isinstance(k, keyboard.KeyCode):
        # For KeyCode, .char can be None for some keys.
        if k.char is None:
            return ""
        # Normalize newlines etc.
        return str(k.char)
    # Special keys
    name = str(k).replace("Key.", "")
    mapping = {"space": " ", "enter": "<ENTER>", "tab": "<TAB>", "backspace": "<BACKSPACE>"}
    return mapping.get(name.lower(), f"<{name.upper()}>")


@dataclass(frozen=True)
class ObserverHandle:
    stop_fn: Callable[[], None]


class ObserverService:
    def __init__(self, cfg: ObserverConfig) -> None:
        self._cfg = cfg
        self._log = logging.getLogger("observer.collector")
        self._client = LogHttpClient(cfg)
        self._settings_client = SettingsHttpClient(cfg)
        self._stop_event = threading.Event()
        self._log_queue: queue.Queue[StructuredLog] = queue.Queue(maxsize=cfg.max_pending_logs)
        self._screenshot_thread: Optional[threading.Thread] = None
        self._sender_thread: Optional[threading.Thread] = None
        self._settings_thread: Optional[threading.Thread] = None
        self._state_lock = threading.Lock()
        self._window_title_lock = threading.Lock()

        self._tracking_enabled = True
        self._privacy_mode = cfg.privacy_mode
        self._screenshots_enabled = cfg.screenshots_enabled
        self._screenshot_every_seconds = cfg.screenshot_every_seconds

        self._keyboard_listener: Optional[keyboard.Listener] = None
        self._mouse_listener: Optional[mouse.Listener] = None

        # Mouse move sampling
        self._last_move_ts = 0.0
        self._min_move_interval = None if cfg.mouse_move_hz <= 0 else 1.0 / cfg.mouse_move_hz

        self._cached_window_title = "Unknown"
        self._window_title_expires_at = 0.0
        self._dropped_log_count = 0

    def _emit(self, log: StructuredLog) -> None:
        if self._stop_event.is_set():
            return
        with self._state_lock:
            tracking_enabled = self._tracking_enabled
        if not tracking_enabled:
            return
        try:
            self._log_queue.put_nowait(log)
        except queue.Full:
            self._dropped_log_count += 1
            if self._dropped_log_count in {1, 25} or self._dropped_log_count % 100 == 0:
                self._log.warning(
                    "Dropping observer events because the send queue is full (dropped=%s)",
                    self._dropped_log_count,
                )

    def _sender_loop(self) -> None:
        while not self._stop_event.is_set():
            try:
                log = self._log_queue.get(timeout=0.25)
            except queue.Empty:
                continue
            try:
                self._client.post_log(log)
            finally:
                self._log_queue.task_done()

    def _start_sender_thread(self) -> None:
        t = threading.Thread(target=self._sender_loop, daemon=True)
        self._sender_thread = t
        t.start()

    def _get_active_window_title(self) -> str:
        now = time.monotonic()
        with self._window_title_lock:
            if now < self._window_title_expires_at:
                return self._cached_window_title

        title = _try_get_active_window_title()
        ttl_seconds = self._cfg.window_title_cache_ms / 1000.0
        expires_at = now + ttl_seconds
        with self._window_title_lock:
            self._cached_window_title = title
            self._window_title_expires_at = expires_at
            return self._cached_window_title

    def _screenshot_loop(self) -> None:
        self._cfg.screenshot_dir.mkdir(parents=True, exist_ok=True)

        with mss.mss() as sct:
            while not self._stop_event.is_set():
                with self._state_lock:
                    tracking_enabled = self._tracking_enabled
                    privacy_mode = self._privacy_mode
                    screenshots_enabled = self._screenshots_enabled
                    interval_seconds = self._screenshot_every_seconds

                interval_seconds = max(5, interval_seconds)
                time.sleep(interval_seconds)
                if not tracking_enabled or privacy_mode or not screenshots_enabled:
                    continue
                window_title = self._get_active_window_title()
                if not _window_matches_whitelist(self._cfg, window_title):
                    continue
                try:
                    filename = f"screenshot_{int(time.time())}.png"
                    out_path = self._cfg.screenshot_dir / filename

                    monitor = sct.monitors[1]  # primary display
                    sct_img = sct.grab(monitor)
                    mss.tools.to_png(sct_img.rgb, sct_img.size, output=str(out_path))

                    self._emit(
                        build_log(
                            app=window_title,
                            action="screenshot",
                            coordinates="",
                            text="",
                            screenshot_path=str(out_path),
                        )
                    )
                except Exception:
                    # Screenshot failures shouldn't stop observation.
                    continue

    def _start_screenshot_thread(self) -> None:
        t = threading.Thread(target=self._screenshot_loop, daemon=True)
        self._screenshot_thread = t
        t.start()

    def _on_click(self, x: int, y: int, button: mouse.Button, pressed: bool) -> None:
        with self._state_lock:
            tracking_enabled = self._tracking_enabled
        if not tracking_enabled:
            return
        if self._stop_event.is_set():
            return
        window_title = self._get_active_window_title()
        if not _window_matches_whitelist(self._cfg, window_title):
            return

        action = "mouse_click_press" if pressed else "mouse_click_release"
        button_name = getattr(button, "name", str(button))
        self._emit(
            build_log(
                app=window_title,
                action=action,
                coordinates=f"x={x},y={y}",
                text=str(button_name),
                screenshot_path="",
            )
        )

    def _on_move(self, x: int, y: int) -> None:
        if self._min_move_interval is None:
            return

        now = time.monotonic()
        if now - self._last_move_ts < self._min_move_interval:
            return
        self._last_move_ts = now

        with self._state_lock:
            tracking_enabled = self._tracking_enabled
        if self._stop_event.is_set():
            return
        if not tracking_enabled:
            return
        window_title = self._get_active_window_title()
        if not _window_matches_whitelist(self._cfg, window_title):
            return

        self._emit(
            build_log(
                app=window_title,
                action="mouse_move",
                coordinates=f"x={x},y={y}",
                text="",
                screenshot_path="",
            )
        )

    def _on_press(self, k: keyboard.Key | keyboard.KeyCode) -> None:
        with self._state_lock:
            tracking_enabled = self._tracking_enabled
            privacy_mode = self._privacy_mode
        if not tracking_enabled:
            return
        if self._stop_event.is_set():
            return
        window_title = self._get_active_window_title()
        if not _window_matches_whitelist(self._cfg, window_title):
            return

        raw = _key_to_text(k)
        if not raw:
            return

        # Heuristic: ignore modifier-only keys.
        if raw.startswith("<") and any(t in raw for t in ("SHIFT", "CTRL", "ALT")):
            return

        masked = _mask_text_if_needed(
            self._cfg,
            privacy_mode=privacy_mode,
            window_title=window_title,
            raw=raw,
        )

        self._emit(
            build_log(
                app=window_title,
                action="key_press",
                coordinates="",
                text=masked,
                screenshot_path="",
            )
        )

    def start(self) -> ObserverHandle:
        # pyautogui often needs accessibility permissions on Windows; we don't block observation.
        try:
            pyautogui.FAILSAFE = False
        except Exception:
            pass

        # Poll settings so UX changes (privacy/stop recording) take effect quickly.
        # Fetch initial settings so we don't capture a few seconds before the first poll.
        initial = self._settings_client.get_settings()
        if initial is not None:
            with self._state_lock:
                self._tracking_enabled = initial.tracking_enabled
                self._privacy_mode = initial.privacy_mode
                self._screenshots_enabled = initial.screenshots_enabled
                self._screenshot_every_seconds = initial.screenshot_every_seconds

        self._start_sender_thread()

        self._settings_thread = threading.Thread(target=self._settings_poll_loop, daemon=True)
        self._settings_thread.start()

        self._start_screenshot_thread()

        self._mouse_listener = mouse.Listener(on_click=self._on_click, on_move=self._on_move)
        self._keyboard_listener = keyboard.Listener(on_press=self._on_press)

        self._mouse_listener.start()
        self._keyboard_listener.start()

        def _stop() -> None:
            self._stop_event.set()
            try:
                if self._mouse_listener:
                    self._mouse_listener.stop()
            finally:
                if self._keyboard_listener:
                    self._keyboard_listener.stop()

        return ObserverHandle(stop_fn=_stop)

    def _settings_poll_loop(self) -> None:
        # Poll backend and update gating flags without restarting listeners.
        while not self._stop_event.is_set():
            settings = self._settings_client.get_settings()
            if settings is not None:
                with self._state_lock:
                    self._tracking_enabled = settings.tracking_enabled
                    self._privacy_mode = settings.privacy_mode
                    self._screenshots_enabled = settings.screenshots_enabled
                    self._screenshot_every_seconds = settings.screenshot_every_seconds
            time.sleep(max(1, self._cfg.settings_poll_seconds))

