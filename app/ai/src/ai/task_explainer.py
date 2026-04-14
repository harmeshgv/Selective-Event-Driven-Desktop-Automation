from __future__ import annotations

import hashlib
import json
import logging
import os
import re
from dataclasses import dataclass
from difflib import SequenceMatcher
from pathlib import Path
from threading import Lock
from typing import Protocol
from urllib import error, request


PROMPT_TEMPLATE = """You are a smart desktop assistant. A user clicked "Explain" on a detected repeated task. Your job is to explain what they were doing in plain English so anyone can understand it.

Here is the detected task data:
{actions_json}

This pattern was seen {repeat_count} time(s).

Write a SHORT explanation (3-4 sentences max, one paragraph). Rules:
- Talk directly to the user: "You were...", "You kept...", "You repeatedly..."
- Say WHAT they were doing, on WHICH app/website, and WHY it looks automatable.
- Be specific: mention the actual app names, search terms, or pages from the data.
- Do NOT use bullet points, numbered lists, or section headers.
- Do NOT be vague or generic. Use the real data.
- Sound like a helpful assistant, not a robot.

After the paragraph, on a new line write exactly:
---METADATA---
IS_REPEATED: <true|false>
REPEATED_CONFIDENCE: <0.00-1.00>"""

# Model output delimiter (must match prompt). Narrative is shown in UI; lines after this are parsed only.
METADATA_MARKERS = ("\n---METADATA---\n", "\n---METADATA---", "---METADATA---")


@dataclass(frozen=True)
class TaskExplanationResult:
    explanation: str
    provider: str
    cached: bool
    used_fallback: bool
    cache_key: str
    is_repeated: bool
    repeated_confidence: float
    repeated_reason: str


@dataclass(frozen=True)
class TaskExplanationInput:
    task_id: int | None
    task_name: str
    signature: str
    repeat_count: int
    last_used: str
    confidence_score: float | None
    actions: list[str]


class TaskExplainer(Protocol):
    provider_name: str

    def explain_repeated_task(self, *, prompt_input: dict[str, object], repeat_count: int) -> str: ...


@dataclass(frozen=True)
class CachedExplanation:
    actions: tuple[str, ...]
    explanation: str
    provider: str
    used_fallback: bool
    is_repeated: bool
    repeated_confidence: float
    repeated_reason: str


_EXPLANATION_CACHE: dict[str, CachedExplanation] = {}
_EXPLANATION_CACHE_LOCK = Lock()
_LOGGER = logging.getLogger("task_explainer")
_DOTENV_CACHE: dict[str, str] | None = None


def _normalize_text(value: str, *, max_len: int = 80) -> str:
    cleaned = re.sub(r"[^a-zA-Z0-9]+", " ", value or "").strip().lower()
    cleaned = re.sub(r"\s+", " ", cleaned)
    return cleaned[:max_len].strip()


def _normalize_action(action: str) -> str:
    action = (action or "").strip()
    if not action:
        return ""

    kind, _, payload = action.partition(":")
    kind = kind.upper().strip()
    payload = payload.strip()

    if kind == "VIEW":
        return f"VIEW:{_normalize_text(payload, max_len=64) or 'unknown'}"
    if kind in {"SUBMIT_TEXT", "TYPE_TEXT"}:
        return f"{kind}:{_normalize_text(payload, max_len=64) or 'unknown'}"
    if kind == "CLICK":
        target = payload.split("@", 1)[0]
        return f"CLICK:{_normalize_text(target, max_len=48) or 'target'}"
    if kind == "ACTION":
        target = payload.split("@", 1)[0]
        return f"ACTION:{_normalize_text(target, max_len=48) or 'action'}"
    if kind == "KEY":
        return f"KEY:{_normalize_text(payload, max_len=24) or 'key'}"
    return f"{kind}:{_normalize_text(payload, max_len=64) or 'unknown'}"


def _canonical_actions(actions: list[str]) -> tuple[str, ...]:
    canonical = tuple(token for token in (_normalize_action(action) for action in actions) if token)
    return canonical or ("UNKNOWN:workflow",)


