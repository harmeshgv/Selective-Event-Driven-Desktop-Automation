from __future__ import annotations

import logging
import os
import shutil
import subprocess
import time
import webbrowser
from dataclasses import dataclass
from typing import Iterable, Optional, Protocol

import pyautogui

_log = logging.getLogger("automation.engine")

# Auto-detect Chrome path
_CHROME_CANDIDATES = [
    r"C:\Program Files\Google\Chrome\Application\chrome.exe",
    r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
    os.path.expandvars(r"%LOCALAPPDATA%\Google\Chrome\Application\chrome.exe"),
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    "google-chrome",
    "google-chrome-stable",
    "chromium-browser",
    "chromium",
]


def _find_chrome() -> str:
    env_path = os.getenv("CHROME_PATH", "").strip()
    if env_path and os.path.isfile(env_path):
        return env_path
    for candidate in _CHROME_CANDIDATES:
        if os.path.isfile(candidate):
            return candidate
        found = shutil.which(candidate)
        if found:
            return found
    return ""


class StepLike(Protocol):
    step_order: int
    description: str
    action_type: str
    target: str
    value: str
    retry_count: int


@dataclass(frozen=True)
class StepExecutionResult:
    step_order: int
    description: str
    status: str  # success|failed
    attempts: int
    error: str = ""


def _parse_selectors(target: str) -> list[str]:
    raw = target.replace("|", ",")
    parts = [p.strip() for p in raw.split(",") if p.strip()]
    return parts


def _parse_click_coords(target: str) -> Optional[tuple[int, int]]:
    t = target.replace(" ", "")
    if t.startswith("coords(") and t.endswith(")"):
        inner = t[len("coords(") : -1]
        x_s, y_s = inner.split(",", 1)
        return int(float(x_s)), int(float(y_s))
    if t.startswith("x=") and "y=" in t:
        x_part, y_part = t.split(",y=", 1) if ",y=" in t else t.split("y=", 1)
        x_s = x_part[len("x=") :]
        y_s = y_part
        return int(float(x_s)), int(float(y_s))
    return None


# ---------------------------------------------------------------------------
# Subprocess / Chrome actions — simple, reliable, actually works
# ---------------------------------------------------------------------------

def _exec_chrome_profile_picker() -> None:
    chrome = _find_chrome()
    if not chrome:
        raise RuntimeError("Chrome not found. Set CHROME_PATH env var.")
    subprocess.Popen([chrome, "--profile-picker"])


def _exec_chrome_open_url(url: str, profile: str = "") -> None:
    chrome = _find_chrome()
    if not chrome:
        raise RuntimeError("Chrome not found. Set CHROME_PATH env var.")
    args = [chrome]
    if profile:
        args.append(f"--profile-directory={profile}")
    args.append(url)
    subprocess.Popen(args)


def _exec_run_command(command: str) -> None:
    subprocess.Popen(command, shell=True)


def _exec_wait(seconds: float) -> None:
    time.sleep(seconds)


# ---------------------------------------------------------------------------
# Single-step executor — handles ALL action types in one place
# ---------------------------------------------------------------------------

def _execute_single_step(step: StepLike) -> StepExecutionResult:
    attempts = 0
    last_err = ""
    ok = False

    while attempts < max(1, step.retry_count):
        attempts += 1
        try:
            at = step.action_type

            # --- Subprocess / Chrome actions (most reliable) ---
            if at == "chrome_profile_picker":
                _exec_chrome_profile_picker()
                ok = True

            elif at == "chrome_open_url":
                url = step.target or step.value
                profile = step.value if step.target else ""
                if step.target.startswith("http"):
                    url = step.target
                    profile = step.value
                _exec_chrome_open_url(url, profile)
                ok = True

            elif at == "open_url":
                url = step.target or step.value
                if _find_chrome():
                    _exec_chrome_open_url(url)
                else:
                    webbrowser.open_new_tab(url)
                ok = True

            elif at == "run_command":
                cmd = step.value or step.target
                if not cmd:
                    raise RuntimeError("run_command requires a command in target or value")
                _exec_run_command(cmd)
                ok = True

            elif at == "wait":
                secs = 3.0
                try:
                    secs = float(step.value or step.target or "3")
                except ValueError:
                    pass
                _exec_wait(secs)
                ok = True

            # --- pyautogui actions ---
            elif at == "type":
                pyautogui.typewrite(step.value, interval=0.02)
                ok = True

            elif at == "click":
                coords = _parse_click_coords(step.target)
                if coords is None:
                    raise RuntimeError("click requires coordinate target like x=10,y=20")
                x, y = coords
                pyautogui.click(x=x, y=y)
                ok = True

            elif at == "key_press":
                key_tok = (step.target or "").replace("KEY:", "")
                key_tok = key_tok.replace("<", "").replace(">", "")
                key_name = key_tok.strip().lower()
                if not key_name:
                    raise RuntimeError("key_press requires a key token in target")
                pyautogui.press(key_name)
                ok = True

            elif at == "hotkey":
                keys = [k.strip().lower() for k in (step.value or step.target or "").split("+") if k.strip()]
                if not keys:
                    raise RuntimeError("hotkey requires keys like 'ctrl+t' in value")
                pyautogui.hotkey(*keys)
                ok = True

            # --- Playwright actions (kept for browser-internal automation) ---
            elif at == "playwright_navigate":
                from playwright.sync_api import sync_playwright
                with sync_playwright() as p:
                    browser = p.chromium.launch(headless=False)
                    page = browser.new_page()
                    page.goto(step.target, timeout=30000, wait_until="load")
                    browser.close()
                ok = True

            elif at in ("noop", "planning"):
                ok = True

            elif at == "move":
                ok = True

            else:
                raise RuntimeError(f"Unknown action_type: {at}")

        except Exception as e:
            last_err = str(e)
            ok = False
            time.sleep(0.7)

        if ok:
            break

    return StepExecutionResult(
        step_order=step.step_order,
        description=step.description,
        status="success" if ok else "failed",
        attempts=attempts,
        error="" if ok else last_err,
    )


def execute_automation(steps: list[StepLike]) -> list[StepExecutionResult]:
    results: list[StepExecutionResult] = []
    for step in steps:
        result = _execute_single_step(step)
        results.append(result)
        if result.status == "failed":
            break
    return results

