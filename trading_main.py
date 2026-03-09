#!/usr/bin/env python3
"""
Trading Engine CLI

Commands:
  index     Scan output/finance/ and index all .meta.json + .pine into signals.db
  fetch     Fetch latest market data from yfinance into signals.db
  run       Run all indicators and strategies
  list      List indexed finance signals from the pipeline
  signals   Show emitted indicator/strategy signals

Examples:
  python trading_main.py index
  python trading_main.py fetch --tickers ^VIX,^VVIX,SPY,PSP,^MOVE
  python trading_main.py run
  python trading_main.py run --skip-fetch
  python trading_main.py list --type strategy
  python trading_main.py list --subcategory volatility
  python trading_main.py signals --name vix_vvix_mean_reversion
"""
from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

# Ensure trading/ package is importable when running from repo root
sys.path.insert(0, str(Path(__file__).parent / "trading"))

from trading.config import SIGNALS_DB_PATH, DEFAULT_TICKERS
from trading.db.schema import setup


def cmd_index(args: argparse.Namespace) -> None:
    from trading import indexer
    from trading.db.reader import summary
    print(f"Indexing pipeline output → {SIGNALS_DB_PATH}")
    indexer.run(verbose=True)
    s = summary()
    print(f"\nDB summary: {s['total']} total signals")
    for k, v in s["by_type"].items():
        print(f"  {k}: {v}")
    print("  by subcategory:")
    for k, v in s["by_subcategory"].items():
        print(f"    {k}: {v}")


def cmd_fetch(args: argparse.Namespace) -> None:
    from trading.fetchers import market_data
    tickers = args.tickers.split(",") if args.tickers else list(DEFAULT_TICKERS)
    print(f"Fetching market data for: {', '.join(tickers)}")
    market_data.fetch(tickers=tickers, verbose=True)


def cmd_run(args: argparse.Namespace) -> None:
    from trading.runner import run_all
    tickers = args.tickers.split(",") if args.tickers else list(DEFAULT_TICKERS)
    run_all(
        tickers=tickers,
        skip_index=args.skip_index,
        skip_fetch=args.skip_fetch,
        verbose=True,
    )


def cmd_list(args: argparse.Namespace) -> None:
    from trading.db.reader import list_finance_signals
    rows = list_finance_signals(
        script_type=args.type,
        subcategory=args.subcategory,
        ticker=args.ticker,
    )
    if not rows:
        print("No signals found. Run: python trading_main.py index")
        return

    print(f"{'TYPE':<12} {'TICKER':<12} {'SUBCATEGORY':<20} {'AUTHOR':<20} {'DATE':<12} RATIONALE")
    print("-" * 110)
    for r in rows:
        rationale = (r["rationale"] or "")[:60].replace("\n", " ")
        print(
            f"{(r['script_type'] or ''):<12} "
            f"{(r['ticker'] or ''):<12} "
            f"{(r['subcategory'] or ''):<20} "
            f"{(r['author'] or ''):<20} "
            f"{(r['date'] or ''):<12} "
            f"{rationale}"
        )
    print(f"\n{len(rows)} signal(s)")


def cmd_signals(args: argparse.Namespace) -> None:
    from trading.db.reader import get_signals
    rows = get_signals(
        name=args.name,
        ticker=args.ticker,
        signal_type=args.type,
        start=args.start,
    )
    if not rows:
        print("No emitted signals found. Run: python trading_main.py run")
        return

    print(f"{'NAME':<30} {'TICKER':<10} {'DATE':<12} {'VALUE':>10} {'DIR':<8} METADATA")
    print("-" * 100)
    for r in rows:
        meta = json.loads(r["metadata_json"] or "{}")
        meta_str = ", ".join(f"{k}={v}" for k, v in list(meta.items())[:3])
        print(
            f"{(r['name'] or ''):<30} "
            f"{(r['ticker'] or ''):<10} "
            f"{(r['date'] or ''):<12} "
            f"{(r['value'] or 0):>10.3f} "
            f"{(r['direction'] or ''):<8} "
            f"{meta_str}"
        )
    print(f"\n{len(rows)} signal(s)")


def main() -> None:
    parser = argparse.ArgumentParser(
        prog="trading_main.py",
        description="Trading Engine — indicators and strategies from x-bookmarks-pipeline",
    )
    sub = parser.add_subparsers(dest="command", required=True)

    # index
    sub.add_parser("index", help="Index pipeline output into signals.db")

    # fetch
    p_fetch = sub.add_parser("fetch", help="Fetch market data from yfinance")
    p_fetch.add_argument("--tickers", help="Comma-separated tickers (default: config DEFAULT_TICKERS)")

    # run
    p_run = sub.add_parser("run", help="Run all indicators and strategies")
    p_run.add_argument("--tickers", help="Comma-separated tickers for market data fetch")
    p_run.add_argument("--skip-index", action="store_true", help="Skip indexing step")
    p_run.add_argument("--skip-fetch", action="store_true", help="Skip market data fetch")

    # list
    p_list = sub.add_parser("list", help="List indexed pipeline finance signals")
    p_list.add_argument("--type", choices=["indicator","strategy"], help="Filter by script_type")
    p_list.add_argument("--subcategory", help="Filter by subcategory")
    p_list.add_argument("--ticker", help="Filter by ticker")

    # signals
    p_sig = sub.add_parser("signals", help="Show emitted indicator/strategy signals")
    p_sig.add_argument("--name", help="Filter by signal name")
    p_sig.add_argument("--ticker", help="Filter by ticker")
    p_sig.add_argument("--type", choices=["indicator","strategy"], help="Filter by signal_type")
    p_sig.add_argument("--start", help="Start date (YYYY-MM-DD)")

    args = parser.parse_args()
    setup()  # ensure DB + tables exist

    dispatch = {
        "index":   cmd_index,
        "fetch":   cmd_fetch,
        "run":     cmd_run,
        "list":    cmd_list,
        "signals": cmd_signals,
    }
    dispatch[args.command](args)


if __name__ == "__main__":
    main()
