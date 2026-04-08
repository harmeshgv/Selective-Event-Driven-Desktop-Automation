from __future__ import annotations

from datetime import datetime

from datetime import timezone

from sqlalchemy import DateTime, ForeignKey, Index, Integer, String, Text
from sqlalchemy.orm import DeclarativeBase, Mapped, mapped_column


class Base(DeclarativeBase):
    pass


class ObservedLog(Base):
    __tablename__ = "observed_logs"

    id: Mapped[int] = mapped_column(primary_key=True, autoincrement=True)
    timestamp: Mapped[datetime] = mapped_column(DateTime(timezone=True), index=True)
    app: Mapped[str] = mapped_column(String(256))
    action: Mapped[str] = mapped_column(String(256))
    coordinates: Mapped[str] = mapped_column(String(256), default="")
    text: Mapped[str] = mapped_column(Text, default="")
    screenshot_path: Mapped[str] = mapped_column(Text, default="")


class TaskPattern(Base):
    __tablename__ = "task_patterns"

    id: Mapped[int] = mapped_column(primary_key=True, autoincrement=True)
    # Signature derived from normalized step sequence.
    signature: Mapped[str] = mapped_column(String(1024), unique=True, index=True)
    name: Mapped[str] = mapped_column(String(256), default="")
    frequency: Mapped[int] = mapped_column(Integer, default=0)
    last_used: Mapped[datetime] = mapped_column(DateTime(timezone=True), index=True)


class TaskStep(Base):
    __tablename__ = "task_steps"

    id: Mapped[int] = mapped_column(primary_key=True, autoincrement=True)
    task_id: Mapped[int] = mapped_column(ForeignKey("task_patterns.id", ondelete="CASCADE"), index=True)
    step_order: Mapped[int] = mapped_column(Integer)
    step: Mapped[str] = mapped_column(String(512))

    __table_args__ = (
        Index("ix_task_steps_task_id_order", "task_id", "step_order"),
    )


class AutomationPlan(Base):
    __tablename__ = "automation_plans"

    id: Mapped[int] = mapped_column(primary_key=True, autoincrement=True)
    task_id: Mapped[int] = mapped_column(ForeignKey("task_patterns.id", ondelete="CASCADE"), index=True)
    name: Mapped[str] = mapped_column(String(256), default="")
    risk_level: Mapped[str] = mapped_column(String(32), default="medium")  # low|medium|high
    # For MVP: store a JSON-ish string representation (will be parsed by backend/clients).
    plan_text: Mapped[str] = mapped_column(Text, default="")
    created_at: Mapped[datetime] = mapped_column(
        DateTime(timezone=True),
        default=lambda: datetime.now(timezone.utc),
    )


class AutomationStep(Base):
    __tablename__ = "automation_steps"

    id: Mapped[int] = mapped_column(primary_key=True, autoincrement=True)
    plan_id: Mapped[int] = mapped_column(ForeignKey("automation_plans.id", ondelete="CASCADE"), index=True)
    step_order: Mapped[int] = mapped_column(Integer)
    # Human readable instruction the UI will present before execution.
    description: Mapped[str] = mapped_column(String(512))
    # Minimal machine-action fields for the automation engine MVP.
    action_type: Mapped[str] = mapped_column(String(64), default="noop")  # e.g. click/type/open
    target: Mapped[str] = mapped_column(String(256), default="")
    value: Mapped[str] = mapped_column(String(256), default="")
    retry_count: Mapped[int] = mapped_column(Integer, default=1)

    __table_args__ = (
        Index("ix_automation_steps_plan_id_order", "plan_id", "step_order"),
    )


class AutomationRun(Base):
    __tablename__ = "automation_runs"

    id: Mapped[int] = mapped_column(primary_key=True, autoincrement=True)
    plan_id: Mapped[int] = mapped_column(ForeignKey("automation_plans.id", ondelete="CASCADE"), index=True)
    created_at: Mapped[datetime] = mapped_column(
        DateTime(timezone=True),
        default=lambda: datetime.now(timezone.utc),
    )
    status: Mapped[str] = mapped_column(String(32), default="queued")  # queued|running|success|failed|blocked
    preview: Mapped[str] = mapped_column(String(16), default="true")  # true|false (string for MVP simplicity)
    error: Mapped[str] = mapped_column(Text, default="")


class AutomationRunStep(Base):
    __tablename__ = "automation_run_steps"

    id: Mapped[int] = mapped_column(primary_key=True, autoincrement=True)
    run_id: Mapped[int] = mapped_column(ForeignKey("automation_runs.id", ondelete="CASCADE"), index=True)
    step_order: Mapped[int] = mapped_column(Integer, index=True)
    description: Mapped[str] = mapped_column(String(512), default="")
    status: Mapped[str] = mapped_column(String(16), default="queued")  # queued|success|failed
    attempts: Mapped[int] = mapped_column(Integer, default=0)
    error: Mapped[str] = mapped_column(Text, default="")

    __table_args__ = (
        Index("ix_run_steps_run_id_step_order", "run_id", "step_order"),
    )


class ObserverSettings(Base):
    __tablename__ = "observer_settings"

    # MVP: single-row settings table (id=1).
    id: Mapped[int] = mapped_column(Integer, primary_key=True, autoincrement=False)
    tracking_enabled: Mapped[bool] = mapped_column(default=True)
    privacy_mode: Mapped[bool] = mapped_column(default=False)
    screenshots_enabled: Mapped[bool] = mapped_column(default=True)
    screenshot_every_seconds: Mapped[int] = mapped_column(default=30)
    # When true, observer will drop logs immediately instead of sending them.
    created_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), default=lambda: datetime.now(timezone.utc))
    updated_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), default=lambda: datetime.now(timezone.utc))

