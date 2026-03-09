"""Tests for trading.fetchers.market_data — yfinance fetch (mocked)."""
from __future__ import annotations

from datetime import date, timedelta
from unittest.mock import MagicMock, patch

import pandas as pd
import pytest

from trading.db.schema import setup, get_connection
from trading.fetchers import market_data


@pytest.fixture
def tmp_db(tmp_path):
    db = tmp_path / "test.db"
    setup(db)
    return db


def _mock_df(ticker: str, days: int = 3) -> pd.DataFrame:
    """Build a minimal yfinance-style DataFrame."""
    start = date(2026, 1, 1)
    idx = pd.DatetimeIndex([start + timedelta(d) for d in range(days)])
    return pd.DataFrame(
        {
            "Open":   [100.0 + i for i in range(days)],
            "High":   [105.0 + i for i in range(days)],
            "Low":    [95.0  + i for i in range(days)],
            "Close":  [102.0 + i for i in range(days)],
            "Volume": [1_000_000.0] * days,
        },
        index=idx,
    )


def test_fetch_writes_rows(tmp_db):
    df = _mock_df("SPY", 3)
    with patch("yfinance.download", return_value=df):
        result = market_data.fetch(tickers=["SPY"], db_path=tmp_db, verbose=False)
    assert result["SPY"] == 3
    conn = get_connection(tmp_db)
    count = conn.execute("SELECT COUNT(*) FROM market_data WHERE ticker='SPY'").fetchone()[0]
    assert count == 3
    conn.close()


def test_fetch_upserts_existing(tmp_db):
    """Second fetch with no new data returns 0 rows written."""
    df = _mock_df("SPY", 3)
    with patch("yfinance.download", return_value=df):
        market_data.fetch(tickers=["SPY"], db_path=tmp_db, verbose=False)
    # Second call: yfinance returns empty (no new bars since last date)
    with patch("yfinance.download", return_value=pd.DataFrame()):
        result = market_data.fetch(tickers=["SPY"], db_path=tmp_db, verbose=False)
    assert result["SPY"] == 0


def test_fetch_empty_response(tmp_db):
    with patch("yfinance.download", return_value=pd.DataFrame()):
        result = market_data.fetch(tickers=["FAKE"], db_path=tmp_db, verbose=False)
    assert result["FAKE"] == 0


def test_fetch_handles_error(tmp_db):
    with patch("yfinance.download", side_effect=Exception("network error")):
        result = market_data.fetch(tickers=["SPY"], db_path=tmp_db, verbose=False)
    assert result["SPY"] == 0


def test_fetch_multiple_tickers(tmp_db):
    df = _mock_df("X", 2)
    with patch("yfinance.download", return_value=df):
        result = market_data.fetch(tickers=["SPY", "^VIX"], db_path=tmp_db, verbose=False)
    assert "SPY" in result
    assert "^VIX" in result


def test_fetch_already_up_to_date(tmp_db):
    """If latest date is today, skip fetching."""
    today = date.today().isoformat()
    conn = get_connection(tmp_db)
    with conn:
        conn.execute(
            "INSERT INTO market_data (ticker, date, close) VALUES ('SPY', ?, 500.0)",
            (today,),
        )
    conn.close()
    with patch("yfinance.download") as mock_dl:
        result = market_data.fetch(tickers=["SPY"], db_path=tmp_db, verbose=False)
    mock_dl.assert_not_called()
    assert result["SPY"] == 0
