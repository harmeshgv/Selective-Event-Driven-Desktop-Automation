from __future__ import annotations

import hashlib
import math
import re
from threading import Lock
from dataclasses import dataclass
from datetime import datetime, timedelta, timezone
from difflib import SequenceMatcher
from typing import Iterable, List, Sequence

from sqlalchemy import func
from sqlalchemy.orm import Session

from backend.db.models import ObservedLog, TaskPattern, TaskStep


_BROWSER_TOKENS = {
    "google chrome",
    "chrome",
    "microsoft edge",
    "edge",
    "mozilla firefox",
    "firefox",
    "brave",
    "opera",
}
_KEYBOARD_MODIFIERS = {"<SHIFT>", "<CTRL>", "<ALT>", "<CMD>", "<META>", "<CAPS_LOCK>"}
_TYPE_FAMILIES = {"TYPE_TEXT", "SUBMIT_TEXT"}


@dataclass(frozen=True)
class Event:
    timestamp: datetime
    app: str
    action: str
    coordinates: str = ""
    text: str = ""
    screenshot_path: str = ""


@dataclass(frozen=True)
class NormalizedToken:
    token: str
    family: str
    context: str
    value: str
    coord_bucket: str
    timestamp: datetime


@dataclass(frozen=True)
class PatternOccurrence:
    start_index: int
    end_index: int
    score: float


@dataclass(frozen=True)
class Pattern:
    signature: str
    steps: list[str]
    repetitions: int
    confidence: float
    occurrences: list[PatternOccurrence]
    last_used: str


@dataclass(frozen=True)
class TaskGroupResult:
    task_id: int
    signature: str
    name: str
    frequency: int
    last_used: str
    steps: List[str]
    confidence_score: float


@dataclass(frozen=True)
class TaskDiscoveryCacheEntry:
    log_count: int
    max_log_id: int
    limit_logs: int
    min_steps: int
    max_steps: int
    created_at: datetime
    results: tuple[TaskGroupResult, ...]


_TASK_DISCOVERY_CACHE: TaskDiscoveryCacheEntry | None = None
_TASK_DISCOVERY_CACHE_LOCK = Lock()


def _coerce_event(raw: Event | ObservedLog) -> Event:
    if isinstance(raw, Event):
        return raw
    return Event(
        timestamp=raw.timestamp,
        app=raw.app,
        action=raw.action,
        coordinates=raw.coordinates,
        text=raw.text,
        screenshot_path=raw.screenshot_path,
    )


def _normalize_free_text(value: str, *, max_len: int = 32) -> str:
    cleaned = re.sub(r"[^a-zA-Z0-9]+", " ", value or "").strip().lower()
    cleaned = re.sub(r"\s+", " ", cleaned)
    if not cleaned:
        return ""
    return cleaned[:max_len]


def _normalize_window_title(title: str) -> str:
    raw = re.sub(r"\s+", " ", (title or "").strip().lower())
    if not raw:
        return "unknown"

    parts = re.split(r"\s+[|\-–—]\s+|\s+:\s+", raw)
    cleaned_parts: list[str] = []
    for part in parts:
        cleaned = _normalize_free_text(part, max_len=48)
        if not cleaned or cleaned in _BROWSER_TOKENS:
            continue
        if cleaned not in cleaned_parts:
            cleaned_parts.append(cleaned)

    if not cleaned_parts:
        return _normalize_free_text(raw, max_len=48) or "unknown"
    if len(cleaned_parts) == 1:
        return cleaned_parts[0]
    return " / ".join(cleaned_parts[-2:])


def _parse_coordinates(raw: str) -> tuple[int, int] | None:
    match = re.search(r"x=(\d+),y=(\d+)", raw or "")
    if not match:
        return None
    return int(match.group(1)), int(match.group(2))


def _bucket_coordinates(raw: str) -> str:
    coords = _parse_coordinates(raw)
    if coords is None:
        return ""
    x, y = coords
    return f"c{min(4, x // 320)}r{min(4, y // 180)}"


