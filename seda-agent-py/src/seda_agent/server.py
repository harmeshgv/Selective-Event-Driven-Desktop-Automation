from __future__ import annotations

import json
import os
import threading
import time
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Optional

from dotenv import load_dotenv
from fastapi import Body, FastAPI, Query
from fastapi.middleware.cors import CORSMiddleware
from fastapi.responses import HTMLResponse, PlainTextResponse

from .config import Config
from .collector_windows import WindowsCollector
from .db import Repository, connect, migrate
from .mining import build_repeated_task_bundles
from .llm_explain import explain_bundle
from .rpc import SafetyEnforcer, jsonrpc_error, jsonrpc_success, tools_list


def _repo_from_config(cfg: Config) -> Repository:
    conn = connect(cfg.database_path)
    migrate(conn)
    return Repository(conn)


def _read_dashboard_html() -> str:
    # Prefer the existing Rust dashboard asset so UI stays identical.
    # PyInstaller: may bundle as dist/SEDA.exe with data files.
    try:
        import sys

        base = Path(getattr(sys, "_MEIPASS"))  # type: ignore[attr-defined]
        bundled = base / "seda-agent" / "src" / "mcp" / "dashboard.html"
        if bundled.exists():
            return bundled.read_text(encoding="utf-8", errors="ignore")
    except Exception:
        pass

    root = Path(__file__).resolve().parents[3]  # .../seda-agent-py/src/seda_agent/ -> repo root
    candidate = root / "seda-agent" / "src" / "mcp" / "dashboard.html"
    if candidate.exists():
        return candidate.read_text(encoding="utf-8", errors="ignore")
    return "<html><body><h1>SEDA Dashboard missing</h1></body></html>"


@dataclass
class CollectorSnapshot:
    collecting: bool
    current_session_id: Optional[str]
    started_ms: Optional[int]
    message: str


class CollectorController:
    def __init__(self, repo: Repository):
        self._repo = repo
        self._collecting = False
        self._session_id: Optional[str] = None
        self._started_ms: Optional[int] = None
        self._collector = WindowsCollector(repo)
        self._lock = threading.RLock()

    def snapshot(self) -> CollectorSnapshot:
        with self._lock:
            return CollectorSnapshot(
                collecting=self._collecting,
                current_session_id=self._session_id,
                started_ms=self._started_ms,
                message="collecting" if self._collecting else "idle",
            )

    def start(self) -> CollectorSnapshot:
        with self._lock:
            if self._collecting:
                return self.snapshot()
            self._session_id = self._repo.open_session()
            self._collecting = True
            self._started_ms = int(time.time() * 1000)
            if self._session_id:
                self._collector.start(self._session_id)
            return self.snapshot()

    def stop(self) -> CollectorSnapshot:
        with self._lock:
            if not self._collecting:
                return self.snapshot()
            self._collector.stop()
            if self._session_id:
                self._repo.close_session(self._session_id)
            self._collecting = False
            self._session_id = None
            self._started_ms = None
            return self.snapshot()

    def clear(self) -> CollectorSnapshot:
        with self._lock:
            self._collector.stop()
            self._repo.clear_collected()
            self._collecting = False
            self._session_id = None
            self._started_ms = None
            snap = self.snapshot()
            snap.message = "cleared"
            return snap


