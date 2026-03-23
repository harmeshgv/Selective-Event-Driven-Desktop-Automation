from __future__ import annotations

import socket
import threading
import time
import webbrowser

import uvicorn

from .config import Config
from .server import create_app


def _port_open(host: str, port: int) -> bool:
    try:
        with socket.create_connection((host, port), timeout=0.4):
            return True
    except OSError:
        return False


def main() -> None:
    cfg = Config.from_env()
    host = "127.0.0.1"
    url = f"http://{host}:{cfg.mcp_port}/dashboard"

    # If another instance is already running, just open the dashboard.
    if _port_open(host, cfg.mcp_port):
        webbrowser.open(url)
        return

    app = create_app(cfg)
    server = uvicorn.Server(
        uvicorn.Config(
            app,
            host=host,
            port=cfg.mcp_port,
            log_level="debug" if cfg.debug else "info",
        )
    )

    t = threading.Thread(target=server.run, name="seda-uvicorn", daemon=True)
    t.start()

    # Wait briefly for bind then open browser.
    for _ in range(25):
        if _port_open(host, cfg.mcp_port):
            break
        time.sleep(0.2)

    webbrowser.open(url)

    # Keep process alive while server runs.
    while t.is_alive():
        time.sleep(0.5)


if __name__ == "__main__":
    main()

