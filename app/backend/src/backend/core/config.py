from __future__ import annotations

import os
from dataclasses import dataclass
from typing import List

from pydantic_settings import BaseSettings


class Settings(BaseSettings):
    database_url: str = "sqlite:///./dev.db"
    cors_origins: str = "*"
    default_tracking_enabled: bool = True
    default_privacy_mode: bool = False
    default_screenshots_enabled: bool = False

    class Config:
        env_file = ".env"


@dataclass(frozen=True)
class AppConfig:
    database_url: str
    cors_origins: List[str]
    default_tracking_enabled: bool
    default_privacy_mode: bool
    default_screenshots_enabled: bool


def get_config() -> AppConfig:
    s = Settings()  # reads environment/.env via BaseSettings
    cors_raw = s.cors_origins.strip()
    if cors_raw == "*":
        cors_origins = ["*"]
    else:
        cors_origins = [o.strip() for o in cors_raw.split(",") if o.strip()]
    return AppConfig(
        database_url=s.database_url,
        cors_origins=cors_origins,
        default_tracking_enabled=s.default_tracking_enabled,
        default_privacy_mode=s.default_privacy_mode,
        default_screenshots_enabled=s.default_screenshots_enabled,
    )