def _cache_key(actions: tuple[str, ...]) -> str:
    # Bump version when prompt shape or intent expectations change (invalidates stale cached blurbs).
    payload = json.dumps({"explain_task_prompt": "intent-v3", "actions": list(actions)}, ensure_ascii=True, separators=(",", ":"))
    return hashlib.sha1(payload.encode("utf-8")).hexdigest()


def _find_similar_cached_entry(actions: tuple[str, ...], exact_key: str) -> CachedExplanation | None:
    joined = "|".join(actions)
    with _EXPLANATION_CACHE_LOCK:
        cached = _EXPLANATION_CACHE.get(exact_key)
        if cached is not None:
            return cached

        for existing in _EXPLANATION_CACHE.values():
            if abs(len(existing.actions) - len(actions)) > 2:
                continue
            similarity = SequenceMatcher(a="|".join(existing.actions), b=joined).ratio()
            if similarity >= 0.94:
                return existing
    return None


def _store_cached_result(
    *,
    actions: tuple[str, ...],
    key: str,
    explanation: str,
    provider: str,
    used_fallback: bool,
    is_repeated: bool,
    repeated_confidence: float,
    repeated_reason: str,
) -> None:
    with _EXPLANATION_CACHE_LOCK:
        _EXPLANATION_CACHE[key] = CachedExplanation(
            actions=actions,
            explanation=explanation,
            provider=provider,
            used_fallback=used_fallback,
            is_repeated=is_repeated,
            repeated_confidence=repeated_confidence,
            repeated_reason=repeated_reason,
        )


def _build_prompt(prompt_input: dict[str, object], repeat_count: int) -> str:
    actions_json = json.dumps(prompt_input, ensure_ascii=True, indent=2)
    return PROMPT_TEMPLATE.format(actions_json=actions_json, repeat_count=repeat_count)


def _log_llm_debug_block(title: str, content: str) -> None:
    # Intentionally logs the full prompt / raw reply for local debugging.
    _LOGGER.warning("========== %s ==========\n%s\n========== END %s ==========", title, content, title)


def _load_dotenv_fallback() -> dict[str, str]:
    global _DOTENV_CACHE
    if _DOTENV_CACHE is not None:
        return _DOTENV_CACHE

    env: dict[str, str] = {}
    # Typical local layouts:
    # - repo_root/.env
    # - repo_root/app/backend/.env
    repo_root = Path(__file__).resolve().parents[4]
    candidates = [repo_root / ".env", repo_root / "app" / "backend" / ".env"]
    for path in candidates:
        if not path.exists():
            continue
        try:
            for raw in path.read_text(encoding="utf-8").splitlines():
                line = raw.strip()
                if not line or line.startswith("#") or "=" not in line:
                    continue
                key, value = line.split("=", 1)
                k = key.strip()
                if not k:
                    continue
                env[k] = value.strip().strip("\"'")
        except OSError:
            continue
    _DOTENV_CACHE = env
    return env


def _get_env(name: str, default: str = "") -> str:
    value = os.getenv(name, "").strip()
    if value:
        return value
    return _load_dotenv_fallback().get(name, default).strip()


def _compact_dict(data: dict[str, object]) -> dict[str, object]:
    compact: dict[str, object] = {}
    for key, value in data.items():
        if value is None:
            continue
        if isinstance(value, str) and not value.strip():
            continue
        if isinstance(value, list) and not value:
            continue
        if isinstance(value, dict) and not value:
            continue
        compact[key] = value
    return compact


def _filter_prompt_actions(actions: list[str]) -> list[str]:
    filtered: list[str] = []
    for action in actions:
        token = (action or "").strip()
        if not token:
            continue
        lowered = token.lower()
        if lowered in {"move", "mouse_move", "mousemove", "screenshot"}:
            continue
        filtered.append(token)
    return filtered


