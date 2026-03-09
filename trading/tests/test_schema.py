"""Tests for trading.db.schema — DB setup, WAL mode, table creation."""
from __future__ import annotations

import sqlite3
import tempfile
from pathlib import Path

import pytest

from trading.db.schema import get_connection, setup


@pytest.fixture
def tmp_db(tmp_path):
    return tmp_path / "test.db"


def test_setup_creates_all_tables(tmp_db):
    setup(tmp_db)
    conn = sqlite3.connect(str(tmp_db))
    tables = {r[0] for r in conn.execute("SELECT name FROM sqlite_master WHERE type='table'").fetchall()}
    assert "finance_signals" in tables
    assert "market_data" in tables
    assert "signals" in tables
    conn.close()


def test_setup_is_idempotent(tmp_db):
    setup(tmp_db)
    setup(tmp_db)  # must not raise
    conn = sqlite3.connect(str(tmp_db))
    count = conn.execute("SELECT COUNT(*) FROM finance_signals").fetchone()[0]
    assert count == 0
    conn.close()


def test_wal_mode_enabled(tmp_db):
    conn = get_connection(tmp_db)
    mode = conn.execute("PRAGMA journal_mode").fetchone()[0]
    assert mode == "wal"
    conn.close()


def test_market_data_unique_constraint(tmp_db):
    setup(tmp_db)
    conn = get_connection(tmp_db)
    with conn:
        conn.execute(
            "INSERT INTO market_data (ticker, date, close) VALUES ('SPY', '2026-01-01', 500.0)"
        )
    # INSERT OR REPLACE should succeed (upsert)
    with conn:
        conn.execute(
            "INSERT OR REPLACE INTO market_data (ticker, date, close) VALUES ('SPY', '2026-01-01', 501.0)"
        )
    row = conn.execute(
        "SELECT close FROM market_data WHERE ticker='SPY' AND date='2026-01-01'"
    ).fetchone()
    assert row[0] == 501.0
    conn.close()


def test_signals_unique_constraint(tmp_db):
    setup(tmp_db)
    conn = get_connection(tmp_db)
    with conn:
        conn.execute(
            "INSERT INTO signals (signal_type, name, ticker, date, value) VALUES ('indicator','foo','SPY','2026-01-01',1.0)"
        )
    with conn:
        conn.execute(
            "INSERT OR REPLACE INTO signals (signal_type, name, ticker, date, value) VALUES ('indicator','foo','SPY','2026-01-01',2.0)"
        )
    row = conn.execute(
        "SELECT value FROM signals WHERE name='foo' AND date='2026-01-01'"
    ).fetchone()
    assert row[0] == 2.0
    conn.close()


def test_get_connection_creates_parent_dirs(tmp_path):
    nested = tmp_path / "a" / "b" / "c" / "test.db"
    conn = get_connection(nested)
    assert nested.exists()
    conn.close()
