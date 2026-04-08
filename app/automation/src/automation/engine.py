from __future__ import annotations

import time
import webbrowser
from dataclasses import dataclass
from typing import Iterable, Optional, Protocol

import pyautogui
from playwright.sync_api import sync_playwright


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
    # Accept selectors separated by comma or pipe for MVP.
    raw = target.replace("|", ",")
    parts = [p.strip() for p in raw.split(",") if p.strip()]
    return parts


def _parse_click_coords(target: str) -> Optional[tuple[int, int]]:
    # MVP coordinate parsing: "x=123,y=456" or "coords(123,456)".
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


def _execute_playwright_steps(steps: Iterable[StepLike]) -> list[StepExecutionResult]:
    results: list[StepExecutionResult] = []

    requires_headful = True
    headless = False
    try:
        # Optional env toggle
        import os

        headless = os.getenv("PLAYWRIGHT_HEADLESS", "false").lower() == "true"
        requires_headful = not headless
    except Exception:
        pass

    with sync_playwright() as p:
        browser = p.chromium.launch(headless=headless)
        context = browser.new_context()
        page = context.new_page()

        for step in steps:
            attempts = 0
            last_err = ""
            ok = False

            while attempts < max(1, step.retry_count):
                attempts += 1
                try:
                    # Navigate / fill / press
                    if step.action_type == "playwright_navigate":
                        page.goto(step.target, timeout=30000, wait_until="load")
                        ok = True
                    elif step.action_type == "playwright_fill":
                        page.locator(step.target).wait_for(timeout=5000)
                        page.fill(step.target, step.value)
                        ok = True
                    elif step.action_type == "playwright_press":
                        page.locator(step.target).wait_for(timeout=5000)
                        page.press(step.target, step.value)
                        ok = True
                    elif step.action_type == "playwright_click_first_result":
                        selectors = _parse_selectors(step.target)
                        clicked = False
                        last_sel_err = ""
                        for sel in selectors:
                            try:
                                loc = page.locator(sel)
                                if loc.count() == 0:
                                    continue
                                loc.first.click(timeout=5000)
                                clicked = True
                                break
                            except Exception as e:
                                last_sel_err = str(e)
                                continue
                        if not clicked:
                            raise RuntimeError(f"Could not click any selector: {selectors}. Last error: {last_sel_err}")
                        ok = True
                    elif step.action_type == "noop":
                        ok = True
                    else:
                        raise RuntimeError(f"Unsupported action_type for Playwright: {step.action_type}")
                except Exception as e:
                    last_err = str(e)
                    ok = False
                    time.sleep(1.0)

                if ok:
                    break

            results.append(
                StepExecutionResult(
                    step_order=step.step_order,
                    description=step.description,
                    status="success" if ok else "failed",
                    attempts=attempts,
                    error="" if ok else last_err,
                )
            )
            if not ok:
                break

        try:
            context.close()
        finally:
            browser.close()

    return results


def _execute_pyautogui_steps(steps: Iterable[StepLike]) -> list[StepExecutionResult]:
    results: list[StepExecutionResult] = []
    for step in steps:
        attempts = 0
        last_err = ""
        ok = False

        while attempts < max(1, step.retry_count):
            attempts += 1
            try:
                if step.action_type == "type":
                    pyautogui.typewrite(step.value)
                elif step.action_type == "click":
                    coords = _parse_click_coords(step.target)
                    if coords is None:
                        raise RuntimeError("click requires coordinate target like x=10,y=20")
                    x, y = coords
                    pyautogui.click(x=x, y=y)
                elif step.action_type == "key_press":
                    # Planner uses target like "KEY:<ENTER>".
                    key_tok = (step.target or "").replace("KEY:", "")
                    key_tok = key_tok.replace("<", "").replace(">", "")
                    key_name = key_tok.strip().lower()
                    if not key_name:
                        raise RuntimeError("key_press requires a key token in target")
                    pyautogui.press(key_name)
                elif step.action_type == "open_url":
                    webbrowser.open_new_tab(step.value or step.target)
                elif step.action_type == "move":
                    # MVP: no reliable UI coordinates/selectors from logs yet.
                    pass
                elif step.action_type == "noop":
                    pass
                else:
                    raise RuntimeError(f"Unsupported action_type for pyautogui: {step.action_type}")

                ok = True
            except Exception as e:
                last_err = str(e)
                ok = False
                time.sleep(0.7)

            if ok:
                break

        results.append(
            StepExecutionResult(
                step_order=step.step_order,
                description=step.description,
                status="success" if ok else "failed",
                attempts=attempts,
                error="" if ok else last_err,
            )
        )
        if not ok:
            break

    return results


def execute_automation(steps: list[StepLike]) -> list[StepExecutionResult]:
    # MVP strategy: if any Playwright action appears, run the whole plan in a Playwright context.
    playwright_action_types = {"playwright_navigate", "playwright_fill", "playwright_press", "playwright_click_first_result"}
    if any(s.action_type in playwright_action_types for s in steps):
        return _execute_playwright_steps(steps)
    return _execute_pyautogui_steps(steps)

