#!/usr/bin/env python3
"""
CLI entrypoint for the X Bookmarks → Pine Script v6 multi-LLM pipeline.

Pipeline flow:
  Bookmark → [xAI] Classify → [Claude] Plan → [ChatGPT] Generate → Validate → Cache

Usage examples:

  # Fetch live bookmarks from X
  python main.py --fetch
  python main.py --fetch --max-results 20

  # From inline text
  python main.py --text "BTC breakout above $42k, RSI oversold on 4h. Target $45k, SL $40k" \
                 --author "CryptoTrader99" --date "2026-03-01"

  # From a JSON bookmark file
  python main.py --file bookmark.json

  # Cache management
  python main.py --cache-stats
  python main.py --clear-cache
"""
from __future__ import annotations

from dotenv import load_dotenv
load_dotenv()

import argparse
import hashlib
import json
import sys
from pathlib import Path

from src.pipeline import MultiLLMPipeline, PipelineResult
from src.cache.bookmark_cache import BookmarkCache


# ---------------------------------------------------------------------------
# Output helpers
# ---------------------------------------------------------------------------

def _print_result(result: PipelineResult, index: int | None = None) -> None:
    """Pretty-print a single PipelineResult."""
    prefix = f"[{index}] " if index is not None else ""
    print(f"\n{'=' * 60}")

    if result.skipped:
        print(f"{prefix}SKIPPED: {result.skip_reason}")
        print("=" * 60)
        return

    if result.cached:
        print(f"{prefix}(cached)")

    if result.error:
        print(f"{prefix}ERROR: {result.error}")
        print("=" * 60)
        return

    if result.classification:
        c = result.classification
        print(f"{prefix}Classification: {'finance' if c.is_finance else 'non-finance'} "
              f"({c.confidence:.0%} via {c.classification_source})")
        if c.detected_topic:
            print(f"{prefix}Topic: {c.detected_topic}")

    if result.plan:
        p = result.plan
        print(f"{prefix}Type      : {p.script_type}")
        print(f"{prefix}Title     : {p.title}")
        print(f"{prefix}Ticker    : {p.ticker}")
        print(f"{prefix}Direction : {p.direction}")
        print(f"{prefix}Timeframe : {p.timeframe}")
        print(f"{prefix}Indicators: {', '.join(p.indicators) or 'none'}")
        print(f"{prefix}Pattern   : {p.pattern or 'none'}")
        if p.key_levels:
            print(f"{prefix}Levels    : {p.key_levels}")
    print("=" * 60)

    if result.validation and result.validation.errors:
        print("\nVALIDATION ERRORS:")
        for err in result.validation.errors:
            print(f"  x {err}")

    if result.validation and result.validation.warnings:
        print("\nValidation warnings:")
        for w in result.validation.warnings:
            print(f"  - {w}")

    if result.validation and result.validation.valid:
        print("\nValidation passed.")

    if result.pine_script:
        print(f"\n{'~' * 60}")
        print(result.pine_script)
        print(f"{'~' * 60}")

    if result.output_path:
        print(f"\nSaved to: {result.output_path}")


def _make_tweet_id(text: str, author: str = "") -> str:
    """Generate a deterministic tweet ID from text content."""
    return hashlib.sha256(f"{author}:{text}".encode()).hexdigest()[:16]


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

