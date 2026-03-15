"""
Strategy: VIX/VVIX Mean Reversion — Buy SPY after extreme vol spikes.

Signal: BUY SPY when VIX > 30 AND VVIX > 125 (extreme fear + vol-of-vol spike)
Exit:   SELL when VIX drops below 20 OR after max_hold_days trading days

Rationale (from @charliebilello bookmark): Large weekly VIX spikes historically
precede positive SPY returns over 13–26 week horizons. When vol-of-vol (VVIX)
also spikes above 125, the dislocation is extreme enough that mean reversion
is statistically robust.

Running this strategy also runs a backtest via backtesting.py and emits:
  - Live signal rows to the signals table (today's signal)
  - A backtest summary in the return dict
"""
from __future__ import annotations

import json
from pathlib import Path

import pandas as pd

from trading.config import SIGNALS_DB_PATH
from trading.db.reader import get_market_data
from trading.db.schema import get_connection

NAME = "vix_vvix_mean_reversion"
DESCRIPTION = "Buy SPY when VIX > 30 AND VVIX > 125; exit when VIX < 20 or after N days"

# Default thresholds — all overridable
VIX_ENTRY_THRESHOLD  = 30.0
VVIX_ENTRY_THRESHOLD = 125.0
VIX_EXIT_THRESHOLD   = 20.0
MAX_HOLD_DAYS        = 65   # ~13 trading weeks


def _build_dataframe(db_path: Path) -> pd.DataFrame | None:
    """Merge SPY OHLCV with VIX and VVIX close into one DataFrame."""
    spy_rows  = get_market_data("SPY",   db_path=db_path)
    vix_rows  = get_market_data("^VIX",  db_path=db_path)
    vvix_rows = get_market_data("^VVIX", db_path=db_path)

    missing = [t for t, r in [("SPY", spy_rows), ("^VIX", vix_rows), ("^VVIX", vvix_rows)] if not r]
    if missing:
        return None

    spy  = pd.DataFrame(spy_rows).set_index("date")[["open","high","low","close","volume"]]
    spy.columns = ["Open","High","Low","Close","Volume"]
    spy.index = pd.DatetimeIndex(spy.index)

    vix  = pd.DataFrame(vix_rows).set_index("date")[["close"]].rename(columns={"close":"VIX"})
    vix.index = pd.DatetimeIndex(vix.index)

    vvix = pd.DataFrame(vvix_rows).set_index("date")[["close"]].rename(columns={"close":"VVIX"})
    vvix.index = pd.DatetimeIndex(vvix.index)

    df = spy.join(vix, how="inner").join(vvix, how="inner").dropna()
    return df if not df.empty else None


def _backtest(df: pd.DataFrame) -> dict:
    """Run backtesting.py and return summary stats."""
    try:
        from backtesting import Backtest, Strategy

        vix_thresh  = VIX_ENTRY_THRESHOLD
        vvix_thresh = VVIX_ENTRY_THRESHOLD
        vix_exit    = VIX_EXIT_THRESHOLD
        max_hold    = MAX_HOLD_DAYS

        class VixVvixStrategy(Strategy):
            def init(self):
                self.vix  = self.data.VIX
                self.vvix = self.data.VVIX
                self._entry_bar: int | None = None

            def next(self):
                vix_now  = self.vix[-1]
                vvix_now = self.vvix[-1]
                in_pos   = self.position

                if not in_pos:
                    if vix_now > vix_thresh and vvix_now > vvix_thresh:
                        self.buy()
                        self._entry_bar = len(self.data)
                else:
                    bars_held = len(self.data) - (self._entry_bar or 0)
                    if vix_now < vix_exit or bars_held >= max_hold:
                        self.position.close()
                        self._entry_bar = None

        bt = Backtest(df, VixVvixStrategy, cash=100_000, commission=0.001)
        stats = bt.run()

        return {
            "return_pct":       round(float(stats["Return [%]"]), 2),
            "sharpe":           round(float(stats["Sharpe Ratio"]), 3) if pd.notna(stats["Sharpe Ratio"]) else None,
            "max_drawdown_pct": round(float(stats["Max. Drawdown [%]"]), 2),
            "num_trades":       int(stats["# Trades"]),
            "win_rate_pct":     round(float(stats["Win Rate [%]"]), 1) if pd.notna(stats["Win Rate [%]"]) else None,
            "buy_hold_pct":     round(float(stats["Buy & Hold Return [%]"]), 2),
            "period_start":     str(df.index[0].date()),
            "period_end":       str(df.index[-1].date()),
        }
    except ImportError:
        return {"error": "backtesting not installed — run: pip install backtesting"}
    except Exception as e:
        return {"error": str(e)}


def _emit_live_signal(df: pd.DataFrame, conn, verbose: bool) -> dict | None:
    """Check today's (latest) bar and emit a live signal if triggered."""
    if df.empty:
        return None

    latest = df.iloc[-1]
    day    = str(df.index[-1].date())
    vix    = float(latest["VIX"])
    vvix   = float(latest["VVIX"])
    spy    = float(latest["Close"])

    triggered = vix > VIX_ENTRY_THRESHOLD and vvix > VVIX_ENTRY_THRESHOLD
    direction = "long" if triggered else "flat"

    meta = {
        "vix": round(vix, 2),
        "vvix": round(vvix, 2),
        "spy_close": round(spy, 2),
        "vix_threshold": VIX_ENTRY_THRESHOLD,
        "vvix_threshold": VVIX_ENTRY_THRESHOLD,
        "triggered": triggered,
    }

    with conn:
        conn.execute(
            """INSERT OR REPLACE INTO signals
               (signal_type, name, ticker, date, value, direction, metadata_json)
               VALUES ('strategy', ?, 'SPY', ?, ?, ?, ?)""",
            (NAME, day, round(vix, 2), direction, json.dumps(meta)),
        )

    if verbose:
        status = "TRIGGERED — BUY SPY" if triggered else "No signal"
        print(
            f"  [{NAME}] {day}: VIX={vix:.1f}, VVIX={vvix:.1f} → {status}"
        )

    return {"date": day, "direction": direction, **meta}


def run(
    db_path: Path = SIGNALS_DB_PATH,
    verbose: bool = True,
) -> dict:
    """
    1. Build combined SPY/VIX/VVIX DataFrame from market_data table.
    2. Run backtest over full history.
    3. Emit today's live signal.
    Returns {"signals": [...], "backtest": {...}}.
    """
    df = _build_dataframe(db_path)
    if df is None:
        msg = "Missing market data — run: bin/trading_main.py fetch"
        if verbose:
            print(f"  [{NAME}] {msg}")
        return {"signals": [], "backtest": {"error": msg}}

    if verbose:
        print(f"  [{NAME}] Running backtest on {len(df)} bars ({df.index[0].date()} → {df.index[-1].date()})")

    backtest_stats = _backtest(df)

    if verbose and "error" not in backtest_stats:
        print(
            f"  [{NAME}] Backtest: return={backtest_stats['return_pct']}%, "
            f"sharpe={backtest_stats['sharpe']}, "
            f"trades={backtest_stats['num_trades']}, "
            f"win_rate={backtest_stats['win_rate_pct']}%"
        )

    conn = get_connection(db_path)
    live_signal = _emit_live_signal(df, conn, verbose)
    conn.close()

    return {
        "signals": [live_signal] if live_signal else [],
        "backtest": backtest_stats,
    }
