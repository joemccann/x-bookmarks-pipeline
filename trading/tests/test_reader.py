"""Tests for trading.db.reader — query helpers."""
from __future__ import annotations

import json
import pytest

from trading.db.schema import setup, get_connection
from trading.db import reader


@pytest.fixture
def db(tmp_path):
    db_path = tmp_path / "test.db"
    setup(db_path)
    conn = get_connection(db_path)
    # Seed finance_signals
    rows = [
        ("tweet1", "strategy", "equities", "SPY",  "2026-01-01", "Strat 1"),
        ("tweet2", "indicator","volatility","^VIX", "2026-01-02", "Indicator 1"),
        ("tweet3", "strategy", "macro",    "TLT",  "2026-01-03", "Strat 2"),
    ]
    for tweet_id, st, sub, ticker, date, rationale in rows:
        conn.execute(
            """INSERT INTO finance_signals
               (tweet_id, script_type, subcategory, ticker, date, rationale)
               VALUES (?,?,?,?,?,?)""",
            (tweet_id, st, sub, ticker, date, rationale),
        )
    # Seed market_data
    conn.execute(
        "INSERT INTO market_data (ticker, date, close) VALUES ('SPY','2026-01-01',500.0)"
    )
    conn.execute(
        "INSERT INTO market_data (ticker, date, close) VALUES ('SPY','2026-01-02',502.0)"
    )
    conn.execute(
        "INSERT INTO market_data (ticker, date, close) VALUES ('^VIX','2026-01-01',25.0)"
    )
    # Seed signals
    conn.execute(
        "INSERT INTO signals (signal_type, name, ticker, date, value, direction) "
        "VALUES ('strategy','vix_buy','SPY','2026-01-01',30.5,'long')"
    )
    conn.commit()
    conn.close()
    return db_path


# ---------------------------------------------------------------------------
# finance_signals
# ---------------------------------------------------------------------------

def test_list_finance_signals_all(db):
    rows = reader.list_finance_signals(db_path=db)
    assert len(rows) == 3


def test_list_finance_signals_by_type(db):
    rows = reader.list_finance_signals(script_type="strategy", db_path=db)
    assert len(rows) == 2
    assert all(r["script_type"] == "strategy" for r in rows)


def test_list_finance_signals_by_subcategory(db):
    rows = reader.list_finance_signals(subcategory="volatility", db_path=db)
    assert len(rows) == 1
    assert rows[0]["ticker"] == "^VIX"


def test_list_finance_signals_by_ticker(db):
    rows = reader.list_finance_signals(ticker="TLT", db_path=db)
    assert len(rows) == 1
    assert rows[0]["tweet_id"] == "tweet3"


def test_list_finance_signals_combined_filter(db):
    rows = reader.list_finance_signals(script_type="strategy", subcategory="macro", db_path=db)
    assert len(rows) == 1
    assert rows[0]["tweet_id"] == "tweet3"


def test_get_finance_signal_found(db):
    row = reader.get_finance_signal("tweet1", db_path=db)
    assert row is not None
    assert row["ticker"] == "SPY"


def test_get_finance_signal_not_found(db):
    row = reader.get_finance_signal("nonexistent", db_path=db)
    assert row is None


def test_summary(db):
    s = reader.summary(db_path=db)
    assert s["total"] == 3
    assert s["by_type"]["strategy"] == 2
    assert s["by_type"]["indicator"] == 1
    assert "equities" in s["by_subcategory"]


# ---------------------------------------------------------------------------
# market_data
# ---------------------------------------------------------------------------

def test_get_market_data_all(db):
    rows = reader.get_market_data("SPY", db_path=db)
    assert len(rows) == 2
    assert rows[0]["date"] == "2026-01-01"  # ordered ASC


def test_get_market_data_with_start(db):
    rows = reader.get_market_data("SPY", start="2026-01-02", db_path=db)
    assert len(rows) == 1
    assert rows[0]["close"] == 502.0


def test_get_market_data_unknown_ticker(db):
    rows = reader.get_market_data("UNKNOWN", db_path=db)
    assert rows == []


def test_latest_market_date(db):
    d = reader.latest_market_date("SPY", db_path=db)
    assert d == "2026-01-02"


def test_latest_market_date_no_data(db):
    d = reader.latest_market_date("MISSING", db_path=db)
    assert d is None


# ---------------------------------------------------------------------------
# emitted signals
# ---------------------------------------------------------------------------

def test_get_signals_all(db):
    rows = reader.get_signals(db_path=db)
    assert len(rows) == 1


def test_get_signals_by_name(db):
    rows = reader.get_signals(name="vix_buy", db_path=db)
    assert len(rows) == 1
    assert rows[0]["direction"] == "long"


def test_get_signals_no_match(db):
    rows = reader.get_signals(name="nonexistent", db_path=db)
    assert rows == []


def test_get_signals_by_start(db):
    rows = reader.get_signals(start="2026-01-02", db_path=db)
    assert rows == []  # signal is on 2026-01-01