def main() -> int:
    parser = argparse.ArgumentParser(
        description="Convert X (Twitter) bookmarks into TradingView Pine Script v6 via multi-LLM pipeline.",
    )

    # --- Live fetch mode ---
    fetch_group = parser.add_argument_group("Live fetch (X API)")
    fetch_group.add_argument(
        "--fetch", action="store_true",
        help="Fetch bookmarks live from X API.",
    )
    fetch_group.add_argument("--x-username", help="Resolve numeric user ID from X username.")
    fetch_group.add_argument(
        "--max-results", type=int, default=10,
        help="Max bookmarks to fetch (default: 10).",
    )

    # --- Manual / single-bookmark mode ---
    manual_group = parser.add_argument_group("Manual input")
    manual_group.add_argument("--text", "-t", help="Tweet text content.")
    manual_group.add_argument("--chart", "-c", default="", help="Plain-text chart description.")
    manual_group.add_argument("--chart-url", help="Chart image URL for vision analysis.")
    manual_group.add_argument("--author", "-a", default="", help="Tweet author handle.")
    manual_group.add_argument("--date", "-d", default="", help="Tweet date (YYYY-MM-DD).")
    manual_group.add_argument(
        "--file", "-f",
        help="Path to a JSON bookmark file.",
    )

    # --- Pipeline options ---
    parser.add_argument("--output-dir", "-o", default="output", help="Directory for output files.")
    parser.add_argument("--no-save", action="store_true", help="Print to stdout only.")
    parser.add_argument("--no-vision", action="store_true", help="Skip vision analysis.")

    # --- Cache options ---
    cache_group = parser.add_argument_group("Cache")
    cache_group.add_argument("--no-cache", action="store_true", help="Disable SQLite cache.")
    cache_group.add_argument("--clear-cache", action="store_true", help="Clear all cached results and exit.")
    cache_group.add_argument("--cache-stats", action="store_true", help="Show cache statistics and exit.")

    args = parser.parse_args()

    # --- Cache management commands ---
    if args.clear_cache:
        cache = BookmarkCache()
        count = cache.clear()
        cache.close()
        print(f"Cleared {count} cached entries.")
        return 0

    if args.cache_stats:
        cache = BookmarkCache()
        stats = cache.stats()
        cache.close()
        print("Cache statistics:")
        for k, v in stats.items():
            print(f"  {k}: {v}")
        return 0

    pipeline = MultiLLMPipeline(
        output_dir=args.output_dir,
        cache_enabled=not args.no_cache,
    )

    # -----------------------------------------------------------------------
    # Mode 1: Live fetch from X API
    # -----------------------------------------------------------------------
    if args.fetch:
        from src.fetchers.x_bookmark_fetcher import XBookmarkFetcher
        from src.generators.vision_analyzer import ClaudeVisionAnalyzer

        fetcher = XBookmarkFetcher()

        if args.x_username:
            print(f"Resolving user ID for @{args.x_username}...")
            fetcher.user_id = fetcher.resolve_user_id(args.x_username)
            print(f"  -> User ID: {fetcher.user_id}")

        print(f"Fetching up to {args.max_results} bookmarks...")
        try:
            bookmarks = fetcher.fetch(max_results=args.max_results)
        except Exception as e:
            print(f"\nx Failed to fetch bookmarks: {e}")
            return 1
        print(f"  -> Fetched {len(bookmarks)} bookmark(s).\n")

        if not bookmarks:
            print("No bookmarks returned.")
            return 1

        vision = None if args.no_vision else ClaudeVisionAnalyzer()
        exit_code = 0

        for i, bm in enumerate(bookmarks, start=1):
            print(f"\n[{i}/{len(bookmarks)}] @{bm.author or 'unknown'} - {bm.date or 'undated'}")
            print(f"  {bm.text[:120]}{'...' if len(bm.text) > 120 else ''}")

            chart_description = ""
            if vision and bm.media_urls:
                print(f"  Analyzing {len(bm.media_urls)} chart image(s) with Claude vision...")
                chart_description = vision.analyze_all(bm.media_urls)

            tweet_id = getattr(bm, "tweet_id", None) or _make_tweet_id(bm.text, bm.author)

            result = pipeline.run(
                tweet_id=tweet_id,
                tweet_text=bm.text,
                image_urls=getattr(bm, "media_urls", []),
                chart_description=chart_description,
                author=bm.author,
                tweet_date=bm.date,
                save=not args.no_save,
            )
            _print_result(result, index=i)
            if result.validation and not result.validation.valid:
                exit_code = 1

        return exit_code

    # -----------------------------------------------------------------------
    # Mode 2: Single bookmark from --file or --text
    # -----------------------------------------------------------------------
    if args.file:
        bookmark = json.loads(Path(args.file).read_text())
        tweet_text = bookmark.get("text", "")
        chart_description = bookmark.get("chart_description", "")
        chart_url = bookmark.get("chart_url", "")
        author = bookmark.get("author", "")
        tweet_date = bookmark.get("date", "")
        image_urls = bookmark.get("image_urls", [])
        tweet_id = bookmark.get("tweet_id", _make_tweet_id(tweet_text, author))
    elif args.text:
        tweet_text = args.text
        chart_description = args.chart
        chart_url = args.chart_url or ""
        author = args.author
        tweet_date = args.date
        image_urls = [chart_url] if chart_url else []
        tweet_id = _make_tweet_id(tweet_text, author)
    else:
        parser.error("Provide either --fetch, --text, or --file.")
        return 1

    # Analyze chart image URL via vision if provided
    if chart_url and not chart_description and not args.no_vision:
        from src.generators.vision_analyzer import ClaudeVisionAnalyzer
        print(f"Analyzing chart image with Claude vision: {chart_url}")
        chart_description = ClaudeVisionAnalyzer().analyze(chart_url)

    result = pipeline.run(
        tweet_id=tweet_id,
        tweet_text=tweet_text,
        image_urls=image_urls,
        chart_description=chart_description,
        author=author,
        tweet_date=tweet_date,
        save=not args.no_save,
    )
    _print_result(result)
    return 0 if (result.validation and result.validation.valid) else 1


if __name__ == "__main__":
    sys.exit(main())
