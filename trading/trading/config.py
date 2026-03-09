"""
Trading engine configuration — all paths and defaults in one place.
Override via environment variables.
"""
from __future__ import annotations

import os
from pathlib import Path

# ---------------------------------------------------------------------------
# Base paths
# ---------------------------------------------------------------------------

# When running inside the monorepo, PROJECT_ROOT is two levels up from here.
# When extracted to its own repo, set TRADING_PROJECT_ROOT env var.
_HERE = Path(__file__).resolve().parent.parent  # trading/ dir
_REPO_ROOT = _HERE.parent                       # x-bookmarks-pipeline/

def _env(key: str, default: str) -> str:
    return os.environ.get(key, default)

# ---------------------------------------------------------------------------
# Database paths
# ---------------------------------------------------------------------------

# signals.db — owned by trading engine (market data + emitted signals + finance_signals index)
SIGNALS_DB_PATH = Path(_env(
    "SIGNALS_DB_PATH",
    str(_REPO_ROOT / "cache" / "signals.db"),
))

# bookmarks.db — owned by pipeline, opened READ-ONLY by trading engine
BOOKMARKS_DB_PATH = Path(_env(
    "BOOKMARKS_DB_PATH",
    str(_REPO_ROOT / "cache" / "bookmarks.db"),
))

# ---------------------------------------------------------------------------
# Pipeline output directory (where .meta.json and .pine files live)
# ---------------------------------------------------------------------------
FINANCE_OUTPUT_DIR = Path(_env(
    "FINANCE_OUTPUT_DIR",
    str(_REPO_ROOT / "output" / "finance"),
))

# ---------------------------------------------------------------------------
# Market data defaults
# ---------------------------------------------------------------------------

# Default tickers fetched for indicator/strategy computation
DEFAULT_TICKERS = _env("DEFAULT_TICKERS", "^VIX,^VVIX,^MOVE,SPY,PSP").split(",")

# How many calendar days back to fetch on first run
MARKET_DATA_LOOKBACK_DAYS = int(_env("MARKET_DATA_LOOKBACK_DAYS", "365"))
