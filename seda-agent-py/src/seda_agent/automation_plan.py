from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Optional


@dataclass(frozen=True)
class PlanWarning:
    code: str
    message: str


def _s(v: Any) -> str:
    return "" if v is None else str(v)


def _primary_app(step: dict[str, Any]) -> str:
    return _s(step.get("target_app") or step.get("source_app") or step.get("precondition", {}).get("app") or "unknown")


def _domain(step: dict[str, Any]) -> str:
    return _s(step.get("action_args", {}).get("website_url") or step.get("website_domain") or step.get("precondition", {}).get("url_domain") or "")


def _selector_key(step: dict[str, Any]) -> str:
    sel = (step.get("selector_bundle") or {}).get("primary") or {}
    # stable-ish identity for dedup; tolerate missing fields
    return "|".join(
        [
            _s(sel.get("automation_id")),
            _s(sel.get("element_id")),
            _s(sel.get("class_name")),
            _s(sel.get("control_type")),
            _s(sel.get("name_hash")),
        ]
    ).strip("|")


def _mk_plan_step(
    *,
    sid: str,
    kind: str,
    title: str,
    app: Optional[str],
    domain: Optional[str],
    source_step_ids: list[str],
    payload: dict[str, Any],
    warnings: list[PlanWarning],
    safety: dict[str, Any],
) -> dict[str, Any]:
    return {
        "id": sid,
        "kind": kind,
        "title": title,
        "app": app,
        "domain": domain,
        "payload": payload,
        "source_step_ids": source_step_ids,
        "warnings": [w.__dict__ for w in warnings],
        "safety": safety,
    }


def simplify_automation_steps(steps: list[dict[str, Any]]) -> list[dict[str, Any]]:
    """
    Convert raw-ish `automation_steps` (from mining) into a cleaner, user-editable
    plan that describes "what the automation will do".

    Heuristics are intentionally simple + deterministic:
    - Collapse consecutive focus/app hops
    - Deduplicate consecutive identical clicks (same selector + interaction)
    - Deduplicate consecutive identical navigations/searches
    - Drop low-signal dwell steps unless they appear alone
    """
    if not steps:
        return []

    plan: list[dict[str, Any]] = []

    last_kind = None
    last_app = None
    last_domain = None
    last_selector = None
    last_interaction = None
    last_action = None

    seq = 0
    for st in steps:
        action = _s(st.get("action") or st.get("action_type")).upper()
        app = _primary_app(st)
        selector = _selector_key(st)
        interaction = _s((st.get("action_args") or {}).get("interaction") or (st.get("ui_context") or {}).get("interaction") or "")
        domain = _s((st.get("action_args") or {}).get("website_url") or (st.get("action_args") or {}).get("website_domain") or st.get("precondition", {}).get("url_domain") or "")
        query = _s((st.get("action_args") or {}).get("search_query") or "")

        destructive = bool(((st.get("safety") or {}).get("destructive")) or action == "CLOSE_APP")
        safety = {"destructive": destructive, "requires_confirmation": bool(((st.get("safety") or {}).get("requires_confirmation")) or destructive)}

        # Map raw action -> plan "kind"
        if action in {"OPEN_APP", "SWITCH_APP"}:
            kind = "focus_app"
            title = f"Focus {app}" if app else "Focus app"
            payload = {"app": app}
        elif action == "VISIT_WEBSITE":
            kind = "navigate_url"
            url = _s((st.get("action_args") or {}).get("website_url") or "")
            title = f"Go to {url}" if url else (f"Go to {domain}" if domain else "Go to website")
            payload = {"url": url, "domain": _s((st.get("precondition") or {}).get("url_domain") or st.get("website_domain") or "")}
        elif action == "SEARCH_WEB":
            kind = "search_web"
            title = f"Search: {query}" if query else "Search web"
            payload = {
                "query": query,
                "engine": _s((st.get("action_args") or {}).get("search_engine") or ""),
                "domain": _s((st.get("precondition") or {}).get("url_domain") or st.get("website_domain") or ""),
            }
        elif action == "TYPE_TEXT":
            kind = "type_text"
            field_type = _s((st.get("action_args") or {}).get("field_type") or "")
            title = f"Type into {field_type}" if field_type else "Type text"
            payload = {"field_type": field_type, "selector_bundle": st.get("selector_bundle")}
        elif action == "INTERACT":
            kind = "click"
            title = "Click element" if not interaction else f"{interaction.title()} element"
            payload = {"interaction": interaction, "selector_bundle": st.get("selector_bundle")}
        elif action == "FOCUS_DURATION":
            kind = "wait"
            dur = (st.get("action_args") or {}).get("duration_ms") or st.get("duration_ms")
            title = f"Wait ({int(dur)}ms)" if isinstance(dur, (int, float)) else "Wait"
            payload = {"duration_ms": dur, "app": app}
        else:
            kind = "unknown"
            title = action or "Unknown step"
            payload = {"raw_action": st}

        # Drop low-signal waits if they are just between two meaningful steps
        if kind == "wait":
            if last_kind in {"focus_app", "click", "type_text", "navigate_url", "search_web"}:
                # likely noise dwell; skip
                last_action = action
                continue

        # Dedup: collapse consecutive same kind/app/domain
        if kind == last_kind:
            if kind == "focus_app" and app and app == last_app:
                last_action = action
                continue
            if kind in {"navigate_url", "search_web"} and domain and domain == last_domain and (kind != "search_web" or query == _s((plan[-1].get("payload") or {}).get("query") or "")):
                last_action = action
                continue
            if kind in {"click", "type_text"} and selector and selector == last_selector and interaction == (last_interaction or ""):
                last_action = action
                continue

        warnings: list[PlanWarning] = []
        requires_el = (st.get("precondition") or {}).get("requires_element")
        if requires_el and not selector:
            warnings.append(PlanWarning(code="missing_selector", message="No UI selector captured for this step. Automation may be unreliable."))
        if kind == "unknown":
            warnings.append(PlanWarning(code="unknown_action", message="This step type is not yet supported; you can edit or delete it."))
        if safety.get("requires_confirmation"):
            warnings.append(PlanWarning(code="destructive", message="Destructive step (requires confirmation)."))

        seq += 1
        sid = f"p{seq}"
        plan.append(
            _mk_plan_step(
                sid=sid,
                kind=kind,
                title=title,
                app=app or None,
                domain=domain or None,
                source_step_ids=[_s(st.get("step_id") or "")] if _s(st.get("step_id") or "") else [],
                payload=payload,
                warnings=warnings,
                safety=safety,
            )
        )

        last_kind = kind
        last_app = app
        last_domain = domain
        last_selector = selector
        last_interaction = interaction
        last_action = action

    # If we dropped everything due to waits/noise, keep at least one original as "unknown"
    if not plan:
        return [
            _mk_plan_step(
                sid="p1",
                kind="unknown",
                title="Noisy sequence (edit to fix)",
                app=None,
                domain=None,
                source_step_ids=[],
                payload={"raw_steps": steps[:50]},
                warnings=[PlanWarning(code="empty_after_simplify", message="All steps were simplified away as noise; adjust manually.")],
                safety={"destructive": False, "requires_confirmation": False},
            )
        ]

    return plan

