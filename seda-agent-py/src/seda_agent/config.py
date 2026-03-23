from __future__ import annotations

import os
from dataclasses import dataclass
from pathlib import Path


def _default_db_path() -> str:
    local_app_data = os.environ.get("LOCALAPPDATA")
    base = Path(local_app_data) if local_app_data else Path.cwd()
    return str(base / "seda-agent" / "seda.db")


@dataclass(frozen=True)
class Config:
    mcp_port: int = 9315
    database_path: str = _default_db_path()
    min_pattern_frequency: int = 3
    max_pattern_length: int = 10
    action_grouping_window_ms: int = 5000
    debug: bool = False

    @classmethod
    def from_env(cls) -> "Config":
        def getenv_int(name: str, default: int) -> int:
            raw = os.environ.get(name)
            if raw is None or raw.strip() == "":
                return default
            try:
                return int(raw)
            except ValueError:
                return default

        mcp_port = getenv_int("SEDA_MCP_PORT", 9315)
        database_path = os.environ.get("SEDA_DATABASE_PATH", _default_db_path())
        min_pattern_frequency = getenv_int("SEDA_MIN_PATTERN_FREQUENCY", 3)
        max_pattern_length = getenv_int("SEDA_MAX_PATTERN_LENGTH", 10)
        action_grouping_window_ms = getenv_int("SEDA_ACTION_GROUPING_WINDOW_MS", 5000)
        debug = bool(os.environ.get("SEDA_DEBUG"))

        return cls(
            mcp_port=mcp_port,
            database_path=database_path,
            min_pattern_frequency=min_pattern_frequency,
            max_pattern_length=max_pattern_length,
            action_grouping_window_ms=action_grouping_window_ms,
            debug=debug,
        )
