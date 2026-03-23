from __future__ import annotations

import uvicorn

from .config import Config
from .server import create_app


def main() -> None:
    cfg = Config.from_env()
    app = create_app(cfg)
    uvicorn.run(
        app,
        host="127.0.0.1",
        port=cfg.mcp_port,
        log_level="debug" if cfg.debug else "info",
    )


if __name__ == "__main__":
    main()

