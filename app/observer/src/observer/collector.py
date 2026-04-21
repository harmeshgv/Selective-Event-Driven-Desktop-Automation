from __future__ import annotations

import ctypes
import ctypes.wintypes
import logging
import queue
import threading
import time
from dataclasses import dataclass
from typing import Callable, Optional

import mss

from observer.config import ObserverConfig
from observer.http_client import LogHttpClient, SettingsHttpClient
from observer.models import StructuredLog, build_log

_user32 = ctypes.windll.user32
_GetAsyncKeyState = _user32.GetAsyncKeyState
_GetCursorPos = _user32.GetCursorPos
_GetForegroundWindow = _user32.GetForegroundWindow
_GetWindowTextW = _user32.GetWindowTextW
_GetWindowTextLengthW = _user32.GetWindowTextLengthW


def _try_get_active_window_title() -> str:
    try:
        hwnd = _GetForegroundWindow()
        length = _GetWindowTextLengthW(hwnd)
        if length <= 0:
            return "Unknown"
        buf = ctypes.create_unicode_buffer(length + 1)
        _GetWindowTextW(hwnd, buf, length + 1)
        title = buf.value.strip()
        return title or "Unknown"
    except Exception:
        return "Unknown"


def _get_cursor_pos() -> tuple[int, int]:
    pt = ctypes.wintypes.POINT()
    _GetCursorPos(ctypes.byref(pt))
    return pt.x, pt.y


_VK_LBUTTON = 0x01
_VK_RBUTTON = 0x02
_VK_MBUTTON = 0x04

_VK_MAP: dict[int, str] = {
    0x08: "<BACKSPACE>", 0x09: "<TAB>", 0x0D: "<ENTER>", 0x1B: "<ESC>",
    0x20: " ", 0x25: "<LEFT>", 0x26: "<UP>", 0x27: "<RIGHT>", 0x28: "<DOWN>",
    0x2E: "<DELETE>", 0x2D: "<INSERT>", 0x21: "<PAGE_UP>", 0x22: "<PAGE_DOWN>",
    0x23: "<END>", 0x24: "<HOME>",
}
_MODIFIER_VKS = {0x10, 0x11, 0x12, 0xA0, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5, 0x14}


