from __future__ import annotations

import hashlib
import time
from dataclasses import dataclass
from typing import Any, Optional


def _hash16(text: str) -> str:
    return hashlib.sha256(text.encode("utf-8", errors="ignore")).hexdigest()[:16]


@dataclass
class RateLimiter:
    window_seconds: int = 3
    max_requests: int = 20
    _bucket: list[float] = None  # type: ignore[assignment]

    def __post_init__(self) -> None:
        if self._bucket is None:
            self._bucket = []

    def allow(self) -> bool:
        now = time.time()
        cutoff = now - self.window_seconds
        self._bucket = [t for t in self._bucket if t >= cutoff]
        if len(self._bucket) >= self.max_requests:
            return False
        self._bucket.append(now)
        return True


class SafetyEnforcer:
    _allowed = {
        "list_windows",
        "get_window_tree",
        "get_patterns",
        "get_transitions",
        "activate_element",
        "press_key",
        "set_clipboard",
        "tools/list",
    }

    def __init__(self) -> None:
        self._limiter = RateLimiter()

    def check(self, method: str) -> Optional[dict[str, Any]]:
        if method not in self._allowed:
            return {"code": -32601, "message": f"Method not found: {method}"}
        if not self._limiter.allow():
            return {"code": -32000, "message": "Rate limit exceeded"}
        return None

    def validate_params(self, method: str, params: Any) -> Optional[dict[str, Any]]:
        if params is None:
            return None
        if not isinstance(params, (dict, list)):
            return {"code": -32602, "message": "Invalid params"}
        if method == "set_clipboard" and isinstance(params, dict):
            text = params.get("text", "")
            if isinstance(text, str) and len(text) > 10240:
                return {"code": -32602, "message": "Text exceeds 10KB limit"}
        return None


def jsonrpc_error(id_value: Any, code: int, message: str) -> dict[str, Any]:
    return {"jsonrpc": "2.0", "id": id_value, "error": {"code": code, "message": message}}


def jsonrpc_success(id_value: Any, result: Any) -> dict[str, Any]:
    return {"jsonrpc": "2.0", "id": id_value, "result": result}


def tools_list() -> dict[str, Any]:
    return {
        "tools": [
            {"name": "list_windows"},
            {"name": "get_window_tree"},
            {"name": "get_patterns"},
            {"name": "get_transitions"},
            {"name": "activate_element"},
            {"name": "press_key"},
            {"name": "set_clipboard"},
            {"name": "tools/list"},
        ]
    }


def privacy_safe_window(title: str, hwnd: str, process_name: str, is_focused: bool) -> dict[str, Any]:
    return {
        "hwnd": hwnd,
        "process_name": process_name,
        "title_hash": _hash16(title),
        "is_focused": is_focused,
        "bounds": None,
    }