def _parse_action_details(action: str) -> dict[str, str]:
    kind, _, payload = action.partition(":")
    action_type = kind.strip().lower() or "unknown"
    payload = payload.strip()
    details: dict[str, str] = {
        "raw": action,
        "type": action_type,
    }

    if action_type == "view":
        details["context"] = payload
        return details

    if action_type in {"submit_text", "type_text"}:
        details["text"] = payload
        return details

    if action_type in {"click", "action"}:
        target, _, context = payload.partition("@")
        if target:
            details["target"] = target
        if context:
            details["context"] = context
        return details

    if action_type == "key":
        details["key"] = payload
        return details

    if payload:
        details["value"] = payload
    return details


def _unique_non_empty(values: list[str]) -> list[str]:
    seen: set[str] = set()
    result: list[str] = []
    for value in values:
        cleaned = value.strip()
        if not cleaned or cleaned in seen:
            continue
        seen.add(cleaned)
        result.append(cleaned)
    return result


def _top_counts(values: list[str], *, limit: int = 5) -> list[dict[str, object]]:
    counts: dict[str, int] = {}
    for value in values:
        cleaned = (value or "").strip()
        if not cleaned:
            continue
        counts[cleaned] = counts.get(cleaned, 0) + 1
    ranked = sorted(counts.items(), key=lambda x: (-x[1], x[0]))
    return [{"value": value, "count": count} for value, count in ranked[:limit]]


def _build_prompt_input(task: TaskExplanationInput) -> tuple[list[str], dict[str, object]]:
    raw_actions_full = [str(a) for a in task.actions if str(a).strip()]
    filtered_actions = _filter_prompt_actions(task.actions)
    action_details = [_parse_action_details(action) for action in filtered_actions]

    # Keep explanation grounded in the dominant app/context to reduce mixed-flow noise
    # (for example LinkedIn actions mixed with unrelated tabs like Monkeytype).
    context_counts: dict[str, int] = {}
    for detail in action_details:
        ctx = (detail.get("context", "") or "").strip()
        if not ctx:
            continue
        context_counts[ctx] = context_counts.get(ctx, 0) + 1
    dominant_context = ""
    if context_counts:
        dominant_context = max(context_counts.items(), key=lambda kv: kv[1])[0]

    def _keep_detail(detail: dict[str, str]) -> bool:
        action_type = detail.get("type", "")
        ctx = (detail.get("context", "") or "").strip()
        raw = detail.get("raw", "")
        target = (detail.get("target", "") or "").strip().lower()

        if action_type == "view":
            return not dominant_context or ctx == dominant_context
        if action_type in {"click", "action"}:
            # Drop generic low-signal clicks unless they are in dominant context.
            if target in {"left", "right", "button", "target"} and dominant_context and ctx != dominant_context:
                return False
            return not dominant_context or ctx == dominant_context
        if action_type in {"submit_text", "type_text", "key"}:
            # Keep text/key events (they may not carry explicit context).
            return True
        return True

    scoped_details = [d for d in action_details if _keep_detail(d)]
    if scoped_details:
        action_details = scoped_details
        filtered_actions = [d.get("raw", "") for d in action_details if d.get("raw", "").strip()]

    views = _unique_non_empty(
        [detail.get("context", "") for detail in action_details if detail.get("type") == "view"]
    )
    text_inputs = _unique_non_empty(
        [detail.get("text", "") for detail in action_details if detail.get("type") in {"submit_text", "type_text"}]
    )
    click_targets = _unique_non_empty(
        [detail.get("target", "") for detail in action_details if detail.get("type") == "click"]
    )
    explicit_actions = _unique_non_empty(
        [detail.get("target", "") for detail in action_details if detail.get("type") == "action"]
    )
    key_inputs = _unique_non_empty(
        [detail.get("key", "") for detail in action_details if detail.get("type") == "key"]
    )
    platforms = _unique_non_empty(
        [
            detail.get("context", "")
            for detail in action_details
            if detail.get("context")
        ]
    )
    step_types: dict[str, int] = {}
    for detail in action_details:
        step_type = detail.get("type", "unknown")
        step_types[step_type] = step_types.get(step_type, 0) + 1

    # Higher-signal evidence blocks to help the LLM produce factual explanations.
    contexts_all = _unique_non_empty([detail.get("context", "") for detail in action_details if detail.get("context")])
    ordered_preview = filtered_actions[:12]
    typed_terms = [detail.get("text", "") for detail in action_details if detail.get("type") in {"submit_text", "type_text"}]
    click_terms = [detail.get("target", "") for detail in action_details if detail.get("type") == "click"]
    key_terms = [detail.get("key", "") for detail in action_details if detail.get("type") == "key"]
    evidence_summary = _compact_dict(
        {
            "ordered_step_preview": ordered_preview,
            "action_type_counts": step_types,
            "top_contexts": _top_counts(contexts_all, limit=5),
            "top_click_targets": _top_counts(click_terms, limit=5),
            "top_typed_terms": _top_counts(typed_terms, limit=5),
            "top_key_inputs": _top_counts(key_terms, limit=5),
        }
    )

    prompt_input = _compact_dict(
        {
            "what_this_json_is": "Explain Task payload with both raw and filtered evidence.",
            "task_id": task.task_id,
            "task_name": task.task_name,
            "signature": task.signature,
            "repeat_count": task.repeat_count,
            "last_used": task.last_used,
            "confidence_score": round(task.confidence_score, 4) if task.confidence_score is not None else None,
            "raw_actions_full": raw_actions_full,
            "raw_action_count": len(raw_actions_full),
            "actions_filtered_for_signal": filtered_actions,
            "filtered_action_count": len(filtered_actions),
            "action_details": action_details,
            "platforms": platforms,
            "views": views,
            "text_inputs": text_inputs,
            "click_targets": click_targets,
            "explicit_actions": explicit_actions,
            "key_inputs": key_inputs,
            "step_type_counts": step_types,
            "evidence_summary": evidence_summary,
        }
    )
    return filtered_actions, prompt_input


