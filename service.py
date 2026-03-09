#!/usr/bin/env python3
"""
X Bookmarks Pipeline — Background Service

Periodically polls X API for new bookmarks and processes them through
the multi-LLM pipeline (classify -> vision -> plan -> generate Pine Script).

Designed to run standalone (python service.py) or via launchd.

Configuration (env vars):
    POLL_INTERVAL       — Seconds between polls (default: 900 = 15 min)
    POLL_MAX_RESULTS    — Max bookmarks per poll (default: 20)
    SERVICE_LOG_FILE    — Log file path (default: ~/.local/log/x-bookmarks-pipeline.log)

All other env vars (API keys, model overrides, etc.) are loaded from the
project's .env file.
"""
from __future__ import annotations

import hashlib
import logging
import os
import signal
import sys
import time
from datetime import datetime, timezone
from pathlib import Path
from typing import NoReturn

# Load .env from the project directory before any src imports
from dotenv import load_dotenv

_PROJECT_DIR = Path(__file__).resolve().parent
load_dotenv(_PROJECT_DIR / ".env")

from src.pipeline import MultiLLMPipeline, PipelineResult
from src.fetchers.x_bookmark_fetcher import XBookmarkFetcher, FetchError
from src.config import OUTPUT_DIR, MAX_WORKERS

# Trading engine hook — indexes each saved .meta.json into signals.db immediately.
# Imported lazily so service.py works even if trading/ deps aren't installed.
def _make_index_hook():
    try:
        import sys as _sys
        _sys.path.insert(0, str(_PROJECT_DIR / "trading"))
        from trading.indexer import upsert_one
        return upsert_one
    except Exception:
        return None

# ---------------------------------------------------------------------------
# Configuration
# ---------------------------------------------------------------------------

POLL_INTERVAL = int(os.environ.get("POLL_INTERVAL", "900"))
POLL_MAX_RESULTS = int(os.environ.get("POLL_MAX_RESULTS", "20"))
LOG_FILE = os.environ.get(
    "SERVICE_LOG_FILE",
    str(Path.home() / ".local" / "log" / "x-bookmarks-pipeline.log"),
)

# ---------------------------------------------------------------------------
# Logging
# ---------------------------------------------------------------------------


def _setup_logging() -> logging.Logger:
    """Configure file + stderr logging with timestamps."""
    log_path = Path(LOG_FILE)
    log_path.parent.mkdir(parents=True, exist_ok=True)

    logger = logging.getLogger("x-bookmarks-pipeline")
    logger.setLevel(logging.INFO)

    fmt = logging.Formatter(
        "%(asctime)s [%(levelname)s] %(message)s",
        datefmt="%Y-%m-%d %H:%M:%S",
    )

    # File handler (append mode)
    fh = logging.FileHandler(str(log_path), encoding="utf-8")
    fh.setFormatter(fmt)
    logger.addHandler(fh)

    # Stderr handler (so launchd captures output too)
    sh = logging.StreamHandler(sys.stderr)
    sh.setFormatter(fmt)
    logger.addHandler(sh)

    return logger


# ---------------------------------------------------------------------------
# Shutdown handling
# ---------------------------------------------------------------------------

_shutdown_requested = False


def _handle_signal(signum: int, _frame) -> None:
    """Handle SIGTERM / SIGINT gracefully."""
    global _shutdown_requested
    sig_name = signal.Signals(signum).name
    log.info("Received %s — shutting down after current cycle", sig_name)
    _shutdown_requested = True


# ---------------------------------------------------------------------------
# Poll cycle
# ---------------------------------------------------------------------------


def _make_tweet_id(text: str, author: str = "") -> str:
    """Generate a deterministic tweet ID from text content."""
    return hashlib.sha256(f"{author}:{text}".encode()).hexdigest()[:16]


