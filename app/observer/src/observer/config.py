from __future__ import annotations

import os
from dataclasses import dataclass
from pathlib import Path
from typing import Optional

from dotenv import load_dotenv


def _find_env_file(start_dir: Path, filename: str = ".env") -> Optional[Path]:
    cur = start_dir
    for _ in range(10):
        candidate = cur / filename
        if candidate.exists():
            return candidate
        if cur.parent == cur:
            break
        cur = cur.parent
    return None


def load_env() -> None:
    repo_root_guess = Path(__file__).resolve()
    env_file = _find_env_file(repo_root_guess)
    if env_file:
        load_dotenv(env_file, override=False)


@dataclass(frozen=True)
class ObserverConfig:
    backend_base_url: str
    logs_endpoint: str
    settings_endpoint: str
    settings_poll_seconds: int

    screenshot_dir: Path
    screenshot_every_seconds: int
    screenshots_enabled: bool

    mouse_move_hz: int
    max_pending_logs: int
    window_title_cache_ms: int

    mask_text_by_default: bool
    sensitive_window_keywords: list[str]
    capture_window_title_keywords: list[str]

    privacy_mode: bool
    max_text_len: int


def get_config() -> ObserverConfig:
    load_env()

    backend_base_url = os.getenv("OBSERVER_BACKEND_URL", "http://localhost:8000").rstrip("/")
    logs_endpoint = os.getenv("OBSERVER_LOG_ENDPOINT", "/logs")
    settings_endpoint = os.getenv("OBSERVER_SETTINGS_ENDPOINT", "/observer/settings")
    settings_poll_seconds = int(os.getenv("OBSERVER_SETTINGS_POLL_SECONDS", "5"))

    screenshot_dir_raw = os.getenv("SCREENSHOT_DIR", "./screenshots")
    # Resolve relative to current working directory for ease of local dev.
    screenshot_dir = Path(screenshot_dir_raw).expanduser().resolve()

    screenshot_every_seconds = int(os.getenv("SCREENSHOT_EVERY_SECONDS", "30"))
    screenshots_enabled = os.getenv("SCREENSHOTS_ENABLED", "true").lower() == "true"
    mouse_move_hz = max(0, int(os.getenv("MOUSE_MOVE_HZ", "2")))
    max_pending_logs = max(32, int(os.getenv("OBSERVER_MAX_PENDING_LOGS", "512")))
    window_title_cache_ms = max(50, int(os.getenv("WINDOW_TITLE_CACHE_MS", "250")))

    mask_text_by_default = os.getenv("MASK_TEXT_BY_DEFAULT", "true").lower() == "true"
    sensitive_window_keywords = [
        s.strip()
        for s in os.getenv("SENSITIVE_WINDOW_KEYWORDS", "password,pass,credential").split(",")
        if s.strip()
    ]
    capture_window_title_keywords = [
        s.strip()
        for s in os.getenv("CAPTURE_WINDOW_TITLE_KEYWORDS", "").split(",")
        if s.strip()
    ]

    privacy_mode = os.getenv("PRIVACY_MODE", "false").lower() == "true"
    max_text_len = int(os.getenv("MAX_TEXT_LEN", "32"))

    return ObserverConfig(
        backend_base_url=backend_base_url,
        logs_endpoint=logs_endpoint,
        settings_endpoint=settings_endpoint,
        settings_poll_seconds=settings_poll_seconds,
        screenshot_dir=screenshot_dir,
        screenshot_every_seconds=screenshot_every_seconds,
        screenshots_enabled=screenshots_enabled,
        mouse_move_hz=mouse_move_hz,
        max_pending_logs=max_pending_logs,
        window_title_cache_ms=window_title_cache_ms,
        mask_text_by_default=mask_text_by_default,
        sensitive_window_keywords=sensitive_window_keywords,
        capture_window_title_keywords=capture_window_title_keywords,
        privacy_mode=privacy_mode,
        max_text_len=max_text_len,
    )