def _to_platform_name(value: str) -> str:
    parts = [part.capitalize() for part in value.split() if part]
    if not parts:
        return "the application"
    if len(parts) == 1 and parts[0].lower() == "linkedin":
        return "LinkedIn"
    return " ".join(parts)


def _post_process_explanation(value: str) -> str:
    text = re.sub(r"\s+", " ", (value or "").strip()).strip("\"' ")
    if not text:
        return ""

    sentence_matches = re.findall(r"[^.!?]+[.!?]?", text)
    sentences = [match.strip() for match in sentence_matches if match.strip()]
    if not sentences:
        return text

    limited = " ".join(sentences[:2]).strip()
    if limited and limited[-1] not in ".!?":
        limited += "."
    return limited


def _clamp01(value: float) -> float:
    return max(0.0, min(1.0, value))


def _heuristic_repeated_judgement(
    *,
    repeat_count: int,
    confidence_score: float | None,
) -> tuple[bool, float, str]:
    base = 0.45
    if repeat_count >= 2:
        base += min(0.4, 0.08 * (repeat_count - 1))
    if confidence_score is not None:
        base = 0.55 * base + 0.45 * confidence_score
    score = _clamp01(base)
    is_repeated = repeat_count >= 2 and score >= 0.55
    reason = (
        f"Detected {repeat_count} occurrence(s)"
        + (f" with pattern confidence {confidence_score:.2f}" if confidence_score is not None else "")
    )
    return is_repeated, score, reason


def _parse_bool_token(raw: str) -> bool | None:
    x = raw.strip().lower()
    if x in {"true", "yes", "y", "1"}:
        return True
    if x in {"false", "no", "n", "0"}:
        return False
    return None


def _extract_structured_lines(raw: str) -> dict[str, str]:
    out: dict[str, str] = {}
    for line in (raw or "").splitlines():
        if ":" not in line:
            continue
        key, value = line.split(":", 1)
        k = key.strip().upper()
        if k in {"SUMMARY", "INTENT", "IS_REPEATED", "REPEATED_CONFIDENCE", "REPEATED_REASON"}:
            out[k] = value.strip()
    return out


