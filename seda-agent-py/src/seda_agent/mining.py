from __future__ import annotations

import hashlib
import json
from dataclasses import dataclass
from datetime import datetime, timezone
from typing import Any, Optional

from .db import StoredAction
from .automation_plan import simplify_automation_steps


def _sha256_hex(text: str) -> str:
    return hashlib.sha256(text.encode("utf-8", errors="ignore")).hexdigest()


def _now_iso(ms: int) -> str:
    return datetime.fromtimestamp(ms / 1000.0, tz=timezone.utc).isoformat()


def normalize_token(value: str, max_words: int, max_chars: int) -> str:
    lowered = (value or "").lower()
    cleaned = "".join(c if c.isalnum() else " " for c in lowered)
    parts = cleaned.split()
    token = "_".join(parts[: max(1, max_words)])
    token = token or "unknown"
    return token[: max(1, max_chars)]


def normalize_domain_hint(domain: str) -> str:
    trimmed = (domain or "").strip().lower()
    if not trimmed:
        return "unknown-domain"
    host = trimmed.removeprefix("www.")
    normalized = "".join(c for c in host if (c.isalnum() or c in ".-"))[:64]
    return normalized or "unknown-domain"


def extract_url_path_hint(url: str) -> str:
    trimmed = (url or "").strip()
    if not trimmed:
        return "root"
    without_scheme = trimmed.split("://", 1)[1] if "://" in trimmed else trimmed
    path = without_scheme.split("/", 1)[1] if "/" in without_scheme else ""
    path = path.split("?", 1)[0].split("#", 1)[0].strip("/")
    if not path:
        return "root"
    segs = [normalize_token(seg, 2, 24) for seg in path.split("/") if seg.strip()]
    segs = [s for s in segs if s]
    return "/".join(segs[:2]) or "root"


@dataclass(frozen=True)
class ActionSignature:
    key: str
    label: str


def _primary_app(action_type: str, source_app: Optional[str], target_app: Optional[str]) -> str:
    at = (action_type or "").upper()
    if at == "COPY_TEXT":
        return (source_app or target_app or "unknown").lower()
    return (target_app or source_app or "unknown").lower()


def build_action_signature(action: StoredAction) -> ActionSignature:
    primary_app = _primary_app(action.action_type, action.source_app, action.target_app)
    normalized_app = normalize_token(primary_app, 3, 48)

    try:
        payload = json.loads(action.action_data)
    except Exception:
        payload = {}

    at = (action.action_type or "").upper()
    if at == "VISIT_WEBSITE":
        domain = normalize_domain_hint(str(payload.get("website_domain") or payload.get("domain") or ""))
        url = str(payload.get("website_url") or payload.get("url") or "")
        path_hint = extract_url_path_hint(url)
        return ActionSignature(
            key=f"VISIT_WEBSITE::{domain}::{path_hint}",
            label=f"VISIT {domain} /{path_hint}",
        )
    if at == "SEARCH_WEB":
        domain = normalize_domain_hint(str(payload.get("website_domain") or payload.get("domain") or ""))
        query = normalize_token(str(payload.get("search_query") or payload.get("query") or ""), 5, 48)
        engine = normalize_token(str(payload.get("search_engine") or payload.get("engine") or "unknown"), 2, 24)
        return ActionSignature(
            key=f"SEARCH_WEB::{domain}::{engine}::{query}",
            label=f"SEARCH {domain} [{engine}] q:{query}",
        )

    return ActionSignature(
        key=f"{at}::{normalized_app}",
        label=f"{at.replace('_', ' ')} @ {normalized_app}",
    )


def contains_contiguous_subsequence(haystack: list[str], needle: list[str]) -> bool:
    if not needle or len(haystack) < len(needle):
        return False
    n = len(needle)
    return any(haystack[i : i + n] == needle for i in range(0, len(haystack) - n + 1))