def _string_similarity(left: str, right: str) -> float:
    if not left and not right:
        return 1.0
    if not left or not right:
        return 0.0
    return SequenceMatcher(a=left, b=right).ratio()


def _append_view_token(tokens: list[NormalizedToken], context: str, timestamp: datetime) -> None:
    if not context:
        return
    if tokens and tokens[-1].family == "VIEW" and tokens[-1].context == context:
        return
    tokens.append(
        NormalizedToken(
            token=f"VIEW:{context}",
            family="VIEW",
            context=context,
            value=context,
            coord_bucket="",
            timestamp=timestamp,
        )
    )


def _append_token(tokens: list[NormalizedToken], token: NormalizedToken) -> None:
    if tokens and token.family in {"VIEW", "CLICK"}:
        prev = tokens[-1]
        if prev.token == token.token and (token.timestamp - prev.timestamp) <= timedelta(seconds=1):
            return
    tokens.append(token)


def _tokenize_events(events: Sequence[Event | ObservedLog]) -> list[NormalizedToken]:
    ordered = sorted((_coerce_event(event) for event in events), key=lambda e: e.timestamp)
    tokens: list[NormalizedToken] = []

    typed_chars: list[str] = []
    typed_context = ""
    typed_started_at: datetime | None = None
    typed_last_at: datetime | None = None
    last_context = ""

    def flush_text_buffer(*, submit: bool = False) -> None:
        nonlocal typed_chars, typed_context, typed_started_at, typed_last_at
        if not typed_chars or typed_started_at is None:
            typed_chars = []
            typed_context = ""
            typed_started_at = None
            typed_last_at = None
            return

        normalized_text = _normalize_free_text("".join(typed_chars), max_len=24)
        if normalized_text:
            family = "SUBMIT_TEXT" if submit else "TYPE_TEXT"
            _append_token(
                tokens,
                NormalizedToken(
                    token=f"{family}:{normalized_text}",
                    family=family,
                    context=typed_context or "unknown",
                    value=normalized_text,
                    coord_bucket="",
                    timestamp=typed_started_at,
                ),
            )

        typed_chars = []
        typed_context = ""
        typed_started_at = None
        typed_last_at = None

    for event in ordered:
        action = (event.action or "").strip()
        raw_text = (event.text or "").strip()
        context = _normalize_window_title(event.app)

        if action not in {"mouse_move", "screenshot"} and context != last_context:
            flush_text_buffer()
            _append_view_token(tokens, context, event.timestamp)
            last_context = context

        if action in {"mouse_move", "screenshot"}:
            continue

        if action == "key_press":
            if raw_text in _KEYBOARD_MODIFIERS:
                continue

            if raw_text in {"<ENTER>", "<TAB>", "<BACKSPACE>"}:
                if raw_text == "<ENTER>" and typed_chars:
                    flush_text_buffer(submit=True)
                else:
                    flush_text_buffer()
                    _append_token(
                        tokens,
                        NormalizedToken(
                            token=f"KEY:{raw_text}",
                            family="KEY",
                            context=context,
                            value=raw_text,
                            coord_bucket="",
                            timestamp=event.timestamp,
                        ),
                    )
                continue

            char_value = raw_text
            if char_value == "<masked>":
                flush_text_buffer()
                _append_token(
                    tokens,
                    NormalizedToken(
                        token=f"TYPE_TEXT:{char_value}",
                        family="TYPE_TEXT",
                        context=context,
                        value=char_value,
                        coord_bucket="",
                        timestamp=event.timestamp,
                    ),
                )
                continue

            if len(char_value) == 1 or char_value == " ":
                if (
                    typed_last_at is not None
                    and (event.timestamp - typed_last_at) > timedelta(seconds=2)
                ):
                    flush_text_buffer()
                if typed_context and typed_context != context:
                    flush_text_buffer()
                if typed_started_at is None:
                    typed_started_at = event.timestamp
                typed_context = context
                typed_chars.append(char_value)
                typed_last_at = event.timestamp
                continue

            flush_text_buffer()
            normalized_text = _normalize_free_text(char_value, max_len=24)
            if normalized_text:
                _append_token(
                    tokens,
                    NormalizedToken(
                        token=f"TYPE_TEXT:{normalized_text}",
                        family="TYPE_TEXT",
                        context=context,
                        value=normalized_text,
                        coord_bucket="",
                        timestamp=event.timestamp,
                    ),
                )
            continue

        flush_text_buffer()
        if action.startswith("mouse_click"):
            button = _normalize_free_text(raw_text or "button", max_len=12).replace(" ", "_") or "button"
            _append_token(
                tokens,
                NormalizedToken(
                    token=f"CLICK:{button}@{context}",
                    family="CLICK",
                    context=context,
                    value=button,
                    coord_bucket=_bucket_coordinates(event.coordinates),
                    timestamp=event.timestamp,
                ),
            )
            continue

        normalized_action = _normalize_free_text(action, max_len=24).replace(" ", "_") or "unknown"
        _append_token(
            tokens,
            NormalizedToken(
                token=f"ACTION:{normalized_action}@{context}",
                family="ACTION",
                context=context,
                value=normalized_action,
                coord_bucket="",
                timestamp=event.timestamp,
            ),
        )

    flush_text_buffer()
    return tokens