def _split_narrative_metadata(raw: str) -> tuple[str, str]:
    """Returns (narrative_for_ui, metadata_lines). Metadata may be empty if the model omitted the delimiter."""
    text = (raw or "").strip()
    for marker in METADATA_MARKERS:
        if marker in text:
            before, after = text.split(marker, 1)
            return before.strip(), after.strip()
    return text, ""


def _parse_llm_result(
    *,
    raw: str,
    repeat_count: int,
    confidence_score: float | None,
) -> tuple[str, bool, float, str]:
    narrative, meta_tail = _split_narrative_metadata(raw)
    fields = _extract_structured_lines(meta_tail if meta_tail else raw)

    heur_rep, heur_score, heur_reason = _heuristic_repeated_judgement(
        repeat_count=repeat_count,
        confidence_score=confidence_score,
    )

    explanation = narrative.strip() if narrative.strip() else _post_process_explanation(raw)

    parsed_bool = _parse_bool_token(fields.get("IS_REPEATED", ""))
    parsed_score_raw = fields.get("REPEATED_CONFIDENCE", "")
    try:
        parsed_score = _clamp01(float(parsed_score_raw)) if parsed_score_raw else heur_score
    except ValueError:
        parsed_score = heur_score
    is_repeated = parsed_bool if parsed_bool is not None else heur_rep
    return explanation, is_repeated, parsed_score, heur_reason


def _infer_platform(actions: list[str]) -> str:
    contexts: list[str] = []
    for action in actions:
        _, _, payload = action.partition(":")
        context = ""
        if action.startswith("VIEW:"):
            context = payload
        elif "@" in payload:
            context = payload.split("@", 1)[1]
        cleaned = _normalize_text(context, max_len=48)
        if cleaned and cleaned not in contexts:
            contexts.append(cleaned)

    if not contexts:
        return ""

    keyword_map = {
        "linkedin": "LinkedIn",
        "gmail": "Gmail",
        "github": "GitHub",
        "slack": "Slack",
        "notion": "Notion",
        "excel": "Excel",
        "google docs": "Google Docs",
        "google sheets": "Google Sheets",
    }
    for candidate in contexts:
        for keyword, label in keyword_map.items():
            if keyword in candidate:
                return label

    preferred = contexts[-1]
    return _to_platform_name(preferred)


def _infer_search_term(actions: list[str]) -> str:
    for action in actions:
        if action.startswith("SUBMIT_TEXT:") or action.startswith("TYPE_TEXT:"):
            value = action.split(":", 1)[1]
            normalized = _normalize_text(value, max_len=64)
            if normalized and normalized != "unknown":
                return normalized
    return ""


def _infer_object(actions: list[str]) -> str:
    nouns: list[str] = []
    for action in actions:
        if action.startswith("CLICK:"):
            value = action.split(":", 1)[1].split("@", 1)[0]
            normalized = _normalize_text(value, max_len=48)
            if normalized and normalized not in {"left", "right", "button", "target"} and normalized not in nouns:
                nouns.append(normalized)
        if action.startswith("VIEW:"):
            value = action.split(":", 1)[1]
            normalized = _normalize_text(value, max_len=48)
            if normalized and normalized not in nouns:
                nouns.append(normalized)
    return nouns[-1] if nouns else ""


