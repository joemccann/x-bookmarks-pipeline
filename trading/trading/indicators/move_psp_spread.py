"""
Indicator: MOVE / PSP Spread

MOVE index (^MOVE) measures bond market volatility (like VIX but for bonds).
PSP is the ProShares Global Listed Private Equity ETF — a proxy for private
credit/equity risk appetite.

The spread (MOVE - PSP_close) widens when bond vol is elevated relative to
private equity, signaling a potential risk-off regime or credit stress.

Emits one signal row per date with:
  value    = MOVE_close - PSP_close  (raw spread)
  metadata = { move, psp, z_score_90d }
"""
from __future__ import annotations

import json
import sqlite3
from pathlib import Path

import pandas as pd

from trading.config import SIGNALS_DB_PATH
from trading.db.reader import get_market_data
from trading.db.schema import get_connection

NAME = "move_psp_spread"
DESCRIPTION = "MOVE index minus PSP close — bond vol vs. private equity risk appetite"

# How many days to use for rolling z-score normalization
ZSCORE_WINDOW = 90


def run(
    db_path: Path = SIGNALS_DB_PATH,
    verbose: bool = True,
) -> list[dict]:
    """
    Compute MOVE/PSP spread for all dates we have data for both tickers.
    Upserts into signals table. Returns list of emitted signal dicts.
    """
    move_rows = get_market_data("^MOVE", db_path=db_path)
    psp_rows  = get_market_data("PSP",   db_path=db_path)

    if not move_rows or not psp_rows:
        if verbose:
            missing = []
            if not move_rows: missing.append("^MOVE")
            if not psp_rows:  missing.append("PSP")
            print(f"  [{NAME}] No market data for: {', '.join(missing)} — run fetch first")
        return []

    move_df = pd.DataFrame(move_rows).set_index("date")[["close"]].rename(columns={"close": "MOVE"})
    psp_df  = pd.DataFrame(psp_rows).set_index("date")[["close"]].rename(columns={"close": "PSP"})

    df = move_df.join(psp_df, how="inner").dropna()
    if df.empty:
        if verbose:
            print(f"  [{NAME}] No overlapping dates between ^MOVE and PSP")
        return []

    df["spread"] = df["MOVE"] - df["PSP"]
    df["zscore"] = (
        (df["spread"] - df["spread"].rolling(ZSCORE_WINDOW, min_periods=20).mean())
        / df["spread"].rolling(ZSCORE_WINDOW, min_periods=20).std()
    )

    conn = get_connection(db_path)
    emitted = []

    with conn:
        for day, row in df.iterrows():
            metadata = {
                "move":        round(float(row["MOVE"]), 4),
                "psp":         round(float(row["PSP"]), 4),
                "zscore_90d":  round(float(row["zscore"]), 4) if pd.notna(row["zscore"]) else None,
            }
            conn.execute(
                """INSERT OR REPLACE INTO signals
                   (signal_type, name, ticker, date, value, direction, metadata_json)
                   VALUES ('indicator', ?, ?, ?, ?, NULL, ?)""",
                (
                    NAME,
                    "^MOVE/PSP",
                    str(day),
                    round(float(row["spread"]), 4),
                    json.dumps(metadata),
                ),
            )
            emitted.append({"date": day, "spread": row["spread"], **metadata})

    conn.close()

    if verbose:
        latest = emitted[-1] if emitted else {}
        print(
            f"  [{NAME}] {len(emitted)} rows emitted. "
            f"Latest ({latest.get('date', '?')}): "
            f"spread={latest.get('spread', 'N/A'):.2f}, "
            f"z={latest.get('zscore_90d', 'N/A')}"
        )

    return emitted
