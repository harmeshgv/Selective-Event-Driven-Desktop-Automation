from __future__ import annotations

import json
import sqlite3
import time
import uuid
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Optional


def _utc_ms() -> int:
    return int(time.time() * 1000)


def connect(db_path: str) -> sqlite3.Connection:
    Path(db_path).parent.mkdir(parents=True, exist_ok=True)
    conn = sqlite3.connect(db_path, check_same_thread=False)
    conn.row_factory = sqlite3.Row
    conn.execute("PRAGMA journal_mode=WAL;")
    conn.execute("PRAGMA foreign_keys=ON;")
    return conn


def migrate(conn: sqlite3.Connection) -> None:
    conn.executescript(
        """
        CREATE TABLE IF NOT EXISTS symbolic_actions (
          id INTEGER PRIMARY KEY AUTOINCREMENT,
          action_type TEXT NOT NULL,
          action_data TEXT NOT NULL,
          timestamp_ms INTEGER NOT NULL,
          session_id TEXT NOT NULL,
          duration_ms INTEGER,
          source_app TEXT,
          target_app TEXT
        );

        CREATE INDEX IF NOT EXISTS idx_actions_ts ON symbolic_actions(timestamp_ms);
        CREATE INDEX IF NOT EXISTS idx_actions_session ON symbolic_actions(session_id);

        CREATE TABLE IF NOT EXISTS action_transitions (
          id INTEGER PRIMARY KEY AUTOINCREMENT,
          from_action_type TEXT NOT NULL,
          to_action_type TEXT NOT NULL,
          from_app TEXT,
          to_app TEXT,
          frequency INTEGER NOT NULL,
          total_duration_ms INTEGER NOT NULL,
          last_seen_ms INTEGER NOT NULL,
          UNIQUE(from_action_type, to_action_type, from_app, to_app)
        );

        CREATE TABLE IF NOT EXISTS detected_patterns (
          id INTEGER PRIMARY KEY AUTOINCREMENT,
          pattern_hash TEXT NOT NULL UNIQUE,
          sequence TEXT NOT NULL,
          frequency INTEGER NOT NULL,
          avg_duration_ms INTEGER,
          confidence REAL NOT NULL,
          first_seen_ms INTEGER NOT NULL,
          last_seen_ms INTEGER NOT NULL,
          user_dismissed INTEGER NOT NULL DEFAULT 0,
          user_accepted INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS audit_log (
          id INTEGER PRIMARY KEY AUTOINCREMENT,
          audit_id TEXT NOT NULL UNIQUE,
          timestamp_ms INTEGER NOT NULL,
          operation TEXT NOT NULL,
          parameters_hash TEXT,
          result TEXT NOT NULL,
          error_message TEXT,
          caller TEXT
        );

        CREATE TABLE IF NOT EXISTS sessions (
          id TEXT PRIMARY KEY,
          started_ms INTEGER NOT NULL,
          ended_ms INTEGER,
          action_count INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS automation_plans (
          pattern_hash TEXT PRIMARY KEY,
          created_ms INTEGER NOT NULL,
          updated_ms INTEGER NOT NULL,
          plan_version INTEGER NOT NULL,
          source_last_seen_ms INTEGER,
          plan_json TEXT NOT NULL
        );
        """
    )
    conn.commit()


@dataclass
class StoredAction:
    id: int
    action_type: str
    action_data: str
    timestamp_ms: int
    session_id: str
    duration_ms: Optional[int]
    source_app: Optional[str]
    target_app: Optional[str]


@dataclass
class StoredTransition:
    from_action_type: str
    to_action_type: str
    from_app: Optional[str]
    to_app: Optional[str]
    frequency: int
    total_duration_ms: int
    last_seen_ms: int

    def avg_duration_ms(self) -> float:
        return 0.0 if self.frequency <= 0 else self.total_duration_ms / float(self.frequency)