def _heuristic_paragraph(actions: list[str], repeat_count: int) -> str:
    """Build a simple, human-friendly paragraph explaining the detected task."""
    platform = _infer_platform(actions)
    search_term = _infer_search_term(actions)
    obj = _infer_object(actions)

    if search_term and platform:
        return (
            f'You were repeatedly searching for "{search_term}" on {platform}. '
            f"This pattern was detected {repeat_count} time(s) - each time you navigated to {platform}, "
            f"typed your query, and browsed through results. "
            f"This is a routine workflow that could be automated so you just review the results instead of repeating the same steps."
        )
    if search_term:
        return (
            f'You kept typing "{search_term}" as part of a repeated workflow. '
            f"This same search or input pattern appeared {repeat_count} time(s) across your sessions. "
            f"Since you're entering the same thing repeatedly, this could be automated to save you the effort."
        )
    if obj and platform:
        return (
            f'You were repeatedly interacting with "{obj}" on {platform}. '
            f"This pattern showed up {repeat_count} time(s) - you kept navigating to the same area and performing similar actions. "
            f"This looks like a routine task that could be streamlined or automated."
        )
    if platform:
        return (
            f"You were going through the same series of steps on {platform} repeatedly. "
            f"This workflow pattern was detected {repeat_count} time(s). "
            f"Since you keep doing the same thing, FlowPilot can help automate this routine for you."
        )
    if obj:
        return (
            f'You kept coming back to "{obj}" and performing similar actions around it. '
            f"This pattern repeated {repeat_count} time(s). "
            f"It looks like a routine task that could benefit from automation."
        )
    return (
        f"A repeated workflow pattern was detected {repeat_count} time(s) in your activity. "
        f"You performed the same sequence of actions multiple times. "
        f"This kind of repetitive task is a good candidate for automation."
    )


def _heuristic_explanation_with_metadata(
    actions: list[str],
    repeat_count: int,
    confidence_score: float | None,
) -> str:
    paragraph = _heuristic_paragraph(actions, repeat_count)
    is_rep, score, _reason = _heuristic_repeated_judgement(
        repeat_count=repeat_count,
        confidence_score=confidence_score,
    )
    return (
        f"{paragraph}\n\n---METADATA---\n"
        f"IS_REPEATED: {str(is_rep).lower()}\n"
        f"REPEATED_CONFIDENCE: {score:.2f}"
    )


class HeuristicTaskExplainer:
    provider_name = "heuristic"

    def explain_repeated_task(self, *, prompt_input: dict[str, object], repeat_count: int) -> str:
        prompt = _build_prompt(prompt_input, repeat_count)
        _log_llm_debug_block("EXPLAIN_TASK_HEURISTIC_PROMPT", prompt)
        actions = [str(value) for value in prompt_input.get("actions", []) if isinstance(value, str)]
        conf = prompt_input.get("confidence_score")
        cs: float | None
        if isinstance(conf, (int, float)):
            cs = float(conf)
        else:
            cs = None
        raw = _heuristic_explanation_with_metadata(actions, repeat_count, cs)
        _log_llm_debug_block("EXPLAIN_TASK_HEURISTIC_RAW_REPLY", raw)
        return raw


class OpenAICompatibleTaskExplainer:
    def __init__(self, *, provider_name: str, endpoint: str, api_key: str, model: str) -> None:
        self.provider_name = provider_name
        self._endpoint = endpoint
        self._api_key = api_key
        self._model = model

    def explain_repeated_task(self, *, prompt_input: dict[str, object], repeat_count: int) -> str:
        prompt = _build_prompt(prompt_input, repeat_count)
        _log_llm_debug_block("EXPLAIN_TASK_LLM_PROMPT", prompt)
        payload = {
            "model": self._model,
            "messages": [{"role": "user", "content": prompt}],
            "temperature": 0.3,
            "max_tokens": 300,
        }
        body = json.dumps(payload, ensure_ascii=True).encode("utf-8")
        req = request.Request(
            self._endpoint,
            data=body,
            headers={
                "Authorization": f"Bearer {self._api_key}",
                "Content-Type": "application/json",
                "Accept": "application/json",
                "User-Agent": "FlowPilotTaskExplainer/1.0 (+local-dev)",
            },
            method="POST",
        )
        try:
            with request.urlopen(req, timeout=12) as resp:
                data = json.loads(resp.read().decode("utf-8"))
        except error.HTTPError as exc:
            detail = exc.read().decode("utf-8", errors="replace")
            _log_llm_debug_block(
                "EXPLAIN_TASK_LLM_HTTP_ERROR",
                f"status={exc.code}\nurl={self._endpoint}\nbody=\n{detail}",
            )
            raise RuntimeError(f"LLM request failed with {exc.code}: {detail}") from exc
        except error.URLError as exc:
            raise RuntimeError(f"LLM request failed: {exc.reason}") from exc

        choices = data.get("choices") or []
        if not choices:
            raise RuntimeError("LLM response did not include choices")

        message = choices[0].get("message") or {}
        content = message.get("content")
        if isinstance(content, list):
            parts = []
            for item in content:
                if isinstance(item, dict) and item.get("type") == "text":
                    parts.append(str(item.get("text", "")))
            content = " ".join(parts)
        if not isinstance(content, str) or not content.strip():
            raise RuntimeError("LLM response did not include message content")
        _log_llm_debug_block("EXPLAIN_TASK_LLM_RAW_REPLY", content)
        return content


