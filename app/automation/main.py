from __future__ import annotations

import sys
from pathlib import Path


def _add_src_to_path() -> None:
    src_dir = Path(__file__).resolve().parent / "src"
    sys.path.insert(0, str(src_dir))


_add_src_to_path()

