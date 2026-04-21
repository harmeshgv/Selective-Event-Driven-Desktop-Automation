from __future__ import annotations

import atexit
import logging
import os
import subprocess
import sys
import threading
from pathlib import Path


def _add_src_to_path() -> None:
    src_dir = Path(__file__).resolve().parent / "src"
    sys.path.insert(0, str(src_dir))
    repo_root = Path(__file__).resolve().parent.parent
    ai_src = repo_root / "ai" / "src"
    if ai_src.exists():
        sys.path.insert(0, str(ai_src))
    automation_src = repo_root / "automation" / "src"
    if automation_src.exists():
        sys.path.insert(0, str(automation_src))
    observer_src = repo_root / "observer" / "src"
    if observer_src.exists():
        sys.path.insert(0, str(observer_src))


_add_src_to_path()

from backend.main import app  # type: ignore  # noqa: E402

_log = logging.getLogger("flowpilot.launcher")

_observer_process = None
_observer_thread = None
_observer_stop_fn = None


def _start_observer_inprocess() -> None:
    """Start the observer directly in-process (used when running as packaged exe)."""
    global _observer_thread, _observer_stop_fn
    try:
        from observer.config import get_config
        from observer.collector import ObserverService

        cfg = get_config()
        service = ObserverService(cfg)
        handle = service.start()
        _observer_stop_fn = handle.stop_fn
        _log.info("Observer started in-process")
    except Exception as exc:
        _log.warning("Failed to start in-process observer: %s", exc)


def _start_observer_subprocess() -> None:
    """Auto-launch the observer as a background subprocess (dev mode)."""
    global _observer_process
    observer_main = Path(__file__).resolve().parent.parent / "observer" / "main.py"
    if not observer_main.exists():
        return
    try:
        flags = 0
        if sys.platform == "win32":
            flags = subprocess.CREATE_NEW_CONSOLE
            si = subprocess.STARTUPINFO()
            si.dwFlags |= subprocess.STARTF_USESHOWWINDOW
            si.wShowWindow = 6  # SW_MINIMIZE
            _observer_process = subprocess.Popen(
                [sys.executable, str(observer_main)],
                cwd=str(observer_main.parent),
                creationflags=flags,
                startupinfo=si,
            )
        else:
            _observer_process = subprocess.Popen(
                [sys.executable, str(observer_main)],
                cwd=str(observer_main.parent),
            )
        _log.info("Observer started as subprocess (pid=%s)", _observer_process.pid)
    except Exception as exc:
        _log.warning("Failed to start observer subprocess: %s", exc)


def _start_observer() -> None:
    """Start the observer — in-process when frozen (packaged exe), subprocess in dev."""
    if getattr(sys, "frozen", False):
        _start_observer_inprocess()
    else:
        _start_observer_subprocess()


def _stop_observer() -> None:
    global _observer_process, _observer_stop_fn
    if _observer_stop_fn is not None:
        try:
            _observer_stop_fn()
        except Exception:
            pass
        _observer_stop_fn = None
    if _observer_process is not None:
        try:
            _observer_process.terminate()
        except Exception:
            pass
        _observer_process = None


if __name__ == "__main__":
    import uvicorn

    logging.basicConfig(level=logging.INFO)
    _start_observer()
    atexit.register(_stop_observer)

    port = int(os.getenv("BACKEND_PORT", "8000"))
    uvicorn.run(app, host="127.0.0.1", port=port, log_level="info")