def get_task_explainer() -> TaskExplainer:
    provider = _get_env("TASK_EXPLAINER_PROVIDER", "heuristic").lower()
    if provider in {"groq", "openai", "openai-compatible"}:
        endpoint = _get_env("TASK_EXPLAINER_ENDPOINT", "")
        api_key = _get_env("TASK_EXPLAINER_API_KEY", "")
        model = _get_env("TASK_EXPLAINER_MODEL", "")
        if endpoint and api_key and model:
            return OpenAICompatibleTaskExplainer(
                provider_name=provider,
                endpoint=endpoint,
                api_key=api_key,
                model=model,
            )
        _LOGGER.warning(
            "TASK_EXPLAINER configured for '%s' but endpoint/api_key/model is missing; falling back to heuristic.",
            provider,
        )
    else:
        _LOGGER.warning(
            "TASK_EXPLAINER_PROVIDER is '%s' (or unset); using heuristic provider. "
            "Set TASK_EXPLAINER_PROVIDER + ENDPOINT + API_KEY + MODEL to enable real LLM calls.",
            provider or "unset",
        )
    return HeuristicTaskExplainer()


def explain_repeated_task_details(task: TaskExplanationInput) -> TaskExplanationResult:
    prompt_actions, prompt_input = _build_prompt_input(task)
    canonical_actions = _canonical_actions(prompt_actions)
    if not prompt_actions:
        prompt_actions = list(canonical_actions)
        prompt_input = {"actions": prompt_actions, "repeat_count": task.repeat_count}
    exact_key = _cache_key(canonical_actions)
    # Intentionally bypass explanation cache: always call LLM/fallback for fresh intent reasoning.

    explainer = get_task_explainer()
    model_name = getattr(explainer, "_model", "n/a")
    _LOGGER.warning(
        "EXPLAIN_TASK provider=%s model=%s repeat_count=%s action_count=%s",
        explainer.provider_name,
        model_name,
        task.repeat_count,
        len(task.actions),
    )
    try:
        raw = explainer.explain_repeated_task(prompt_input=prompt_input, repeat_count=task.repeat_count)
        explanation, is_repeated, repeated_confidence, repeated_reason = _parse_llm_result(
            raw=raw,
            repeat_count=task.repeat_count,
            confidence_score=task.confidence_score,
        )
        if not explanation:
            raise RuntimeError("LLM returned an empty explanation")
        result = TaskExplanationResult(
            explanation=explanation,
            provider=explainer.provider_name,
            cached=False,
            used_fallback=False,
            cache_key=exact_key,
            is_repeated=is_repeated,
            repeated_confidence=repeated_confidence,
            repeated_reason=repeated_reason,
        )
    except Exception:
        raw_h = _heuristic_explanation_with_metadata(prompt_actions, task.repeat_count, task.confidence_score)
        fb_explanation, is_repeated, repeated_confidence, repeated_reason = _parse_llm_result(
            raw=raw_h,
            repeat_count=task.repeat_count,
            confidence_score=task.confidence_score,
        )
        result = TaskExplanationResult(
            explanation=fb_explanation,
            provider="heuristic",
            cached=False,
            used_fallback=explainer.provider_name != "heuristic",
            cache_key=exact_key,
            is_repeated=is_repeated,
            repeated_confidence=repeated_confidence,
            repeated_reason=repeated_reason,
        )

    return result


def explain_repeated_task(task: TaskExplanationInput) -> str:
    return explain_repeated_task_details(task).explanation