def _vk_to_text(vk: int) -> str:
    if vk in _MODIFIER_VKS:
        return ""
    if vk in _VK_MAP:
        return _VK_MAP[vk]
    if 0x30 <= vk <= 0x39:
        return chr(vk)
    if 0x41 <= vk <= 0x5A:
        shift = bool(_GetAsyncKeyState(0x10) & 0x8000)
        ch = chr(vk)
        return ch if shift else ch.lower()
    if 0xBA <= vk <= 0xC0 or 0xDB <= vk <= 0xDF:
        return ""
    return ""


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
    if _is_sensitive_window(cfg, window_title):
        return "<masked>"
    if cfg.mask_text_by_default and not cfg.sensitive_window_keywords:
        return "<masked>"
    if len(raw) > cfg.max_text_len:
        return raw[: cfg.max_text_len]
    return raw


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
        self._state_lock = threading.Lock()

        self._tracking_enabled = True
        self._privacy_mode = cfg.privacy_mode
        self._screenshots_enabled = cfg.screenshots_enabled
        self._screenshot_every_seconds = cfg.screenshot_every_seconds

        self._min_move_interval = None if cfg.mouse_move_hz <= 0 else 1.0 / cfg.mouse_move_hz
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
                self._log.warning("Send queue full (dropped=%s)", self._dropped_log_count)

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

    def _screenshot_loop(self) -> None:
        self._cfg.screenshot_dir.mkdir(parents=True, exist_ok=True)
        with mss.mss() as sct:
            while not self._stop_event.is_set():
                with self._state_lock:
                    tracking = self._tracking_enabled
                    privacy = self._privacy_mode
                    ss_on = self._screenshots_enabled
                    interval = self._screenshot_every_seconds
                interval = max(5, interval)
                time.sleep(interval)
                if not tracking or privacy or not ss_on:
                    continue
                title = _try_get_active_window_title()
                if not _window_matches_whitelist(self._cfg, title):
                    continue
                try:
                    fname = f"screenshot_{int(time.time())}.png"
                    out = self._cfg.screenshot_dir / fname
                    img = sct.grab(sct.monitors[1])
                    mss.tools.to_png(img.rgb, img.size, output=str(out))
                    self._emit(build_log(app=title, action="screenshot", screenshot_path=str(out)))
                except Exception:
                    continue

    def _input_poll_loop(self) -> None:
        """Poll keyboard and mouse state using ctypes (works without hooks)."""
        prev_keys: set[int] = set()
        prev_lmb = False
        prev_rmb = False
        prev_mmb = False
        last_move_ts = 0.0
        prev_x, prev_y = _get_cursor_pos()

        while not self._stop_event.is_set():
            with self._state_lock:
                tracking = self._tracking_enabled
                privacy = self._privacy_mode
            if not tracking:
                time.sleep(0.1)
                continue

            title = _try_get_active_window_title()
            if not _window_matches_whitelist(self._cfg, title):
                time.sleep(0.05)
                continue

            # Mouse buttons
            lmb = bool(_GetAsyncKeyState(_VK_LBUTTON) & 0x8000)
            rmb = bool(_GetAsyncKeyState(_VK_RBUTTON) & 0x8000)
            mmb = bool(_GetAsyncKeyState(_VK_MBUTTON) & 0x8000)
            cx, cy = _get_cursor_pos()

            if lmb and not prev_lmb:
                self._emit(build_log(app=title, action="mouse_click_press", coordinates=f"x={cx},y={cy}", text="left"))
            if rmb and not prev_rmb:
                self._emit(build_log(app=title, action="mouse_click_press", coordinates=f"x={cx},y={cy}", text="right"))
            if mmb and not prev_mmb:
                self._emit(build_log(app=title, action="mouse_click_press", coordinates=f"x={cx},y={cy}", text="middle"))
            prev_lmb, prev_rmb, prev_mmb = lmb, rmb, mmb

            # Mouse move
            now = time.monotonic()
            if self._min_move_interval is not None and (cx != prev_x or cy != prev_y):
                if now - last_move_ts >= self._min_move_interval:
                    self._emit(build_log(app=title, action="mouse_move", coordinates=f"x={cx},y={cy}"))
                    last_move_ts = now
            prev_x, prev_y = cx, cy

            # Keyboard — scan all key VKs
            for vk in range(0x08, 0xFF):
                if vk in _MODIFIER_VKS:
                    continue
                pressed = bool(_GetAsyncKeyState(vk) & 0x8000)
                was_pressed = vk in prev_keys
                if pressed and not was_pressed:
                    raw = _vk_to_text(vk)
                    if raw:
                        masked = _mask_text_if_needed(self._cfg, privacy_mode=privacy, window_title=title, raw=raw)
                        self._emit(build_log(app=title, action="key_press", text=masked))
                    prev_keys.add(vk)
                elif not pressed and was_pressed:
                    prev_keys.discard(vk)

            time.sleep(0.015)

    def start(self) -> ObserverHandle:
        initial = self._settings_client.get_settings()
        if initial is not None:
            with self._state_lock:
                self._tracking_enabled = initial.tracking_enabled
                self._privacy_mode = initial.privacy_mode
                self._screenshots_enabled = initial.screenshots_enabled
                self._screenshot_every_seconds = initial.screenshot_every_seconds

        threading.Thread(target=self._sender_loop, daemon=True).start()
        threading.Thread(target=self._settings_poll_loop, daemon=True).start()
        threading.Thread(target=self._screenshot_loop, daemon=True).start()
        threading.Thread(target=self._input_poll_loop, daemon=True).start()

        self._log.info("Observer started (polling mode)")

        def _stop() -> None:
            self._stop_event.set()

        return ObserverHandle(stop_fn=_stop)

    def _settings_poll_loop(self) -> None:
        while not self._stop_event.is_set():
            settings = self._settings_client.get_settings()
            if settings is not None:
                with self._state_lock:
                    self._tracking_enabled = settings.tracking_enabled
                    self._privacy_mode = settings.privacy_mode
                    self._screenshots_enabled = settings.screenshots_enabled
                    self._screenshot_every_seconds = settings.screenshot_every_seconds
            time.sleep(max(1, self._cfg.settings_poll_seconds))

