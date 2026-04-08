from __future__ import annotations

import sys
from pathlib import Path


def _add_src_to_path() -> None:
    src_dir = Path(__file__).resolve().parent / "src"
    sys.path.insert(0, str(src_dir))
    # Allow importing the sibling `app/ai/src` package without installing.
    repo_root = Path(__file__).resolve().parent.parent
    ai_src = repo_root / "ai" / "src"
    if ai_src.exists():
        sys.path.insert(0, str(ai_src))
    # Allow importing the sibling `app/automation/src` package without installing.
    automation_src = repo_root / "automation" / "src"
    if automation_src.exists():
        sys.path.insert(0, str(automation_src))


_add_src_to_path()

from backend.main import app  # type: ignore  # noqa: E402

