from __future__ import annotations

import json
import logging
import os
import time as _time
from dataclasses import dataclass
from pathlib import Path
from typing import Generator, Literal
from urllib import error, request

_LOGGER = logging.getLogger("ai.llm_executor")

StepStatus = Literal["pending", "running", "success", "failed", "corrected"]


@dataclass
class ExecutionStepEvent:
    step_order: int
    description: str
    action_type: str
    target: str
    value: str
    status: StepStatus
    attempts: int
    error: str = ""
    llm_reasoning: str = ""


@dataclass
class ExecutionDoneEvent:
    status: Literal["success", "failed"]
    total_steps: int
    completed_steps: int
    error: str = ""


SYSTEM_PROMPT = """\
You are FlowPilot, a desktop automation engine on Windows.

You receive RAW USER ACTIVITY DATA showing what the user did on their computer.

YOUR JOB: Understand their goal and create a SIMPLE automation using subprocess commands.

=== ONLY USE THESE ACTION TYPES ===

1. "chrome_profile_picker" — Opens Chrome profile picker so user can choose their account.
   target: "" , value: ""

2. "chrome_open_url" — Opens a URL directly in Chrome via subprocess.
   target: the full URL (https://...) , value: "" (or Chrome profile name)

3. "open_url" — Opens a URL in the default browser.
   target: the full URL , value: ""

4. "wait" — Pause between steps.
   value: number of seconds (e.g. "5")

5. "run_command" — Run a shell command.
   value: the command string

6. "key_press" — Press a keyboard key.
   target: "KEY:<KEYNAME>" , value: ""

7. "hotkey" — Press a key combination.
   value: "ctrl+t" or "alt+f4"

8. "type" — Type text with keyboard.
   value: text to type

9. "noop" — Do nothing (skip).

=== RULES ===

- DO NOT use playwright_navigate, playwright_fill, playwright_press, or playwright_click_first_result. These are BANNED.
- DO NOT try to click on elements inside web pages. Just open the right URL directly.
- ALWAYS start web automations with: chrome_profile_picker → wait 5 → chrome_open_url
- Encode search terms directly into URLs. Example:
  LinkedIn job search → https://www.linkedin.com/jobs/search/?keywords=Machine%20Learning%20Intern
  Google search → https://www.google.com/search?q=python+developer
  YouTube search → https://www.youtube.com/results?search_query=tutorial
- Use %20 or + for spaces in URLs.
- Keep it to 3-4 steps max. Fewer steps = more reliable.
- Focus on the user's MAIN GOAL, ignore noise actions (random clicks, mouse moves, screenshots).

=== EXAMPLE ===

Raw actions: VIEW:LinkedIn, CLICK:search, TYPE_TEXT:machine learning intern, KEY:<ENTER>

Output:
{
  "intent": "Search LinkedIn for machine learning intern jobs",
  "steps": [
    {"step_order": 1, "description": "Open Chrome profile picker", "action_type": "chrome_profile_picker", "target": "", "value": "", "reasoning": "Let user pick their logged-in profile"},
    {"step_order": 2, "description": "Wait for profile selection", "action_type": "wait", "target": "", "value": "5", "reasoning": "Give user time to pick profile"},
    {"step_order": 3, "description": "Open LinkedIn ML intern job search", "action_type": "chrome_open_url", "target": "https://www.linkedin.com/jobs/search/?keywords=Machine%20Learning%20Intern", "value": "", "reasoning": "Go directly to search results"}
  ]
}

=== OUTPUT FORMAT ===

Respond ONLY with valid JSON (no markdown fences, no extra text):
{
  "intent": "one sentence",
  "steps": [{"step_order": 1, "description": "...", "action_type": "...", "target": "...", "value": "...", "reasoning": "..."}]
}"""

CORRECTION_PROMPT = """\
Step {step_order} FAILED.

Goal: {intent}

Failed step:
{step_json}

Error:
{error_message}

Fix this step. ONLY use these action types: chrome_profile_picker, chrome_open_url, open_url, wait, run_command, key_press, hotkey, type, noop.
DO NOT use any playwright actions.
If chrome_open_url failed, try open_url instead.

Respond ONLY with valid JSON:
{{
  "step_order": {step_order},
  "description": "...",
  "action_type": "...",
  "target": "...",
  "value": "...",
  "reasoning": "what you fixed"
}}"""


