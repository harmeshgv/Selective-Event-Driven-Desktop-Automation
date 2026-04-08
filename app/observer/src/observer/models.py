from __future__ import annotations

from datetime import datetime, timezone
from typing import Optional

from pydantic import BaseModel, Field


class StructuredLog(BaseModel):
    timestamp: str
    app: str
    action: str
    coordinates: str = ""
    text: str = ""
    screenshot_path: str = ""

    @staticmethod
    def now() -> str:
        return datetime.now(timezone.utc).isoformat()


def build_log(
    *,
    app: str,
    action: str,
    coordinates: str = "",
    text: str = "",
    screenshot_path: str = "",
) -> StructuredLog:
    return StructuredLog(
        timestamp=StructuredLog.now(),
        app=app,
        action=action,
        coordinates=coordinates,
        text=text,
        screenshot_path=screenshot_path,
    )