def _derive_signature(steps: Iterable[str]) -> str:
    joined = "|".join(step for step in steps if step)
    if len(joined) <= 900:
        return joined
    digest = hashlib.sha1(joined.encode("utf-8")).hexdigest()[:16]
    prefix = "|".join(joined.split("|")[:4])
    return f"{prefix}|HASH:{digest}"


def _token_similarity(left: NormalizedToken, right: NormalizedToken) -> float:
    if left.token == right.token:
        return 1.0

    if left.family == right.family == "VIEW":
        return _string_similarity(left.context, right.context)

    if left.family == right.family == "CLICK":
        context_score = _string_similarity(left.context, right.context)
        button_score = 1.0 if left.value == right.value else 0.5
        bucket_bonus = 0.15 if left.coord_bucket and left.coord_bucket == right.coord_bucket else 0.0
        return min(1.0, 0.65 * context_score + 0.25 * button_score + bucket_bonus)

    if left.family in _TYPE_FAMILIES and right.family in _TYPE_FAMILIES:
        context_score = _string_similarity(left.context, right.context)
        text_score = _string_similarity(left.value, right.value)
        submit_bonus = 0.1 if left.family == right.family else 0.0
        return min(1.0, 0.2 * context_score + 0.7 * text_score + submit_bonus)

    if left.family == right.family == "KEY":
        return 1.0 if left.value == right.value else 0.0

    if left.family != right.family:
        return 0.0

    context_score = _string_similarity(left.context, right.context)
    value_score = _string_similarity(left.value, right.value)
    return 0.5 * context_score + 0.5 * value_score


def _sequence_similarity(pattern: Sequence[NormalizedToken], window: Sequence[NormalizedToken]) -> float:
    if not pattern or not window:
        return 0.0

    delete_cost = 0.55
    insert_cost = 0.35
    rows = len(pattern)
    cols = len(window)
    dp: list[list[float]] = [[0.0] * (cols + 1) for _ in range(rows + 1)]

    for i in range(1, rows + 1):
        dp[i][0] = dp[i - 1][0] + delete_cost
    for j in range(1, cols + 1):
        dp[0][j] = dp[0][j - 1] + insert_cost

    for i in range(1, rows + 1):
        for j in range(1, cols + 1):
            match_cost = 1.0 - _token_similarity(pattern[i - 1], window[j - 1])
            dp[i][j] = min(
                dp[i - 1][j - 1] + match_cost,
                dp[i - 1][j] + delete_cost,
                dp[i][j - 1] + insert_cost,
            )

    distance = dp[rows][cols]
    normalizer = max(rows, cols)
    return max(0.0, 1.0 - (distance / normalizer))