# ---------------------------------------------------------------------------
# Env / LLM helpers
# ---------------------------------------------------------------------------

def _dotenv_cache() -> dict[str, str]:
    env: dict[str, str] = {}
    repo_root = Path(__file__).resolve().parents[4]
    for p in [repo_root / ".env", repo_root / "app" / "backend" / ".env"]:
        if not p.exists():
            continue
        try:
            for raw in p.read_text(encoding="utf-8").splitlines():
                line = raw.strip()
                if not line or line.startswith("#") or "=" not in line:
                    continue
                k, v = line.split("=", 1)
                k = k.strip()
                if k:
                    env[k] = v.strip().strip("\"'")
        except OSError:
            continue
    return env


_ENV: dict[str, str] | None = None


def _env(name: str, default: str = "") -> str:
    global _ENV
    val = os.getenv(name, "").strip()
    if val:
        return val
    if _ENV is None:
        _ENV = _dotenv_cache()
    return _ENV.get(name, default).strip()


def _llm_config() -> tuple[str, str, str]:
    endpoint = _env("AUTOMATION_LLM_ENDPOINT") or _env("TASK_EXPLAINER_ENDPOINT")
    api_key = _env("AUTOMATION_LLM_API_KEY") or _env("TASK_EXPLAINER_API_KEY")
    model = _env("AUTOMATION_LLM_MODEL") or _env("TASK_EXPLAINER_MODEL")
    return endpoint, api_key, model


_LLM_MAX_RETRIES = 3
_LLM_BACKOFF_BASE = 2.0


def _call_llm(*, system: str, user: str, temperature: float = 0.2) -> str:
    endpoint, api_key, model = _llm_config()
    if not endpoint or not api_key or not model:
        raise RuntimeError(
            "LLM not configured. Set AUTOMATION_LLM_ENDPOINT/API_KEY/MODEL "
            "or TASK_EXPLAINER_ENDPOINT/API_KEY/MODEL in .env"
        )

    payload = {
        "model": model,
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": user},
        ],
        "temperature": temperature,
        "max_tokens": 2048,
    }
    body = json.dumps(payload, ensure_ascii=True).encode("utf-8")

    last_exc: Exception | None = None
    for attempt in range(1, _LLM_MAX_RETRIES + 1):
        req = request.Request(
            endpoint,
            data=body,
            headers={
                "Authorization": f"Bearer {api_key}",
                "Content-Type": "application/json",
                "Accept": "application/json",
                "User-Agent": "FlowPilot-AutomationExecutor/1.0",
            },
            method="POST",
        )
        try:
            with request.urlopen(req, timeout=30) as resp:
                data = json.loads(resp.read().decode("utf-8"))
            choices = data.get("choices") or []
            if not choices:
                raise RuntimeError("LLM returned no choices")
            content = (choices[0].get("message") or {}).get("content", "")
            if isinstance(content, list):
                content = " ".join(
                    item.get("text", "") for item in content if isinstance(item, dict)
                )
            return (content or "").strip()
        except error.HTTPError as exc:
            last_exc = exc
            code = exc.code
            detail = exc.read().decode("utf-8", errors="replace")
            _LOGGER.warning("LLM HTTP %s (attempt %d/%d): %s", code, attempt, _LLM_MAX_RETRIES, detail[:200])
            if code == 429 and attempt < _LLM_MAX_RETRIES:
                wait = _LLM_BACKOFF_BASE ** attempt
                _LOGGER.info("Rate limited, waiting %.1fs before retry", wait)
                _time.sleep(wait)
                continue
            raise RuntimeError(f"LLM request failed ({code})") from exc
        except error.URLError as exc:
            raise RuntimeError(f"LLM request failed: {exc.reason}") from exc

    raise RuntimeError(f"LLM request failed after {_LLM_MAX_RETRIES} retries") from last_exc