def create_app(cfg: Config) -> FastAPI:
    load_dotenv(dotenv_path=Path.cwd() / ".env", override=False)
    load_dotenv(dotenv_path=Path.cwd().parent / ".env", override=False)

    repo = _repo_from_config(cfg)
    controller = CollectorController(repo)
    safety = SafetyEnforcer()

    app = FastAPI(title="SEDA Agent (Python)")

    # Allow local web UI development (Vite) to call the API.
    # Keep this scoped to localhost origins for safety.
    app.add_middleware(
        CORSMiddleware,
        allow_origins=[
            "http://127.0.0.1:5173",
            "http://localhost:5173",
            "http://127.0.0.1:4173",
            "http://localhost:4173",
        ],
        allow_credentials=True,
        allow_methods=["*"],
        allow_headers=["*"],
    )

    @app.get("/", response_class=PlainTextResponse)
    @app.get("/health", response_class=PlainTextResponse)
    def health() -> str:
        return "SEDA Agent MCP Server OK"

    @app.get("/dashboard", response_class=HTMLResponse)
    def dashboard() -> str:
        return _read_dashboard_html()

    @app.get("/api/dashboard/status")
    def dashboard_status() -> dict[str, Any]:
        return {"success": True, "message": "Collector status retrieved", "data": controller.snapshot().__dict__}

    @app.post("/api/dashboard/start")
    def dashboard_start() -> dict[str, Any]:
        return {"success": True, "message": "Data collection is running", "data": controller.start().__dict__}

    @app.post("/api/dashboard/stop")
    def dashboard_stop() -> dict[str, Any]:
        return {"success": True, "message": "Data collection stopped", "data": controller.stop().__dict__}

    @app.post("/api/dashboard/clear")
    def dashboard_clear() -> dict[str, Any]:
        return {"success": True, "message": "Collected data cleared", "data": controller.clear().__dict__}

    @app.get("/api/dashboard/actions")
    def dashboard_actions(limit: int = Query(default=100, ge=10, le=500)) -> dict[str, Any]:
        actions = repo.get_recent_actions(limit)
        data = []
        for a in actions:
            payload = json.loads(a.action_data)
            timestamp_iso = datetime.fromtimestamp(a.timestamp_ms / 1000.0, tz=timezone.utc).isoformat()
            data.append(
                {
                    "id": a.id,
                    "action_type": a.action_type,
                    "node_id": f"{a.action_type}::{(a.target_app or a.source_app or 'unknown')}",
                    "source_app": a.source_app,
                    "target_app": a.target_app,
                    "element_type": payload.get("element_type"),
                    "element_id": payload.get("element_id"),
                    "element_control_type": payload.get("element_control_type"),
                    "element_automation_id": payload.get("element_automation_id"),
                    "element_class_name": payload.get("element_class_name"),
                    "element_name_hash": payload.get("element_name_hash"),
                    "element_is_keyboard_focusable": payload.get("element_is_keyboard_focusable"),
                    "element_interaction": payload.get("element_interaction"),
                    "element_field_type": payload.get("element_field_type"),
                    "website_url": payload.get("website_url"),
                    "website_domain": payload.get("website_domain"),
                    "search_query": payload.get("search_query"),
                    "search_engine": payload.get("search_engine"),
                    "duration_ms": a.duration_ms,
                    "session_id": a.session_id,
                    "timestamp_ms": a.timestamp_ms,
                    "timestamp_iso": timestamp_iso,
                }
            )
        return {"success": True, "message": "Recent actions loaded", "data": data}

    @app.get("/api/dashboard/flow")
    def dashboard_flow(limit: int = Query(default=1200, ge=50, le=5000)) -> dict[str, Any]:
        actions = repo.get_recent_actions_chronological(limit)
        data = []
        for a in actions:
            payload = json.loads(a.action_data)
            timestamp_iso = datetime.fromtimestamp(a.timestamp_ms / 1000.0, tz=timezone.utc).isoformat()
            data.append(
                {
                    "id": a.id,
                    "action_type": a.action_type,
                    "node_id": f"{a.action_type}::{(a.target_app or a.source_app or 'unknown')}",
                    "source_app": a.source_app,
                    "target_app": a.target_app,
                    "element_type": payload.get("element_type"),
                    "element_id": payload.get("element_id"),
                    "element_control_type": payload.get("element_control_type"),
                    "element_automation_id": payload.get("element_automation_id"),
                    "element_class_name": payload.get("element_class_name"),
                    "element_name_hash": payload.get("element_name_hash"),
                    "element_is_keyboard_focusable": payload.get("element_is_keyboard_focusable"),
                    "element_interaction": payload.get("element_interaction"),
                    "element_field_type": payload.get("element_field_type"),
                    "website_url": payload.get("website_url"),
                    "website_domain": payload.get("website_domain"),
                    "search_query": payload.get("search_query"),
                    "search_engine": payload.get("search_engine"),
                    "duration_ms": a.duration_ms,
                    "session_id": a.session_id,
                    "timestamp_ms": a.timestamp_ms,
                    "timestamp_iso": timestamp_iso,
                }
            )
        return {
            "success": True,
            "message": "Flow actions loaded (latest window, oldest to newest)",
            "data": data,
        }

    @app.get("/api/dashboard/repeated_tasks")
    def dashboard_repeated_tasks(
        min_repeats: int = Query(default=2, ge=2, le=64, alias="min_frequency"),
        limit: int = Query(default=25, ge=1, le=100),
        flow_limit: int = Query(default=5000, ge=200, le=20000),
    ) -> dict[str, Any]:
        flow_actions = repo.get_recent_actions_chronological(flow_limit)
        # Keep logic aligned with Rust: min_frequency maps to min_repeats in the dashboard.
        max_pattern_length = max(10, min_repeats)
        bundles = build_repeated_task_bundles(
            actions=flow_actions,
            min_pattern_length=min_repeats,
            min_occurrences=2,
            limit=limit,
            max_pattern_length=min(64, max_pattern_length),
        )
        return {"success": True, "message": "Repeated tasks loaded", "data": bundles}

    @app.get("/api/automation/provider")
    def automation_provider() -> dict[str, Any]:
        return {
            "success": True,
            "message": "Automation provider status loaded",
            "data": {
                "provider": "disabled",
                "model": "n/a",
                "timeout_seconds": 30,
                "enabled": False,
                "disabled_reason": "LLM integration not implemented in Python build yet",
                "min_steps_threshold": 15,
            },
        }

    @app.get("/api/automation/candidates")
    def automation_candidates(
        min_steps: int = Query(default=15, ge=2, le=256),
        min_frequency: int = Query(default=3, ge=2, le=64),
        limit: int = Query(default=25, ge=1, le=100),
        flow_limit: int = Query(default=5000, ge=200, le=20000),
    ) -> dict[str, Any]:
        flow_actions = repo.get_recent_actions_chronological(flow_limit)
        bundles = build_repeated_task_bundles(
            actions=flow_actions,
            min_pattern_length=min_frequency,
            min_occurrences=2,
            limit=limit,
            max_pattern_length=min(64, max(10, min_frequency)),
        )
        candidates = [
            {
                "pattern_hash": b["pattern_hash"],
                "sequence_label": b["sequence_label"],
                "frequency": b["frequency"],
                "avg_duration_ms": b["avg_duration_ms"],
                "last_seen_ms": b["last_seen_ms"],
                "step_count": len(b.get("automation_steps") or []),
            }
            for b in bundles
            if len(b.get("automation_steps") or []) >= min_steps
        ]
        return {"success": True, "message": "Automation candidates loaded", "data": candidates}

    @app.get("/api/dashboard/graph")
    def dashboard_graph(min_frequency: int = Query(default=1, ge=1, le=9999)) -> dict[str, Any]:
        transitions = repo.get_frequent_transitions(min_frequency)
        node_ids: set[str] = set()
        edges = []
        for t in transitions:
            from_id = f"{t.from_action_type}::{(t.from_app or 'unknown')}"
            to_id = f"{t.to_action_type}::{(t.to_app or 'unknown')}"
            node_ids.add(from_id)
            node_ids.add(to_id)
            edges.append(
                {
                    "from": from_id,
                    "to": to_id,
                    "frequency": t.frequency,
                    "avg_duration_ms": t.avg_duration_ms(),
                    "last_seen_ms": t.last_seen_ms,
                }
            )
        nodes = [{"id": n} for n in sorted(node_ids)]
        return {"success": True, "message": "Graph data loaded", "data": {"nodes": nodes, "edges": edges}}

    @app.post("/api/ui/explain_bundle")
    def ui_explain_bundle(bundle: dict[str, Any] = Body(...)) -> dict[str, Any]:
        """
        Optional feature: send a repeated-task bundle to an LLM and return a human explanation.
        Controlled by env vars:
          - SEDA_LLM_PROVIDER = disabled|ollama|groq
          - SEDA_LLM_MODEL, SEDA_LLM_BASE_URL, SEDA_LLM_TIMEOUT_SECONDS
          - SEDA_GROQ_API_KEY (or GROQ_API_KEY)
        """
        try:
            result = explain_bundle(bundle)
            ok = bool(result.get("enabled")) and bool(result.get("explanation"))
            return {
                "success": ok,
                "message": "Explanation generated" if ok else "Explanation unavailable",
                "data": result,
            }
        except Exception as e:
            return {
                "success": False,
                "message": "Explanation failed",
                "data": {
                    "enabled": False,
                    "provider": "unknown",
                    "model": "unknown",
                    "explanation": None,
                    "error": f"{type(e).__name__}: {e}",
                },
            }

    @app.post("/api/plans/resolve")
    def resolve_plan(bundle: dict[str, Any] = Body(...)) -> dict[str, Any]:
        """
        Return an editable automation plan for a bundle.

        If a saved plan exists for the bundle's pattern_hash, return it.
        Otherwise, create a new plan from bundle.plan_steps and persist it.
        """
        pattern_hash = str(bundle.get("pattern_hash") or "").strip()
        if not pattern_hash:
            return {"success": False, "message": "Missing pattern_hash", "data": None}

        saved = repo.get_automation_plan(pattern_hash)
        if saved and isinstance(saved.get("plan_steps"), list) and saved["plan_steps"]:
            return {"success": True, "message": "Plan loaded", "data": saved}

        source_last_seen_ms = bundle.get("last_seen_ms")
        plan_steps = bundle.get("plan_steps") or []
        if not isinstance(plan_steps, list):
            plan_steps = []

        created = repo.upsert_automation_plan(
            pattern_hash,
            plan_steps,
            source_last_seen_ms=int(source_last_seen_ms) if isinstance(source_last_seen_ms, (int, float)) else None,
            plan_version=1,
        )
        return {"success": True, "message": "Plan created", "data": created}

    @app.put("/api/plans/{pattern_hash}")
    def save_plan(pattern_hash: str, body: dict[str, Any] = Body(...)) -> dict[str, Any]:
        plan_steps = body.get("plan_steps") or []
        if not isinstance(plan_steps, list):
            return {"success": False, "message": "plan_steps must be a list", "data": None}
        source_last_seen_ms = body.get("source_last_seen_ms")
        plan_version = body.get("plan_version") or 1
        saved = repo.upsert_automation_plan(
            pattern_hash.strip(),
            plan_steps,
            source_last_seen_ms=int(source_last_seen_ms) if isinstance(source_last_seen_ms, (int, float)) else None,
            plan_version=int(plan_version) if isinstance(plan_version, (int, float)) else 1,
        )
        return {"success": True, "message": "Plan saved", "data": saved}

    @app.post("/rpc")
    @app.post("/mcp")
    def rpc(request: dict[str, Any] = Body(...)) -> dict[str, Any]:
        if request.get("jsonrpc") != "2.0":
            return jsonrpc_error(request.get("id"), -32600, "Invalid JSON-RPC version")

        method = request.get("method", "")
        params = request.get("params")
        rid = request.get("id")

        err = safety.check(method)
        if err:
            return jsonrpc_error(rid, err["code"], err["message"])
        perr = safety.validate_params(method, params)
        if perr:
            return jsonrpc_error(rid, perr["code"], perr["message"])

        # Minimal Python port: return safe stubs for Windows automation tools.
        if method == "tools/list":
            return jsonrpc_success(rid, tools_list())
        if method == "list_windows":
            return jsonrpc_success(rid, [])
        if method == "get_window_tree":
            return jsonrpc_success(
                rid,
                {
                    "element_id": "root",
                    "control_type": "Window",
                    "name_hash": None,
                    "is_enabled": True,
                    "is_keyboard_focusable": False,
                    "children": [],
                    "supported_patterns": [],
                    "bounds": None,
                },
            )
        if method in ("get_patterns", "get_transitions"):
            return jsonrpc_success(rid, [])
        if method in ("activate_element", "press_key", "set_clipboard"):
            return jsonrpc_error(
                rid,
                -32001,
                f"{method} is not implemented in the Python port yet (Windows automation backend pending)",
            )

        return jsonrpc_error(rid, -32601, f"Method not found: {method}")

    return app