@dataclass
class RepeatedSequenceStats:
    sequence_keys: list[str]
    sequence_labels: list[str]
    frequency: int
    total_duration_ms: int
    first_seen_ms: int
    last_seen_ms: int
    latest_start_idx: int
    latest_end_idx: int


def prune_actions_for_mining(
    actions: list[StoredAction],
    min_dwell_ms: int = 800,
) -> list[StoredAction]:
    """
    Remove obvious noise before mining:
    - Very short SWITCH_APP / FOCUS_DURATION hops (rapid Alt+Tab, twitches)
    - Events tied to unknown apps.
    """
    if not actions:
        return []

    cleaned: list[StoredAction] = []
    for a in actions:
        at = (a.action_type or "").upper()
        app = _primary_app(a.action_type, a.source_app, a.target_app)
        if app == "unknown":
            # Low-signal; keep only if it's a clipboard event.
            if at not in {"COPY_TEXT", "CLIPBOARD_CHANGED"}:
                continue

        if at in {"SWITCH_APP", "FOCUS_DURATION"}:
            dur = a.duration_ms or 0
            if dur < min_dwell_ms:
                # Fast flicker between apps – usually not meaningful as a "step".
                continue

        # Drop CLIPBOARD_STATUS baseline events from mining; they only mark initial state.
        if at == "CLIPBOARD_STATUS":
            continue

        cleaned.append(a)

    return cleaned


def build_repeated_task_bundles(
    actions: list[StoredAction],
    min_pattern_length: int,
    min_occurrences: int,
    limit: int,
    max_pattern_length: int,
) -> list[dict[str, Any]]:
    # First pass: prune obvious noise so mining focuses on meaningful steps.
    actions = prune_actions_for_mining(actions)

    abs_min_len = 2
    min_len = max(abs_min_len, int(min_pattern_length))
    if len(actions) < min_len:
        return []

    signatures = [build_action_signature(a) for a in actions]
    counts: dict[str, RepeatedSequenceStats] = {}
    max_len = min(max(int(max_pattern_length), min_len), len(actions))

    for span in range(min_len, max_len + 1):
        for start in range(0, len(actions) - span + 1):
            end = start + span - 1
            keys_slice = signatures[start : start + span]
            sequence_key = "||".join(sig.key for sig in keys_slice)
            duration_ms = max(0, actions[end].timestamp_ms - actions[start].timestamp_ms)

            if sequence_key not in counts:
                counts[sequence_key] = RepeatedSequenceStats(
                    sequence_keys=[sig.key for sig in keys_slice],
                    sequence_labels=[sig.label for sig in keys_slice],
                    frequency=0,
                    total_duration_ms=0,
                    first_seen_ms=actions[start].timestamp_ms,
                    last_seen_ms=actions[end].timestamp_ms,
                    latest_start_idx=start,
                    latest_end_idx=end,
                )

            entry = counts[sequence_key]
            entry.frequency += 1
            entry.total_duration_ms += duration_ms
            entry.first_seen_ms = min(entry.first_seen_ms, actions[start].timestamp_ms)
            if actions[end].timestamp_ms >= entry.last_seen_ms:
                entry.last_seen_ms = actions[end].timestamp_ms
                entry.latest_start_idx = start
                entry.latest_end_idx = end

    candidates = [v for v in counts.values() if v.frequency >= max(2, int(min_occurrences))]
    candidates.sort(
        key=lambda s: (s.frequency, len(s.sequence_keys), s.last_seen_ms),
        reverse=True,
    )

    selected: list[RepeatedSequenceStats] = []
    for cand in candidates:
        dominated = any(
            existing.frequency >= cand.frequency
            and len(existing.sequence_keys) >= len(cand.sequence_keys)
            and contains_contiguous_subsequence(existing.sequence_keys, cand.sequence_keys)
            for existing in selected
        )
        if not dominated:
            selected.append(cand)
        if len(selected) >= int(limit):
            break

    bundles: list[dict[str, Any]] = []
    for stats in selected:
        signature_joined = "||".join(stats.sequence_keys)
        pattern_hash = _sha256_hex(signature_joined)
        span = len(stats.sequence_keys)
        avg_duration_ms = None
        if stats.frequency > 0:
            avg_duration_ms = max(span, int(stats.total_duration_ms / stats.frequency))

        run = actions[stats.latest_start_idx : stats.latest_end_idx + 1]
        sample_run = [to_dashboard_action(a) for a in run]
        automation_steps = [to_automation_step(i, item) for i, item in enumerate(sample_run)]
        plan_steps = simplify_automation_steps(automation_steps)

        bundles.append(
            {
                "pattern_hash": pattern_hash,
                "sequence": stats.sequence_labels,
                "sequence_label": " -> ".join(stats.sequence_labels),
                "frequency": stats.frequency,
                "avg_duration_ms": avg_duration_ms,
                "confidence": 1.0,
                "first_seen_ms": stats.first_seen_ms,
                "last_seen_ms": stats.last_seen_ms,
                "last_seen_iso": _now_iso(stats.last_seen_ms),
                "sample_run": sample_run,
                "automation_steps": automation_steps,
                "plan_steps": plan_steps,
            }
        )

    return bundles


