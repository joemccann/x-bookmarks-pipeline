"""
signals.db schema setup.

Three tables:
  finance_signals — indexed view of pipeline output (.meta.json + .pine files)
  market_data     — OHLCV from yfinance, keyed by (ticker, date)
  signals         — emitted signals from Python indicators/strategies
"""
from __future__ import annotations

import sqlite3
from pathlib import Path

from trading.config import SIGNALS_DB_PATH


def get_connection(path: Path = SIGNALS_DB_PATH) -> sqlite3.Connection:
    """Open signals.db with WAL mode and return connection."""
    path.parent.mkdir(parents=True, exist_ok=True)
    conn = sqlite3.connect(str(path))
    conn.row_factory = sqlite3.Row
    # WAL allows concurrent reads while writer is active — no lock contention
    conn.execute("PRAGMA journal_mode=WAL")
    conn.execute("PRAGMA synchronous=NORMAL")
    conn.execute("PRAGMA foreign_keys=ON")
    return conn


def setup(path: Path = SIGNALS_DB_PATH) -> None:
    """Create all tables if they don't exist. Safe to call repeatedly."""
    conn = get_connection(path)
    with conn:
        # ------------------------------------------------------------------
        # finance_signals — indexed from pipeline output files
        # Populated by trading.indexer.  Read-only for indicators/strategies.
        # ------------------------------------------------------------------
        conn.execute("""
            CREATE TABLE IF NOT EXISTS finance_signals (
                id                INTEGER PRIMARY KEY,
                tweet_id          TEXT UNIQUE NOT NULL,
                tweet_url         TEXT,
                author            TEXT,
                date              TEXT,           -- YYYY-MM-DD
                script_type       TEXT,           -- 'indicator' | 'strategy'
                category          TEXT,
                subcategory       TEXT,
                ticker            TEXT,
                direction         TEXT,           -- 'long' | 'short' | 'both' | null
                timeframe         TEXT,
                indicators_json   TEXT,           -- JSON array of indicator names
                key_levels_json   TEXT,           -- JSON object
                rationale         TEXT,
                pine_script       TEXT,           -- full .pine content (null if no .pine file)
                meta_json         TEXT,           -- full .meta.json content
                validation_passed INTEGER,        -- 1 | 0
                file_path         TEXT,           -- relative path to .meta.json
                indexed_at        TEXT DEFAULT (datetime('now')),
                updated_at        TEXT DEFAULT (datetime('now'))
            )
        """)
        conn.execute("""
            CREATE INDEX IF NOT EXISTS idx_fs_script_type ON finance_signals(script_type)
        """)
        conn.execute("""
            CREATE INDEX IF NOT EXISTS idx_fs_ticker ON finance_signals(ticker)
        """)
        conn.execute("""
            CREATE INDEX IF NOT EXISTS idx_fs_subcategory ON finance_signals(subcategory)
        """)

        # ------------------------------------------------------------------
        # market_data — OHLCV cache from yfinance
        # Written by trading.fetchers.market_data, read by indicators/strategies.
        # ------------------------------------------------------------------
        conn.execute("""
            CREATE TABLE IF NOT EXISTS market_data (
                id          INTEGER PRIMARY KEY,
                ticker      TEXT NOT NULL,
                date        TEXT NOT NULL,       -- ISO8601: 'YYYY-MM-DD'
                open        REAL,
                high        REAL,
                low         REAL,
                close       REAL,
                volume      REAL,
                fetched_at  TEXT DEFAULT (datetime('now')),
                UNIQUE(ticker, date)
            )
        """)
        conn.execute("""
            CREATE INDEX IF NOT EXISTS idx_md_ticker_date ON market_data(ticker, date)
        """)

        # ------------------------------------------------------------------
        # signals — emitted by Python indicators and strategies
        # Written by indicator/strategy run(); read by dashboards / consumers.
        # ------------------------------------------------------------------
        conn.execute("""
            CREATE TABLE IF NOT EXISTS signals (
                id              INTEGER PRIMARY KEY,
                signal_type     TEXT NOT NULL,   -- 'indicator' | 'strategy'
                name            TEXT NOT NULL,   -- e.g. 'move_psp_spread', 'vix_vvix_buy'
                ticker          TEXT,
                date            TEXT NOT NULL,   -- YYYY-MM-DD
                value           REAL,            -- numeric output (spread value, score, etc.)
                direction       TEXT,            -- 'long' | 'short' | 'flat' | null
                metadata_json   TEXT,            -- arbitrary JSON for extra context
                created_at      TEXT DEFAULT (datetime('now')),
                UNIQUE(name, ticker, date)       -- one signal per (name, ticker, date)
            )
        """)
        conn.execute("""
            CREATE INDEX IF NOT EXISTS idx_sig_name_date ON signals(name, date)
        """)
        conn.execute("""
            CREATE INDEX IF NOT EXISTS idx_sig_ticker ON signals(ticker)
        """)

    conn.close()