def _is_informative_pattern(pattern: Sequence[NormalizedToken]) -> bool:
    if len(pattern) < 3:
        return False
    distinct_tokens = {token.token for token in pattern}
    if len(distinct_tokens) < 3:
        return False
    non_view = [token for token in pattern if token.family != "VIEW"]
    return len(non_view) >= 2


def _find_next_occurrence(
    pattern: Sequence[NormalizedToken],
    tokens: Sequence[NormalizedToken],
    start_index: int,
    candidate_starts: dict[str, list[int]],
    *,
    min_similarity: float,
    length_slack: int,
) -> PatternOccurrence | None:
    if not pattern:
        return None

    first_family = pattern[0].family
    candidate_positions = candidate_starts.get(first_family, [])
    target_len = len(pattern)

    for position in candidate_positions:
        if position < start_index:
            continue
        if _token_similarity(pattern[0], tokens[position]) < 0.55:
            continue

        best_end = -1
        best_score = 0.0
        min_window = max(2, target_len - length_slack)
        max_window = min(len(tokens) - position, target_len + length_slack)
        for window_len in range(min_window, max_window + 1):
            window = tokens[position : position + window_len]
            score = _sequence_similarity(pattern, window)
            if score > best_score:
                best_score = score
                best_end = position + window_len

        if best_score >= min_similarity and best_end > position:
            return PatternOccurrence(start_index=position, end_index=best_end, score=best_score)

    return None


def _occurrence_overlap(left: PatternOccurrence, right: PatternOccurrence) -> int:
    start = max(left.start_index, right.start_index)
    end = min(left.end_index, right.end_index)
    return max(0, end - start)


def _patterns_are_similar(left: Pattern, right: Pattern) -> bool:
    if left.signature == right.signature:
        return True

    left_tokens = [_token_from_step(step) for step in left.steps]
    right_tokens = [_token_from_step(step) for step in right.steps]
    score = _sequence_similarity(left_tokens, right_tokens)
    if score < 0.82:
        return False

    for left_occurrence in left.occurrences:
        for right_occurrence in right.occurrences:
            if _occurrence_overlap(left_occurrence, right_occurrence) >= min(
                left_occurrence.end_index - left_occurrence.start_index,
                right_occurrence.end_index - right_occurrence.start_index,
            ) * 0.6:
                return True
    return False


def _token_from_step(step: str) -> NormalizedToken:
    now = datetime.now(timezone.utc)
    if step.startswith("VIEW:"):
        context = step.split(":", 1)[1]
        return NormalizedToken(step, "VIEW", context, context, "", now)
    if step.startswith("CLICK:"):
        body = step.split(":", 1)[1]
        button, _, context = body.partition("@")
        return NormalizedToken(step, "CLICK", context, button, "", now)
    if step.startswith("SUBMIT_TEXT:"):
        value = step.split(":", 1)[1]
        return NormalizedToken(step, "SUBMIT_TEXT", "", value, "", now)
    if step.startswith("TYPE_TEXT:"):
        value = step.split(":", 1)[1]
        return NormalizedToken(step, "TYPE_TEXT", "", value, "", now)
    if step.startswith("KEY:"):
        value = step.split(":", 1)[1]
        return NormalizedToken(step, "KEY", "", value, "", now)
    if step.startswith("ACTION:"):
        body = step.split(":", 1)[1]
        value, _, context = body.partition("@")
        return NormalizedToken(step, "ACTION", context, value, "", now)
    return NormalizedToken(step, "UNKNOWN", "", step, "", now)


