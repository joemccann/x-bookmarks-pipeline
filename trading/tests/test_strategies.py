"""Tests for trading.strategies.vix_vvix_mean_reversion."""
from __future__ import annotations

import json
from datetime import date, timedelta

import pytest

from trading.db.schema import setup, get_connection
from trading.strategies import vix_vvix_mean_reversion as strat


@pytest.fixture
def tmp_db(tmp_path):
    db = tmp_path / "test.db"
    setup(db)
    return db


def _seed(db, ticker: str, values: list[float], col: str = "close", start: str = "2025-01-01"):
    conn = get_connection(db)
    d = date.fromisoformat(start)
    with conn:
        for i, v in enumerate(values):
            day = (d + timedelta(days=i)).isoformat()
            conn.execute(
                f"INSERT OR REPLACE INTO market_data (ticker, date, open, high, low, close, volume) "
                f"VALUES (?,?,?,?,?,?,?)",
                (ticker, day, v, v * 1.01, v * 0.99, v, 1_000_000),
            )
    conn.close()


def _seed_all(db, spy=500.0, vix=25.0, vvix=100.0, days=30):
    _seed(db, "SPY",   [spy  + i * 0.1 for i in range(days)])
    _seed(db, "^VIX",  [vix  + i * 0.0 for i in range(days)])
    _seed(db, "^VVIX", [vvix + i * 0.0 for i in range(days)])


def test_run_returns_signals_and_backtest(tmp_db):
    _seed_all(tmp_db, days=30)
    result = strat.run(db_path=tmp_db, verbose=False)
    assert "signals" in result
    assert "backtest" in result


def test_run_no_trigger_when_vix_low(tmp_db):
    _seed_all(tmp_db, vix=20.0, vvix=100.0, days=30)
    result = strat.run(db_path=tmp_db, verbose=False)
    assert result["signals"][0]["direction"] == "flat"
    assert result["signals"][0]["triggered"] is False


def test_run_triggers_when_vix_and_vvix_spike(tmp_db):
    # Last bar has extreme readings
    spy_vals  = [500.0] * 29 + [490.0]
    vix_vals  = [20.0]  * 29 + [35.0]   # > 30 threshold
    vvix_vals = [100.0] * 29 + [130.0]  # > 125 threshold
    _seed(tmp_db, "SPY",   spy_vals,  start="2025-01-01")
    _seed(tmp_db, "^VIX",  vix_vals,  start="2025-01-01")
    _seed(tmp_db, "^VVIX", vvix_vals, start="2025-01-01")
    result = strat.run(db_path=tmp_db, verbose=False)
    assert result["signals"][0]["direction"] == "long"
    assert result["signals"][0]["triggered"] is True


def test_run_emits_to_signals_table(tmp_db):
    _seed_all(tmp_db, days=30)
    strat.run(db_path=tmp_db, verbose=False)
    conn = get_connection(tmp_db)
    count = conn.execute(
        "SELECT COUNT(*) FROM signals WHERE name=?", (strat.NAME,)
    ).fetchone()[0]
    assert count == 1
    conn.close()


def test_run_signal_metadata_has_vix_vvix(tmp_db):
    _seed_all(tmp_db, vix=35.0, vvix=130.0, days=30)
    strat.run(db_path=tmp_db, verbose=False)
    conn = get_connection(tmp_db)
    row = conn.execute(
        "SELECT metadata_json FROM signals WHERE name=?", (strat.NAME,)
    ).fetchone()
    meta = json.loads(row["metadata_json"])
    assert "vix" in meta
    assert "vvix" in meta
    assert "spy_close" in meta
    conn.close()


def test_run_returns_error_when_no_data(tmp_db):
    result = strat.run(db_path=tmp_db, verbose=False)
    assert result["signals"] == []
    assert "error" in result["backtest"]


def test_run_backtest_has_expected_keys(tmp_db):
    _seed_all(tmp_db, days=60)
    result = strat.run(db_path=tmp_db, verbose=False)
    bt = result["backtest"]
    if "error" not in bt:
        for key in ("return_pct", "sharpe", "max_drawdown_pct", "num_trades", "buy_hold_pct"):
            assert key in bt


def test_run_is_idempotent(tmp_db):
    _seed_all(tmp_db, days=30)
    strat.run(db_path=tmp_db, verbose=False)
    strat.run(db_path=tmp_db, verbose=False)
    conn = get_connection(tmp_db)
    count = conn.execute(
        "SELECT COUNT(*) FROM signals WHERE name=?", (strat.NAME,)
    ).fetchone()[0]
    assert count == 1  # UNIQUE(name, ticker, date)
    conn.close()
