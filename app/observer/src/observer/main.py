from __future__ import annotations

import logging
import signal
import sys
import time

from observer.collector import ObserverService
from observer.config import get_config


def _setup_logging() -> None:
    logging.basicConfig(
        level=logging.INFO,
        format="%(asctime)s [%(levelname)s] %(name)s: %(message)s",
    )


def run_forever() -> int:
    _setup_logging()
    log = logging.getLogger("observer.main")

    cfg = get_config()
    log.info("Observer starting (backend=%s, endpoint=%s)", cfg.backend_base_url, cfg.logs_endpoint)

    service = ObserverService(cfg)
    handle = service.start()

    stop_signals = {signal.SIGINT, signal.SIGTERM}

    def _shutdown() -> None:
        log.info("Shutting down observer...")
        handle.stop_fn()

    for sig in stop_signals:
        try:
            signal.signal(sig, lambda *_: _shutdown())
        except Exception:
            # Signal wiring may fail on some platforms.
            pass

    try:
        while True:
            time.sleep(1)
    except KeyboardInterrupt:
        _shutdown()
    return 0


if __name__ == "__main__":
    sys.exit(run_forever())