def _parse_json(raw: str) -> object:
    text = raw.strip()
    if text.startswith("```"):
        lines = text.splitlines()
        lines = lines[1:]
        if lines and lines[-1].strip() == "```":
            lines = lines[:-1]
        text = "\n".join(lines).strip()
    return json.loads(text)


# ---------------------------------------------------------------------------
# Safety net: convert any Playwright actions the LLM sneaks in
# ---------------------------------------------------------------------------

def _sanitize_step(step: dict) -> dict:
    """Auto-convert banned Playwright actions to subprocess equivalents."""
    at = step.get("action_type", "")

    if at == "playwright_navigate":
        url = step.get("target", "")
        step = {**step, "action_type": "chrome_open_url", "target": url}
        _LOGGER.info("Sanitized playwright_navigate -> chrome_open_url: %s", url)

    elif at in ("playwright_fill", "playwright_press", "playwright_click_first_result"):
        _LOGGER.warning("Dropping banned action %s, converting to noop", at)
        step = {**step, "action_type": "noop", "reasoning": f"Original {at} was blocked — not supported"}

    return step


def _sanitize_steps(steps: list[dict]) -> list[dict]:
    sanitized = [_sanitize_step(s) for s in steps]
    # Remove consecutive noops
    result = []
    for s in sanitized:
        if s.get("action_type") == "noop" and result and result[-1].get("action_type") == "noop":
            continue
        result.append(s)
    return result


# ---------------------------------------------------------------------------
# Noise filtering — remove app-switching, launcher, and FlowPilot actions
# ---------------------------------------------------------------------------

_NOISE_VIEWS = {
    "flowpilot", "search", "google chrome", "task manager",
    "desktop", "taskbar", "start menu", "start", "file explorer",
    "settings", "microsoft store",
}

_NOISE_PREFIXES = ("MOVE", "SCREENSHOT", "screenshot", "mouse_move")


def _is_noise_action(action: str, dominant_context: str) -> bool:
    lowered = action.lower().strip()

    if any(lowered.startswith(p.lower()) for p in _NOISE_PREFIXES):
        return True

    kind, _, payload = action.partition(":")
    kind_up = kind.strip().upper()
    payload_clean = payload.strip().lower()

    if kind_up == "VIEW" and payload_clean in _NOISE_VIEWS:
        return True

    # Short app-launcher searches like "chr", "not", "exc" — clearly not part of the workflow
    if kind_up in ("SUBMIT_TEXT", "TYPE_TEXT") and len(payload.strip()) <= 4 and " " not in payload.strip():
        return True

    # Clicks/views on FlowPilot itself
    if "flowpilot" in payload_clean:
        return True

    return False


def _find_dominant_context(actions: list[str]) -> str:
    """Find the most frequent VIEW context — that's the main app/site."""
    counts: dict[str, int] = {}
    for action in actions:
        kind, _, payload = action.partition(":")
        if kind.strip().upper() != "VIEW":
            continue
        ctx = payload.strip().lower()
        if ctx and ctx not in _NOISE_VIEWS:
            counts[ctx] = counts.get(ctx, 0) + 1
    if not counts:
        return ""
    return max(counts, key=lambda k: counts[k])


def _filter_noise(actions: list[str]) -> list[str]:
    """Remove noise actions and trailing app-switch garbage."""
    if not actions:
        return actions

    dominant = _find_dominant_context(actions)

    # First pass: mark each action as signal or noise
    cleaned = []
    trailing_noise_count = 0
    for action in actions:
        if _is_noise_action(action, dominant):
            trailing_noise_count += 1
            continue
        # If we had noise, and now we're back to signal, reset counter
        trailing_noise_count = 0
        cleaned.append(action)

    # If we removed everything, keep originals (better than nothing)
    if not cleaned:
        return actions

    _LOGGER.info(
        "Noise filter: %d raw -> %d clean (removed %d noise actions)",
        len(actions), len(cleaned), len(actions) - len(cleaned),
    )
    return cleaned


# ---------------------------------------------------------------------------
# Core: understand task from raw data, build & execute automation
# ---------------------------------------------------------------------------

@dataclass
class TaskContext:
    """Raw context about what the user was doing."""
    task_name: str
    raw_actions: list[str]
    frequency: int
    signature: str = ""


