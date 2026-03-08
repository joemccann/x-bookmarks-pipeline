"""
Centralized configuration — all defaults in one place, overridable via env vars.
"""
from __future__ import annotations

import os


def _env(key: str, default: str) -> str:
    return os.environ.get(key, default)


def _env_int(key: str, default: int) -> int:
    val = os.environ.get(key)
    return int(val) if val else default


def _env_float(key: str, default: float) -> float:
    val = os.environ.get(key)
    return float(val) if val else default


# ---------------------------------------------------------------------------
# Models (override via env vars or CLI)
# ---------------------------------------------------------------------------
CEREBRAS_MODEL = _env("CEREBRAS_MODEL", "qwen-3-235b-a22b-instruct-2507")
XAI_MODEL = _env("XAI_MODEL", "grok-4-0709")
ANTHROPIC_MODEL = _env("ANTHROPIC_MODEL", "claude-opus-4-6")
OPENAI_MODEL = _env("OPENAI_MODEL", "gpt-5.4")

# ---------------------------------------------------------------------------
# Timeouts (seconds)
# ---------------------------------------------------------------------------
API_TIMEOUT = _env_float("API_TIMEOUT", 120.0)
VISION_TIMEOUT = _env_float("VISION_TIMEOUT", 60.0)
FETCH_TIMEOUT = _env_float("FETCH_TIMEOUT", 30.0)

# ---------------------------------------------------------------------------
# Pipeline defaults
# ---------------------------------------------------------------------------
MAX_WORKERS = _env_int("MAX_WORKERS", 5)
OUTPUT_DIR = _env("OUTPUT_DIR", "output")
CACHE_PATH = _env("CACHE_PATH", "cache/bookmarks.db")
DEFAULT_TICKER = _env("DEFAULT_TICKER", "BTCUSDT")
DEFAULT_TIMEFRAME = _env("DEFAULT_TIMEFRAME", "D")
