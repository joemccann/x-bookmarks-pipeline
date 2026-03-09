"""Tests for trading.indicators.move_psp_spread."""
from __future__ import annotations

import json
from datetime import date, timedelta

import pytest

from trading.db.schema import setup, get_connection
from trading.indicators import move_psp_spread


@pytest.fixture
def tmp_db(tmp_path):
    db = tmp_path / "test.db"
    setup(db)
    return db


def _seed_market_data(db, ticker: str, closes: list[float], start: str = "2026-01-01"):
    conn = get_connection(db)
    d = date.fromisoformat(start)
    with conn:
        for i, close in enumerate(closes):
            day = (d + timedelta(days=i)).isoformat()
            conn.execute(
                "INSERT OR REPLACE INTO market_data (ticker, date, close) VALUES (?,?,?)",
                (ticker, day, close),
            )
    conn.close()


def test_run_emits_signals_for_overlapping_dates(tmp_db):
    _seed_market_data(tmp_db, "^MOVE", [100.0, 110.0, 120.0])
    _seed_market_data(tmp_db, "PSP",   [50.0,  55.0,  60.0])
    emitted = move_psp_spread.run(db_path=tmp_db, verbose=False)
    assert len(emitted) == 3


def test_run_calculates_spread_correctly(tmp_db):
    _seed_market_data(tmp_db, "^MOVE", [100.0])
    _seed_market_data(tmp_db, "PSP",   [40.0])
    emitted = move_psp_spread.run(db_path=tmp_db, verbose=False)
    assert len(emitted) == 1
    assert abs(emitted[0]["spread"] - 60.0) < 0.01


def test_run_writes_to_signals_table(tmp_db):
    _seed_market_data(tmp_db, "^MOVE", [100.0, 110.0])
    _seed_market_data(tmp_db, "PSP",   [50.0,  55.0])
    move_psp_spread.run(db_path=tmp_db, verbose=False)
    conn = get_connection(tmp_db)
    count = conn.execute(
        "SELECT COUNT(*) FROM signals WHERE name=?", (move_psp_spread.NAME,)
    ).fetchone()[0]
    assert count == 2
    conn.close()


def test_run_metadata_contains_move_psp_zscore(tmp_db):
    # Need >= 20 days for z-score to be non-null
    move_vals = [100.0 + i for i in range(25)]
    psp_vals  = [50.0  + i * 0.5 for i in range(25)]
    _seed_market_data(tmp_db, "^MOVE", move_vals)
    _seed_market_data(tmp_db, "PSP",   psp_vals)
    emitted = move_psp_spread.run(db_path=tmp_db, verbose=False)
    last = emitted[-1]
    assert "move" in last
    assert "psp" in last
    assert "zscore_90d" in last
    assert last["zscore_90d"] is not None


def test_run_returns_empty_when_no_move_data(tmp_db):
    _seed_market_data(tmp_db, "PSP", [50.0])
    result = move_psp_spread.run(db_path=tmp_db, verbose=False)
    assert result == []


def test_run_returns_empty_when_no_psp_data(tmp_db):
    _seed_market_data(tmp_db, "^MOVE", [100.0])
    result = move_psp_spread.run(db_path=tmp_db, verbose=False)
    assert result == []


def test_run_returns_empty_when_no_overlap(tmp_db):
    _seed_market_data(tmp_db, "^MOVE", [100.0], start="2025-01-01")
    _seed_market_data(tmp_db, "PSP",   [50.0],  start="2026-06-01")
    result = move_psp_spread.run(db_path=tmp_db, verbose=False)
    assert result == []


def test_run_upserts_signals(tmp_db):
    _seed_market_data(tmp_db, "^MOVE", [100.0])
    _seed_market_data(tmp_db, "PSP",   [50.0])
    move_psp_spread.run(db_path=tmp_db, verbose=False)
    move_psp_spread.run(db_path=tmp_db, verbose=False)  # second run upserts
    conn = get_connection(tmp_db)
    count = conn.execute(
        "SELECT COUNT(*) FROM signals WHERE name=?", (move_psp_spread.NAME,)
    ).fetchone()[0]
    assert count == 1  # UNIQUE constraint keeps only one row per date
    conn.close()