def poll_once(
    pipeline: MultiLLMPipeline,
    max_results: int = 20,
) -> dict:
    """
    Run a single fetch-and-process cycle.

    Returns a summary dict with counts:
        fetched, new, cached, finance, categorized, failed
    """
    stats = {
        "fetched": 0,
        "new": 0,
        "cached": 0,
        "finance": 0,
        "categorized": 0,
        "failed": 0,
    }

    # --- Fetch bookmarks ---
    try:
        fetcher = XBookmarkFetcher()
    except ValueError as e:
        log.error("Cannot create fetcher: %s", e)
        stats["failed"] = 1
        return stats

    try:
        bookmarks = fetcher.fetch(max_results=max_results)
    except FetchError as e:
        log.error("Fetch failed: %s", e)
        stats["failed"] = 1
        return stats
    except Exception as e:
        log.error("Unexpected fetch error: %s", e, exc_info=True)
        stats["failed"] = 1
        return stats

    stats["fetched"] = len(bookmarks)

    if not bookmarks:
        log.info("No bookmarks returned")
        return stats

    # --- Process each bookmark ---
    for bm in bookmarks:
        if _shutdown_requested:
            log.info("Shutdown requested — stopping bookmark processing")
            break

        tweet_id = getattr(bm, "tweet_id", None) or _make_tweet_id(bm.text, bm.author)
        tweet_url = f"https://x.com/{bm.author}/status/{tweet_id}" if bm.author else ""

        # Skip already-completed bookmarks (fast path)
        if pipeline.cache and pipeline.cache.has_completed(tweet_id):
            stats["cached"] += 1
            continue

        stats["new"] += 1
        log.info(
            "Processing @%s [%s]: %s",
            bm.author or "unknown",
            tweet_id,
            bm.text[:100].replace("\n", " "),
        )

        try:
            t0 = time.time()
            result = pipeline.run(
                tweet_id=tweet_id,
                tweet_text=bm.text,
                image_urls=getattr(bm, "media_urls", []),
                author=bm.author,
                tweet_date=bm.date,
                tweet_url=tweet_url,
                save=True,
            )
            elapsed = time.time() - t0

            if result.error:
                log.warning(
                    "  Pipeline error for %s (%.1fs): %s",
                    tweet_id, elapsed, result.error,
                )
                stats["failed"] += 1
            elif result.classification and result.classification.is_finance:
                valid = result.validation and result.validation.valid
                log.info(
                    "  Finance: %s/%s [%s] — %s (%.1fs)",
                    result.classification.category,
                    result.classification.subcategory,
                    "VALID" if valid else "INVALID",
                    result.plan.title if result.plan else "no plan",
                    elapsed,
                )
                if valid:
                    stats["finance"] += 1
                else:
                    stats["failed"] += 1
            else:
                cat = ""
                if result.classification:
                    cat = f"{result.classification.category}/{result.classification.subcategory}"
                log.info("  Categorized: %s (%.1fs)", cat, elapsed)
                stats["categorized"] += 1

            if result.output_path:
                log.info("  Saved: %s", result.output_path)
            if result.meta_path:
                log.info("  Meta:  %s", result.meta_path)

        except Exception as e:
            log.error(
                "  Unhandled error processing %s: %s",
                tweet_id, e, exc_info=True,
            )
            stats["failed"] += 1

    return stats


# ---------------------------------------------------------------------------
# Main loop
# ---------------------------------------------------------------------------


def run_daemon(
    interval: int = POLL_INTERVAL,
    max_results: int = POLL_MAX_RESULTS,
) -> NoReturn:
    """
    Run the polling loop forever (or until SIGTERM/SIGINT).
    """
    # Register signal handlers (safe to call whether invoked directly or via main.py)
    signal.signal(signal.SIGTERM, _handle_signal)
    signal.signal(signal.SIGINT, _handle_signal)

    log.info(
        "Starting daemon — poll every %ds, max %d bookmarks/poll, pid=%d",
        interval, max_results, os.getpid(),
    )
    log.info("Project dir: %s", _PROJECT_DIR)
    log.info("Output dir:  %s", OUTPUT_DIR)
    log.info("Log file:    %s", LOG_FILE)

    pipeline = MultiLLMPipeline(
        output_dir=OUTPUT_DIR,
        cache_enabled=True,
        vision_enabled=True,
        on_meta_saved=_make_index_hook(),
    )

    cycle = 0
    while not _shutdown_requested:
        cycle += 1
        ts = datetime.now(timezone.utc).strftime("%Y-%m-%d %H:%M:%S UTC")
        log.info("--- Poll cycle %d at %s ---", cycle, ts)

        t0 = time.time()
        stats = poll_once(pipeline, max_results=max_results)
        elapsed = time.time() - t0

        log.info(
            "Cycle %d done in %.1fs — fetched=%d new=%d cached=%d "
            "finance=%d categorized=%d failed=%d",
            cycle, elapsed,
            stats["fetched"], stats["new"], stats["cached"],
            stats["finance"], stats["categorized"], stats["failed"],
        )

        if _shutdown_requested:
            break

        # Sleep in small increments so we can respond to signals quickly
        log.info("Sleeping %ds until next poll...", interval)
        sleep_until = time.time() + interval
        while time.time() < sleep_until and not _shutdown_requested:
            time.sleep(1)

    log.info("Daemon stopped (pid=%d)", os.getpid())
    sys.exit(0)


# ---------------------------------------------------------------------------
# Entrypoint
# ---------------------------------------------------------------------------

# Set up logging before anything else can use it
log = _setup_logging()

if __name__ == "__main__":
    # Register signal handlers
    signal.signal(signal.SIGTERM, _handle_signal)
    signal.signal(signal.SIGINT, _handle_signal)

    # Allow CLI override of interval and max_results
    import argparse

    parser = argparse.ArgumentParser(description="X Bookmarks Pipeline daemon")
    parser.add_argument(
        "--interval", type=int, default=POLL_INTERVAL,
        help=f"Seconds between polls (default: {POLL_INTERVAL})",
    )
    parser.add_argument(
        "--max-results", type=int, default=POLL_MAX_RESULTS,
        help=f"Max bookmarks per poll (default: {POLL_MAX_RESULTS})",
    )
    args = parser.parse_args()

    run_daemon(interval=args.interval, max_results=args.max_results)
