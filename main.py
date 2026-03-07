#!/usr/bin/env python3
"""
CLI entrypoint for the X Bookmarks → Pine Script v6 pipeline.

Usage examples:

  # From inline text
  python main.py --text "BTC breakout above \$42k, RSI oversold on 4h. Target \$45k, SL \$40k" \
                 --author "CryptoTrader99" --date "2026-03-01"

  # From a JSON bookmark file
  python main.py --file bookmark.json

  # With chart description
  python main.py --text "Long ETH here" \
                 --chart "4h chart showing ascending triangle with support at 3200 and resistance at 3500" \
                 --author "DeFiWhale" --date "2026-03-05"
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

from src.pipeline import BookmarkToPineScriptPipeline


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Convert X (Twitter) bookmarks into TradingView Pine Script v6 strategies.",
    )
    parser.add_argument("--text", "-t", help="Tweet text content.")
    parser.add_argument("--chart", "-c", default="", help="Chart image description (from Grok vision).")
    parser.add_argument("--author", "-a", default="", help="Tweet author handle.")
    parser.add_argument("--date", "-d", default="", help="Tweet date (YYYY-MM-DD).")
    parser.add_argument("--file", "-f", help="Path to a JSON bookmark file with keys: text, chart_description, author, date.")
    parser.add_argument("--model", "-m", default="grok-4.1", help="xAI model to use (default: grok-4.1).")
    parser.add_argument("--output-dir", "-o", default="output", help="Directory for output files.")
    parser.add_argument("--no-save", action="store_true", help="Print to stdout only, don't save files.")

    args = parser.parse_args()

    # Load from file or CLI args
    if args.file:
        bookmark = json.loads(Path(args.file).read_text())
        tweet_text = bookmark.get("text", "")
        chart_description = bookmark.get("chart_description", "")
        author = bookmark.get("author", "")
        tweet_date = bookmark.get("date", "")
    elif args.text:
        tweet_text = args.text
        chart_description = args.chart
        author = args.author
        tweet_date = args.date
    else:
        parser.error("Provide either --text or --file.")
        return 1

    # Run pipeline
    pipeline = BookmarkToPineScriptPipeline(
        model=args.model,
        output_dir=args.output_dir,
    )
    result = pipeline.run(
        tweet_text=tweet_text,
        chart_description=chart_description,
        author=author,
        tweet_date=tweet_date,
        save=not args.no_save,
    )

    # Output
    print("=" * 60)
    print(f"Ticker    : {result.signal.ticker}")
    print(f"Direction : {result.signal.direction}")
    print(f"Timeframe : {result.signal.timeframe}")
    print(f"Indicators: {', '.join(result.signal.indicators) or 'none'}")
    print(f"Pattern   : {result.signal.pattern or 'none'}")
    print(f"Levels    : {result.signal.key_levels}")
    print("=" * 60)

    if result.validation.errors:
        print("\n⚠ VALIDATION ERRORS:")
        for err in result.validation.errors:
            print(f"  ✗ {err}")

    if result.validation.warnings:
        print("\nValidation warnings:")
        for w in result.validation.warnings:
            print(f"  ● {w}")

    if result.validation.valid:
        print("\n✓ Validation passed.")

    print(f"\n{'─' * 60}")
    print(result.pine_script)
    print(f"{'─' * 60}")

    if result.output_path:
        print(f"\nSaved to: {result.output_path}")

    return 0 if result.validation.valid else 1


if __name__ == "__main__":
    sys.exit(main())