class Repository:
    def __init__(self, conn: sqlite3.Connection):
        self._conn = conn

    def record_audit(
        self,
        operation: str,
        parameters_hash: Optional[str],
        result: str,
        error_message: Optional[str] = None,
        caller: Optional[str] = None,
    ) -> str:
        audit_id = str(uuid.uuid4())
        self._conn.execute(
            """
            INSERT INTO audit_log(audit_id, timestamp_ms, operation, parameters_hash, result, error_message, caller)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            """,
            (audit_id, _utc_ms(), operation, parameters_hash, result, error_message, caller),
        )
        self._conn.commit()
        return audit_id

    def open_session(self) -> str:
        session_id = str(uuid.uuid4())
        self._conn.execute(
            "INSERT INTO sessions(id, started_ms, ended_ms, action_count) VALUES (?, ?, NULL, 0)",
            (session_id, _utc_ms()),
        )
        self._conn.commit()
        return session_id

    def close_session(self, session_id: str) -> None:
        self._conn.execute(
            "UPDATE sessions SET ended_ms = ? WHERE id = ? AND ended_ms IS NULL",
            (_utc_ms(), session_id),
        )
        self._conn.commit()

    def clear_collected(self) -> None:
        self._conn.execute("DELETE FROM symbolic_actions;")
        self._conn.execute("DELETE FROM action_transitions;")
        self._conn.execute("DELETE FROM detected_patterns;")
        self._conn.execute("DELETE FROM sessions;")
        self._conn.execute("DELETE FROM automation_plans;")
        self._conn.commit()

    def get_automation_plan(self, pattern_hash: str) -> Optional[dict[str, Any]]:
        row = self._conn.execute(
            """
            SELECT pattern_hash, created_ms, updated_ms, plan_version, source_last_seen_ms, plan_json
            FROM automation_plans
            WHERE pattern_hash = ?
            """,
            (pattern_hash,),
        ).fetchone()
        if not row:
            return None
        plan_json = str(row["plan_json"] or "[]")
        try:
            plan = json.loads(plan_json)
        except Exception:
            plan = []
        return {
            "pattern_hash": row["pattern_hash"],
            "created_ms": row["created_ms"],
            "updated_ms": row["updated_ms"],
            "plan_version": row["plan_version"],
            "source_last_seen_ms": row["source_last_seen_ms"],
            "plan_steps": plan,
        }

    def upsert_automation_plan(
        self,
        pattern_hash: str,
        plan_steps: list[dict[str, Any]],
        *,
        source_last_seen_ms: Optional[int] = None,
        plan_version: int = 1,
    ) -> dict[str, Any]:
        now = _utc_ms()
        payload = json.dumps(plan_steps, ensure_ascii=False)
        existing = self._conn.execute(
            "SELECT created_ms FROM automation_plans WHERE pattern_hash = ?",
            (pattern_hash,),
        ).fetchone()
        created = int(existing["created_ms"]) if existing else now
        self._conn.execute(
            """
            INSERT INTO automation_plans(pattern_hash, created_ms, updated_ms, plan_version, source_last_seen_ms, plan_json)
            VALUES (?, ?, ?, ?, ?, ?)
            ON CONFLICT(pattern_hash)
            DO UPDATE SET
              updated_ms = excluded.updated_ms,
              plan_version = excluded.plan_version,
              source_last_seen_ms = excluded.source_last_seen_ms,
              plan_json = excluded.plan_json
            """,
            (pattern_hash, created, now, int(plan_version), source_last_seen_ms, payload),
        )
        self._conn.commit()
        return {
            "pattern_hash": pattern_hash,
            "created_ms": created,
            "updated_ms": now,
            "plan_version": int(plan_version),
            "source_last_seen_ms": source_last_seen_ms,
            "plan_steps": plan_steps,
        }

    def store_action(
        self,
        action_type: str,
        action_payload: dict[str, Any],
        timestamp_ms: Optional[int],
        session_id: str,
        duration_ms: Optional[int] = None,
        source_app: Optional[str] = None,
        target_app: Optional[str] = None,
    ) -> int:
        ts = timestamp_ms if timestamp_ms is not None else _utc_ms()
        action_data = json.dumps(action_payload, ensure_ascii=False)
        cur = self._conn.execute(
            """
            INSERT INTO symbolic_actions(action_type, action_data, timestamp_ms, session_id, duration_ms, source_app, target_app)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            """,
            (action_type, action_data, ts, session_id, duration_ms, source_app, target_app),
        )
        self._conn.execute(
            "UPDATE sessions SET action_count = action_count + 1 WHERE id = ?",
            (session_id,),
        )
        self._conn.commit()
        return int(cur.lastrowid)

    def get_recent_actions(self, limit: int) -> list[StoredAction]:
        rows = self._conn.execute(
            """
            SELECT id, action_type, action_data, timestamp_ms, session_id, duration_ms, source_app, target_app
            FROM symbolic_actions
            ORDER BY timestamp_ms DESC
            LIMIT ?
            """,
            (limit,),
        ).fetchall()
        return [StoredAction(**dict(r)) for r in rows]

    def get_recent_actions_chronological(self, limit: int) -> list[StoredAction]:
        rows = self._conn.execute(
            """
            SELECT id, action_type, action_data, timestamp_ms, session_id, duration_ms, source_app, target_app
            FROM symbolic_actions
            ORDER BY timestamp_ms DESC
            LIMIT ?
            """,
            (limit,),
        ).fetchall()
        items = [StoredAction(**dict(r)) for r in rows]
        items.sort(key=lambda a: a.timestamp_ms)
        return items

    def upsert_transition(
        self,
        from_action_type: str,
        to_action_type: str,
        from_app: Optional[str],
        to_app: Optional[str],
        duration_ms: int,
        last_seen_ms: int,
    ) -> None:
        self._conn.execute(
            """
            INSERT INTO action_transitions(from_action_type, to_action_type, from_app, to_app, frequency, total_duration_ms, last_seen_ms)
            VALUES (?, ?, ?, ?, 1, ?, ?)
            ON CONFLICT(from_action_type, to_action_type, from_app, to_app)
            DO UPDATE SET
              frequency = frequency + 1,
              total_duration_ms = total_duration_ms + excluded.total_duration_ms,
              last_seen_ms = MAX(last_seen_ms, excluded.last_seen_ms)
            """,
            (from_action_type, to_action_type, from_app, to_app, duration_ms, last_seen_ms),
        )
        self._conn.commit()

    def get_frequent_transitions(self, min_frequency: int) -> list[StoredTransition]:
        rows = self._conn.execute(
            """
            SELECT from_action_type, to_action_type, from_app, to_app, frequency, total_duration_ms, last_seen_ms
            FROM action_transitions
            WHERE frequency >= ?
            ORDER BY frequency DESC, last_seen_ms DESC
            """,
            (min_frequency,),
        ).fetchall()
        return [StoredTransition(**dict(r)) for r in rows]

