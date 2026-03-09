"""
Market data fetcher — yfinance → market_data table in signals.db.

Always fetches into DB first; indicators/strategies read from DB only.
This decouples data sourcing from computation and makes the yfinance
dependency easy to swap (Tiingo, Polygon, etc.) without touching strategy code.
"""
from __future__ import annotations

import sqlite3
from datetime import date, datetime, timedelta
from pathlib import Path
from typing import Sequence

from trading.config import (
    DEFAULT_TICKERS,
    MARKET_DATA_LOOKBACK_DAYS,
    SIGNALS_DB_PATH,
)
from trading.db.schema import get_connection, setup
from trading.db.reader import latest_market_date


def fetch(
    tickers: Sequence[str] = DEFAULT_TICKERS,
    lookback_days: int = MARKET_DATA_LOOKBACK_DAYS,
    db_path: Path = SIGNALS_DB_PATH,
    verbose: bool = True,
) -> dict[str, int]:
    """
    Fetch OHLCV for each ticker from the last known date (or lookback_days ago)
    through today. Upserts into market_data. Returns {"ticker": rows_written}.
    """
    try:
        import yfinance as yf
    except ImportError:
        raise ImportError("yfinance not installed — run: pip install yfinance")

    setup(db_path)
    conn = get_connection(db_path)
    results: dict[str, int] = {}
    today = date.today().isoformat()

    for ticker in tickers:
        # Resume from last fetched date rather than refetching everything
        last = latest_market_date(ticker, db_path)
        if last:
            start = (datetime.fromisoformat(last) + timedelta(days=1)).date().isoformat()
        else:
            start = (date.today() - timedelta(days=lookback_days)).isoformat()

        if start > today:
            if verbose:
                print(f"  {ticker}: already up to date ({last})")
            results[ticker] = 0
            continue

        try:
            df = yf.download(ticker, start=start, end=today, auto_adjust=True, progress=False)
            if df.empty:
                if verbose:
                    print(f"  {ticker}: no data returned for {start}→{today}")
                results[ticker] = 0
                continue

            rows_written = 0
            with conn:
                for idx, row in df.iterrows():
                    day = idx.strftime("%Y-%m-%d")
                    # yfinance MultiIndex columns when downloading single ticker
                    def _val(col: str) -> float | None:
                        try:
                            v = row[col] if col in row.index else None
                            return float(v) if v is not None else None
                        except (TypeError, ValueError):
                            return None

                    conn.execute(
                        """INSERT OR REPLACE INTO market_data
                           (ticker, date, open, high, low, close, volume, fetched_at)
                           VALUES (?, ?, ?, ?, ?, ?, ?, datetime('now'))""",
                        (
                            ticker, day,
                            _val("Open"), _val("High"), _val("Low"),
                            _val("Close"), _val("Volume"),
                        ),
                    )
                    rows_written += 1

            results[ticker] = rows_written
            if verbose:
                print(f"  {ticker}: {rows_written} rows ({start} → {today})")

        except Exception as e:
            results[ticker] = 0
            if verbose:
                print(f"  {ticker}: ERROR — {e}")

    conn.close()
    return results
