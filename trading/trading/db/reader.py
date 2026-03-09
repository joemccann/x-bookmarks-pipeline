"""
Read-only query helpers for signals.db.

All functions return plain dicts or lists of dicts for easy consumption.
"""
from __future__ import annotations

import json
import sqlite3
from datetime import date
from pathlib import Path
from typing import Any

from trading.config import SIGNALS_DB_PATH, BOOKMARKS_DB_PATH
from trading.db.schema import get_connection


# ---------------------------------------------------------------------------
# finance_signals queries
# ---------------------------------------------------------------------------

def list_finance_signals(
    script_type: str | None = None,
    subcategory: str | None = None,
    ticker: str | None = None,
    db_path: Path = SIGNALS_DB_PATH,
) -> list[dict[str, Any]]:
    """List indexed pipeline signals, optionally filtered."""
    conn = get_connection(db_path)
    clauses, params = [], []
    if script_type:
        clauses.append("script_type = ?")
        params.append(script_type)
    if subcategory:
        clauses.append("subcategory = ?")
        params.append(subcategory)
    if ticker:
        clauses.append("ticker = ?")
        params.append(ticker)

    where = f"WHERE {' AND '.join(clauses)}" if clauses else ""
    rows = conn.execute(
        f"SELECT * FROM finance_signals {where} ORDER BY date DESC",
        params,
    ).fetchall()
    conn.close()
    return [dict(r) for r in rows]


def get_finance_signal(tweet_id: str, db_path: Path = SIGNALS_DB_PATH) -> dict | None:
    """Fetch a single finance_signal by tweet_id."""
    conn = get_connection(db_path)
    row = conn.execute(
        "SELECT * FROM finance_signals WHERE tweet_id = ?", (tweet_id,)
    ).fetchone()
    conn.close()
    return dict(row) if row else None


def summary(db_path: Path = SIGNALS_DB_PATH) -> dict[str, Any]:
    """Return count breakdown of indexed finance signals."""
    conn = get_connection(db_path)
    total = conn.execute("SELECT COUNT(*) FROM finance_signals").fetchone()[0]
    by_type = {
        r["script_type"]: r["cnt"]
        for r in conn.execute(
            "SELECT script_type, COUNT(*) as cnt FROM finance_signals GROUP BY script_type"
        ).fetchall()
    }
    by_sub = {
        r["subcategory"]: r["cnt"]
        for r in conn.execute(
            "SELECT subcategory, COUNT(*) as cnt FROM finance_signals GROUP BY subcategory ORDER BY cnt DESC"
        ).fetchall()
    }
    conn.close()
    return {"total": total, "by_type": by_type, "by_subcategory": by_sub}


# ---------------------------------------------------------------------------
# market_data queries
# ---------------------------------------------------------------------------

def get_market_data(
    ticker: str,
    start: str | None = None,
    end: str | None = None,
    db_path: Path = SIGNALS_DB_PATH,
) -> list[dict[str, Any]]:
    """Return OHLCV rows for a ticker, optionally date-bounded."""
    conn = get_connection(db_path)
    clauses = ["ticker = ?"]
    params: list[Any] = [ticker]
    if start:
        clauses.append("date >= ?")
        params.append(start)
    if end:
        clauses.append("date <= ?")
        params.append(end)
    rows = conn.execute(
        f"SELECT * FROM market_data WHERE {' AND '.join(clauses)} ORDER BY date ASC",
        params,
    ).fetchall()
    conn.close()
    return [dict(r) for r in rows]


def latest_market_date(ticker: str, db_path: Path = SIGNALS_DB_PATH) -> str | None:
    """Return the most recent date we have market data for a ticker."""
    conn = get_connection(db_path)
    row = conn.execute(
        "SELECT MAX(date) as d FROM market_data WHERE ticker = ?", (ticker,)
    ).fetchone()
    conn.close()
    return row["d"] if row else None


# ---------------------------------------------------------------------------
# emitted signals queries
# ---------------------------------------------------------------------------

def get_signals(
    name: str | None = None,
    ticker: str | None = None,
    signal_type: str | None = None,
    start: str | None = None,
    db_path: Path = SIGNALS_DB_PATH,
) -> list[dict[str, Any]]:
    """Query emitted signals."""
    conn = get_connection(db_path)
    clauses, params = [], []
    if name:
        clauses.append("name = ?"); params.append(name)
    if ticker:
        clauses.append("ticker = ?"); params.append(ticker)
    if signal_type:
        clauses.append("signal_type = ?"); params.append(signal_type)
    if start:
        clauses.append("date >= ?"); params.append(start)
    where = f"WHERE {' AND '.join(clauses)}" if clauses else ""
    rows = conn.execute(
        f"SELECT * FROM signals {where} ORDER BY date DESC", params
    ).fetchall()
    conn.close()
    return [dict(r) for r in rows]
