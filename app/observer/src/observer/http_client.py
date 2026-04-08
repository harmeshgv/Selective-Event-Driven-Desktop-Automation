from __future__ import annotations

import threading
from dataclasses import dataclass
from typing import Optional

import httpx

from observer.config import ObserverConfig
from observer.models import StructuredLog


@dataclass(frozen=True)
class SendResult:
    ok: bool
    status_code: Optional[int] = None
    error: Optional[str] = None


class LogHttpClient:
    def __init__(self, cfg: ObserverConfig) -> None:
        self._cfg = cfg
        self._client = httpx.Client(timeout=3.5)
        self._lock = threading.Lock()

    def post_log(self, log: StructuredLog) -> SendResult:
        url = f"{self._cfg.backend_base_url}{self._cfg.logs_endpoint}"
        try:
            # Requests are issued from a background thread; lock to be safe.
            with self._lock:
                resp = self._client.post(url, json=log.model_dump(), headers={"Content-Type": "application/json"})
            return SendResult(ok=resp.status_code < 300, status_code=resp.status_code, error=None)
        except Exception as e:  # pragma: no cover (runtime dependent)
            return SendResult(ok=False, error=str(e))


@dataclass(frozen=True)
class ObserverSettingsOut:
    tracking_enabled: bool
    privacy_mode: bool
    screenshots_enabled: bool
    screenshot_every_seconds: int


class SettingsHttpClient:
    def __init__(self, cfg: ObserverConfig) -> None:
        self._cfg = cfg
        self._client = httpx.Client(timeout=3.5)
        self._lock = threading.Lock()

    def get_settings(self) -> Optional[ObserverSettingsOut]:
        url = f"{self._cfg.backend_base_url}{self._cfg.settings_endpoint}"
        try:
            with self._lock:
                resp = self._client.get(url, headers={"Content-Type": "application/json"})
            if resp.status_code >= 300:
                return None
            data = resp.json()
            return ObserverSettingsOut(
                tracking_enabled=bool(data.get("tracking_enabled", True)),
                privacy_mode=bool(data.get("privacy_mode", False)),
                screenshots_enabled=bool(data.get("screenshots_enabled", True)),
                screenshot_every_seconds=int(data.get("screenshot_every_seconds", 30)),
            )
        except Exception:  # pragma: no cover
            return None