def detect_repeated_sequences(
    events: List[Event],
    *,
    min_pattern_steps: int = 3,
    max_pattern_steps: int = 12,
    min_repetitions: int = 2,
    min_similarity: float = 0.72,
    max_returned_patterns: int = 10,
) -> List[Pattern]:
    """
    Detect repeated multi-step workflows from noisy UI event streams.

    The detector first collapses low-level events into higher-level tokens
    (for example view changes, text submissions, and clicks) and then scans
    sliding windows from longest to shortest. Candidate windows are matched
    against later subsequences with a weighted edit-distance score, which
    tolerates optional extra events, small text variation, and ignored noise.

    With ``n`` normalized tokens and a maximum pattern length ``L``, the
    practical runtime is O(n^2 * L^2). In this codebase ``L`` is bounded to a
    small constant, so the detector is quadratic in the number of meaningful
    events after noise filtering.
    """

    tokens = _tokenize_events(events)
    if len(tokens) < min_pattern_steps * min_repetitions:
        return []

    max_pattern_steps = min(max_pattern_steps, max(min_pattern_steps, len(tokens) // min_repetitions))
    candidate_starts: dict[str, list[int]] = {}
    for idx, token in enumerate(tokens):
        candidate_starts.setdefault(token.family, []).append(idx)

    found: list[Pattern] = []
    exact_seen: set[str] = set()
    length_slack = 3

    for pattern_len in range(max_pattern_steps, min_pattern_steps - 1, -1):
        for start_index in range(0, len(tokens) - pattern_len + 1):
            pattern_tokens = tokens[start_index : start_index + pattern_len]
            if not _is_informative_pattern(pattern_tokens):
                continue

            signature = _derive_signature(token.token for token in pattern_tokens)
            if signature in exact_seen:
                continue

            occurrences = [
                PatternOccurrence(
                    start_index=start_index,
                    end_index=start_index + pattern_len,
                    score=1.0,
                )
            ]
            search_from = start_index + pattern_len
            while search_from < len(tokens):
                occurrence = _find_next_occurrence(
                    pattern_tokens,
                    tokens,
                    search_from,
                    candidate_starts,
                    min_similarity=min_similarity,
                    length_slack=length_slack,
                )
                if occurrence is None:
                    break
                occurrences.append(occurrence)
                search_from = occurrence.end_index

            if len(occurrences) < min_repetitions:
                continue

            similarities = [match.score for match in occurrences[1:]]
            compactness = [
                pattern_len / max(1, match.end_index - match.start_index)
                for match in occurrences[1:]
            ]
            mean_similarity = sum(similarities) / max(1, len(similarities))
            mean_compactness = sum(compactness) / max(1, len(compactness))
            repetition_bonus = min(1.0, len(occurrences) / 4.0)
            confidence = min(
                1.0,
                0.55 * mean_similarity + 0.25 * mean_compactness + 0.20 * repetition_bonus,
            )

            last_used = tokens[occurrences[-1].end_index - 1].timestamp.isoformat()
            found.append(
                Pattern(
                    signature=signature,
                    steps=[token.token for token in pattern_tokens],
                    repetitions=len(occurrences),
                    confidence=round(confidence, 4),
                    occurrences=occurrences,
                    last_used=last_used,
                )
            )
            exact_seen.add(signature)

    found.sort(
        key=lambda pattern: (
            -len(pattern.steps),
            -pattern.repetitions,
            -pattern.confidence,
            pattern.occurrences[0].start_index,
        )
    )

    deduplicated: list[Pattern] = []
    for pattern in found:
        if any(_patterns_are_similar(pattern, accepted) for accepted in deduplicated):
            continue
        deduplicated.append(pattern)
        if len(deduplicated) >= max_returned_patterns:
            break

    return deduplicated


def _infer_step_label(token: str) -> str | None:
    if not token:
        return None
    if token.startswith("VIEW:"):
        return f"Open {token.split(':', 1)[1]}"
    if token.startswith("CLICK:"):
        body = token.split(":", 1)[1]
        button = body.split("@", 1)[0].replace("_", " ").upper()
        return f"Click {button}"
    if token.startswith("SUBMIT_TEXT:"):
        text = token.split(":", 1)[1]
        return f"Submit '{text}'"
    if token.startswith("TYPE_TEXT:"):
        text = token.split(":", 1)[1]
        return f"Type '{text}'"
    if token.startswith("KEY:"):
        key_name = token.split(":", 1)[1].replace("<", "").replace(">", "")
        return f"Press {key_name}"
    if token.startswith("ACTION:"):
        action_name = token.split(":", 1)[1].split("@", 1)[0].replace("_", " ")
        return action_name.title()
    return token


def _infer_task_name(steps: list[str]) -> str:
    labels: list[str] = []
    for step in steps:
        label = _infer_step_label(step)
        if label:
            labels.append(label)
        if len(labels) >= 2:
            break
    if not labels:
        return "Repeated workflow"
    if len(labels) == 1:
        return labels[0]
    return f"{labels[0]} -> {labels[1]}"


def _load_recent_events(db: Session, *, limit_logs: int) -> list[Event]:
    rows = (
        db.query(
            ObservedLog.timestamp,
            ObservedLog.app,
            ObservedLog.action,
            ObservedLog.coordinates,
            ObservedLog.text,
            ObservedLog.screenshot_path,
        )
        .order_by(ObservedLog.timestamp.desc())
        .limit(limit_logs)
        .all()
    )
    return [
        Event(
            timestamp=row.timestamp,
            app=row.app,
            action=row.action,
            coordinates=row.coordinates,
            text=row.text,
            screenshot_path=row.screenshot_path,
        )
        for row in reversed(rows)
    ]


def _get_log_watermark(db: Session) -> tuple[int, int]:
    log_count, max_log_id = db.query(func.count(ObservedLog.id), func.max(ObservedLog.id)).one()
    return int(log_count or 0), int(max_log_id or 0)


def _get_cached_results(
    *,
    log_count: int,
    max_log_id: int,
    limit_logs: int,
    min_steps: int,
    max_steps: int,
    max_cache_age_seconds: int,
) -> list[TaskGroupResult] | None:
    with _TASK_DISCOVERY_CACHE_LOCK:
        cached = _TASK_DISCOVERY_CACHE
        if cached is None:
            return None
        # Fast path: serve recent cache for responsiveness even if new logs arrived.
        # This keeps the dashboard snappy while logs stream continuously.
        if (
            cached.limit_logs == limit_logs
            and cached.min_steps == min_steps
            and cached.max_steps == max_steps
            and (datetime.now(timezone.utc) - cached.created_at) <= timedelta(seconds=max_cache_age_seconds)
        ):
            return list(cached.results)
        if (
            cached.log_count != log_count
            or cached.max_log_id != max_log_id
            or cached.limit_logs != limit_logs
            or cached.min_steps != min_steps
            or cached.max_steps != max_steps
        ):
            return None
        return list(cached.results)


def _set_cached_results(
    *,
    log_count: int,
    max_log_id: int,
    limit_logs: int,
    min_steps: int,
    max_steps: int,
    results: list[TaskGroupResult],
) -> None:
    global _TASK_DISCOVERY_CACHE
    with _TASK_DISCOVERY_CACHE_LOCK:
        _TASK_DISCOVERY_CACHE = TaskDiscoveryCacheEntry(
            log_count=log_count,
            max_log_id=max_log_id,
            limit_logs=limit_logs,
            min_steps=min_steps,
            max_steps=max_steps,
            created_at=datetime.now(timezone.utc),
            results=tuple(results),
        )


def discover_and_persist_tasks(
    *,
    db: Session,
    limit_logs: int = 1000,
    segment_gap_seconds: int = 15,
    min_steps: int = 4,
    max_steps: int = 12,
) -> List[TaskGroupResult]:
    del segment_gap_seconds  # No longer needed after sequence-based detection.
    # Tune for UI responsiveness with constantly incoming events.
    max_cache_age_seconds = 12

    log_count, max_log_id = _get_log_watermark(db)
    if log_count == 0:
        _set_cached_results(
            log_count=0,
            max_log_id=0,
            limit_logs=limit_logs,
            min_steps=min_steps,
            max_steps=max_steps,
            results=[],
        )
        return []

    cached = _get_cached_results(
        log_count=log_count,
        max_log_id=max_log_id,
        limit_logs=limit_logs,
        min_steps=min_steps,
        max_steps=max_steps,
        max_cache_age_seconds=max_cache_age_seconds,
    )
    if cached is not None:
        return cached

    events = _load_recent_events(db, limit_logs=limit_logs)
    if not events:
        _set_cached_results(
            log_count=log_count,
            max_log_id=max_log_id,
            limit_logs=limit_logs,
            min_steps=min_steps,
            max_steps=max_steps,
            results=[],
        )
        return []

    patterns = detect_repeated_sequences(
        events,
        min_pattern_steps=min_steps,
        max_pattern_steps=max_steps,
    )
    if not patterns:
        _set_cached_results(
            log_count=log_count,
            max_log_id=max_log_id,
            limit_logs=limit_logs,
            min_steps=min_steps,
            max_steps=max_steps,
            results=[],
        )
        return []

    results: list[TaskGroupResult] = []
    for pattern in patterns:
        existing = db.query(TaskPattern).filter(TaskPattern.signature == pattern.signature).one_or_none()
        inferred_name = _infer_task_name(pattern.steps)
        last_used_dt = datetime.fromisoformat(pattern.last_used)

        if existing is None:
            task_row = TaskPattern(
                signature=pattern.signature,
                name=inferred_name,
                frequency=pattern.repetitions,
                last_used=last_used_dt,
            )
            db.add(task_row)
            db.flush()
        else:
            task_row = existing
            task_row.frequency = pattern.repetitions
            task_row.last_used = last_used_dt
            if not (task_row.name or "").strip():
                task_row.name = inferred_name

        db.query(TaskStep).filter(TaskStep.task_id == task_row.id).delete()
        for idx, step in enumerate(pattern.steps):
            db.add(TaskStep(task_id=task_row.id, step_order=idx, step=step))

        results.append(
            TaskGroupResult(
                task_id=task_row.id,
                signature=pattern.signature,
                name=task_row.name or inferred_name,
                frequency=pattern.repetitions,
                last_used=pattern.last_used,
                steps=pattern.steps,
                confidence_score=pattern.confidence,
            )
        )
        # Commit per pattern so log ingestion and other readers are not blocked for the whole loop.
        db.commit()
    _set_cached_results(
        log_count=log_count,
        max_log_id=max_log_id,
        limit_logs=limit_logs,
        min_steps=min_steps,
        max_steps=max_steps,
        results=results,
    )
    return results


def _snapshot_confidence_from_frequency(frequency: int) -> float:
    """Approximate confidence when serving DB snapshot (live discovery stores real scores)."""
    return float(min(1.0, 0.18 + math.log1p(max(0, frequency)) * 0.14))


def load_persisted_tasks_snapshot(db: Session) -> list[TaskGroupResult]:
    """Fast path: read task_patterns + task_steps only (no log scan / pattern detection)."""
    task_rows = db.query(TaskPattern).order_by(TaskPattern.frequency.desc()).all()
    out: list[TaskGroupResult] = []
    for tp in task_rows:
        step_rows = (
            db.query(TaskStep)
            .filter(TaskStep.task_id == tp.id)
            .order_by(TaskStep.step_order.asc())
            .all()
        )
        step_strs = [r.step for r in step_rows]
        lu = tp.last_used
        last_used = lu.astimezone(timezone.utc).isoformat() if lu.tzinfo else lu.replace(tzinfo=timezone.utc).isoformat()
        out.append(
            TaskGroupResult(
                task_id=tp.id,
                signature=tp.signature,
                name=(tp.name or "").strip(),
                frequency=tp.frequency,
                last_used=last_used,
                steps=step_strs,
                confidence_score=_snapshot_confidence_from_frequency(tp.frequency),
            )
        )
    return out
