"""
Runner — orchestrates the full trading engine cycle:
  1. Index pipeline output → finance_signals
  2. Fetch latest market data → market_data
  3. Run indicators → signals
  4. Run strategies → signals + backtest stats
"""
from __future__ import annotations

from pathlib import Path
from typing import Sequence

from trading.config import DEFAULT_TICKERS, SIGNALS_DB_PATH
from trading.db.schema import setup
from trading.indicators import REGISTRY as INDICATOR_REGISTRY
from trading.strategies import REGISTRY as STRATEGY_REGISTRY


def run_all(
    tickers: Sequence[str] = DEFAULT_TICKERS,
    db_path: Path = SIGNALS_DB_PATH,
    skip_index: bool = False,
    skip_fetch: bool = False,
    verbose: bool = True,
) -> dict:
    """Run the full cycle and return a summary dict."""
    setup(db_path)
    results: dict = {}

    # 1. Index pipeline output
    if not skip_index:
        from trading import indexer
        if verbose:
            print("\n[1/4] Indexing pipeline output...")
        results["index"] = indexer.run(db_path=db_path, verbose=verbose)

    # 2. Fetch market data
    if not skip_fetch:
        from trading.fetchers import market_data
        if verbose:
            print(f"\n[2/4] Fetching market data for: {', '.join(tickers)}")
        results["fetch"] = market_data.fetch(tickers=tickers, db_path=db_path, verbose=verbose)

    # 3. Run indicators
    if verbose:
        print(f"\n[3/4] Running {len(INDICATOR_REGISTRY)} indicator(s)...")
    results["indicators"] = {}
    for name, module in INDICATOR_REGISTRY.items():
        results["indicators"][name] = module.run(db_path=db_path, verbose=verbose)

    # 4. Run strategies
    if verbose:
        print(f"\n[4/4] Running {len(STRATEGY_REGISTRY)} strategy/strategies...")
    results["strategies"] = {}
    for name, module in STRATEGY_REGISTRY.items():
        results["strategies"][name] = module.run(db_path=db_path, verbose=verbose)

    return results