def to_dashboard_action(a: StoredAction) -> dict[str, Any]:
    try:
        payload = json.loads(a.action_data)
    except Exception:
        payload = {}
    ts_iso = _now_iso(a.timestamp_ms)
    return {
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
        "timestamp_iso": ts_iso,
    }


def to_automation_step(index: int, action: dict[str, Any]) -> dict[str, Any]:
    destructive = action.get("action_type") == "CLOSE_APP"
    selector = {
        "element_id": action.get("element_id"),
        "control_type": action.get("element_control_type"),
        "automation_id": action.get("element_automation_id"),
        "class_name": action.get("element_class_name"),
        "name_hash": action.get("element_name_hash"),
        "is_keyboard_focusable": action.get("element_is_keyboard_focusable"),
    }
    has_selector = any(v is not None for v in selector.values())
    return {
        "step_id": f"s{index + 1}",
        "action": action.get("action_type"),
        "node_id": action.get("node_id"),
        "timestamp_ms": action.get("timestamp_ms"),
        "source_app": action.get("source_app"),
        "target_app": action.get("target_app"),
        "selector_bundle": {"primary": selector if has_selector else None, "fallbacks": []},
        "ui_context": {
            "element_type": action.get("element_type"),
            "control_type": action.get("element_control_type"),
            "interaction": action.get("element_interaction"),
            "class_name": action.get("element_class_name"),
            "automation_id": action.get("element_automation_id"),
            "name_hash": action.get("element_name_hash"),
            "is_button": None,
            "is_input": None,
            "is_keyboard_focusable": action.get("element_is_keyboard_focusable"),
        },
        "precondition": {
            "app": action.get("target_app") or action.get("source_app"),
            "url_domain": action.get("website_domain"),
            "requires_element": True if has_selector else None,
        },
        "action_args": {
            "interaction": action.get("element_interaction"),
            "field_type": action.get("element_field_type"),
            "website_url": action.get("website_url"),
            "search_query": action.get("search_query"),
            "search_engine": action.get("search_engine"),
            "duration_ms": action.get("duration_ms"),
            "key_shortcut_hint": None,
        },
        "wait_rule": {"timeout_ms": 5000, "poll_interval_ms": 200, "retry": 2},
        "postcondition": {
            "expected_app": action.get("target_app") or action.get("source_app"),
            "expected_domain": action.get("website_domain"),
            "node_reached": action.get("node_id"),
        },
        "on_failure": {"strategy": "retry_then_abort", "retry_max": 2},
        "variables": [],
        "safety": {"destructive": destructive, "requires_confirmation": destructive},
    }