def _build_task_prompt(ctx: TaskContext) -> str:
    # Filter noise BEFORE sending to LLM
    clean_actions = _filter_noise(ctx.raw_actions)

    full_json = json.dumps({
        "task_name": ctx.task_name,
        "times_repeated": ctx.frequency,
        "total_raw_actions": len(ctx.raw_actions),
        "cleaned_actions": [
            {"step": i + 1, "action": a}
            for i, a in enumerate(clean_actions)
        ],
    }, indent=2, ensure_ascii=True)

    return (
        f"Here is the task data (noise-filtered) as JSON:\n\n{full_json}\n\n"
        f"ACTION FORMAT GUIDE:\n"
        f"- VIEW:page title = user viewed/navigated to a page\n"
        f"- CLICK:target@context = user clicked something\n"
        f"- TYPE_TEXT:text = user typed text\n"
        f"- SUBMIT_TEXT:text = user submitted text (search, form)\n"
        f"- KEY:<KEYNAME> = user pressed a key\n"
        f"- ACTION:name@context = user performed an action\n\n"
        f"Understand the user's GOAL from these actions and create a simple automation.\n"
        f"ONLY use: chrome_profile_picker, chrome_open_url, open_url, wait, run_command, type, key_press, hotkey.\n"
        f"DO NOT use playwright actions. Build URLs directly with search queries encoded in them."
    )


def generate_automation_from_task(ctx: TaskContext) -> tuple[str, list[dict]]:
    user_msg = _build_task_prompt(ctx)
    _LOGGER.info("Asking LLM to understand task '%s' (%d raw actions)", ctx.task_name, len(ctx.raw_actions))

    raw = _call_llm(system=SYSTEM_PROMPT, user=user_msg)
    _LOGGER.info("LLM response length: %d", len(raw))

    parsed = _parse_json(raw)
    if not isinstance(parsed, dict):
        raise RuntimeError("LLM did not return a JSON object")

    intent = parsed.get("intent", "")
    steps = parsed.get("steps", [])
    if not isinstance(steps, list) or not steps:
        raise RuntimeError("LLM returned no steps")

    # Safety net: convert any Playwright actions
    steps = _sanitize_steps(steps)

    _LOGGER.info("LLM intent: %s | %d steps", intent, len(steps))
    return intent, steps


def correct_failed_step(
    *,
    intent: str,
    failed_step: dict,
    error_message: str,
    all_steps: list[dict],
) -> dict:
    prompt = CORRECTION_PROMPT.format(
        step_order=failed_step.get("step_order", "?"),
        intent=intent,
        step_json=json.dumps(failed_step, indent=2),
        error_message=error_message,
        all_steps_json=json.dumps(all_steps, indent=2),
    )
    raw = _call_llm(system=SYSTEM_PROMPT, user=prompt, temperature=0.1)

    parsed = _parse_json(raw)
    if not isinstance(parsed, dict):
        raise RuntimeError("LLM correction did not return a JSON object")

    # Safety net on corrections too
    return _sanitize_step(parsed)


MAX_CORRECTION_ATTEMPTS = 3


def execute_with_llm(
    task_context: TaskContext,
    engine_execute_fn,
) -> Generator[ExecutionStepEvent | ExecutionDoneEvent, None, None]:
    _LOGGER.info(
        "Starting LLM execution for '%s' (%d raw actions)",
        task_context.task_name, len(task_context.raw_actions),
    )

    try:
        intent, executable_steps = generate_automation_from_task(task_context)
    except Exception as e:
        _LOGGER.error("LLM automation generation failed: %s", e)
        yield ExecutionDoneEvent(
            status="failed", total_steps=0, completed_steps=0,
            error=f"AI failed to create automation: {e}",
        )
        return

    yield ExecutionStepEvent(
        step_order=0,
        description=f"AI understood: {intent}",
        action_type="planning",
        target="", value="",
        status="success", attempts=0,
        llm_reasoning=intent,
    )

    completed = 0
    for i, step in enumerate(executable_steps):
        step_order = step.get("step_order", i + 1)
        desc = step.get("description", f"Step {step_order}")
        reasoning = step.get("reasoning", "")

        yield ExecutionStepEvent(
            step_order=step_order, description=desc,
            action_type=step.get("action_type", "noop"),
            target=step.get("target", ""), value=step.get("value", ""),
            status="running", attempts=0, llm_reasoning=reasoning,
        )

        current_step = dict(step)
        ok = False
        last_error = ""
        attempts = 0

        for attempt in range(1, MAX_CORRECTION_ATTEMPTS + 1):
            attempts = attempt
            try:
                ok, last_error = engine_execute_fn(current_step)
            except Exception as exc:
                ok, last_error = False, str(exc)

            if ok:
                break

            _LOGGER.warning("Step %d failed (attempt %d/%d): %s", step_order, attempt, MAX_CORRECTION_ATTEMPTS, last_error)

            if attempt < MAX_CORRECTION_ATTEMPTS:
                yield ExecutionStepEvent(
                    step_order=step_order, description=desc,
                    action_type=current_step.get("action_type", "noop"),
                    target=current_step.get("target", ""), value=current_step.get("value", ""),
                    status="corrected", attempts=attempt, error=last_error,
                    llm_reasoning=f"Failed, asking AI to fix...",
                )
                try:
                    corrected = correct_failed_step(
                        intent=intent, failed_step=current_step,
                        error_message=last_error, all_steps=executable_steps,
                    )
                    current_step = {**current_step, **corrected}
                    desc = corrected.get("description", desc)
                    reasoning = corrected.get("reasoning", "")
                except Exception as ce:
                    _LOGGER.error("LLM correction failed: %s", ce)
                    last_error = f"{last_error} (AI correction failed: {ce})"
                    break

        final_status: StepStatus = "success" if ok else "failed"
        yield ExecutionStepEvent(
            step_order=step_order, description=desc,
            action_type=current_step.get("action_type", "noop"),
            target=current_step.get("target", ""), value=current_step.get("value", ""),
            status=final_status, attempts=attempts,
            error="" if ok else last_error, llm_reasoning=reasoning,
        )

        if ok:
            completed += 1
        else:
            yield ExecutionDoneEvent(
                status="failed", total_steps=len(executable_steps),
                completed_steps=completed,
                error=f"Step {step_order} failed after {attempts} attempts: {last_error}",
            )
            return

    yield ExecutionDoneEvent(
        status="success",
        total_steps=len(executable_steps),
        completed_steps=completed,
    )


def execute_cached_steps(
    cached_steps: list[dict],
    engine_execute_fn,
) -> Generator[ExecutionStepEvent | ExecutionDoneEvent, None, None]:
    """Re-run previously successful steps from cache. No LLM calls needed."""
    _LOGGER.info("Executing %d cached steps (no LLM call)", len(cached_steps))

    yield ExecutionStepEvent(
        step_order=0,
        description="Using cached automation plan (previously successful)",
        action_type="planning",
        target="", value="",
        status="success", attempts=0,
        llm_reasoning="Reusing steps from last successful run — no LLM call needed",
    )

    completed = 0
    for i, step in enumerate(cached_steps):
        step_order = step.get("step_order", i + 1)
        desc = step.get("description", f"Step {step_order}")

        yield ExecutionStepEvent(
            step_order=step_order, description=desc,
            action_type=step.get("action_type", "noop"),
            target=step.get("target", ""), value=step.get("value", ""),
            status="running", attempts=0,
            llm_reasoning="cached",
        )

        try:
            ok, err = engine_execute_fn(step)
        except Exception as exc:
            ok, err = False, str(exc)

        yield ExecutionStepEvent(
            step_order=step_order, description=desc,
            action_type=step.get("action_type", "noop"),
            target=step.get("target", ""), value=step.get("value", ""),
            status="success" if ok else "failed", attempts=1,
            error="" if ok else err,
            llm_reasoning="cached",
        )

        if ok:
            completed += 1
        else:
            yield ExecutionDoneEvent(
                status="failed", total_steps=len(cached_steps),
                completed_steps=completed,
                error=f"Cached step {step_order} failed: {err}",
            )
            return

    yield ExecutionDoneEvent(
        status="success",
        total_steps=len(cached_steps),
        completed_steps=completed,
    )
