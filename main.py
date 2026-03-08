#!/usr/bin/env python3
"""
CLI entrypoint for the X Bookmarks -> Pine Script v6 multi-LLM pipeline.

Pipeline flow:
  Bookmark -> [xAI] Classify -> [Claude] Plan -> [ChatGPT] Generate -> Validate -> Cache
"""
from __future__ import annotations

from dotenv import load_dotenv
load_dotenv()

import argparse
import hashlib
import json
import sys
import time
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path

from rich.panel import Panel
from rich.syntax import Syntax
from rich.table import Table
from rich.rule import Rule

from src.console import console
from src.pipeline import MultiLLMPipeline, PipelineResult
from src.cache.bookmark_cache import BookmarkCache


# ---------------------------------------------------------------------------
# Output helpers
# ---------------------------------------------------------------------------

def _print_result(result: PipelineResult, index: int | None = None) -> None:
    """Pretty-print a single PipelineResult using Rich."""
    tag = f"[{index}]" if index is not None else ""

    # --- CACHED HIT ---
    if result.cached:
        console.print(
            f"  {tag} [cached]CACHE HIT[/cached] — already processed, skipping",
            style="cached",
        )
        return

    # --- SKIPPED (non-finance) ---
    if result.skipped:
        console.print(
            f"  {tag} [skip]SKIP[/skip] {result.skip_reason}",
            style="skip",
        )
        return

    # --- ERROR ---
    if result.error:
        console.print(f"  {tag} [error]ERROR[/error] {result.error}")
        return

    # --- Classification ---
    if result.classification:
        c = result.classification
        if c.is_finance:
            conf_color = "green" if c.confidence >= 0.8 else "yellow"
            console.print(
                f"  {tag} [success]FINANCE[/success] "
                f"[{conf_color}]{c.confidence:.0%}[/{conf_color}] "
                f"via {c.classification_source}  "
                f"[dim]topic=[/dim][info]{c.detected_topic}[/info]"
            )
        else:
            console.print(
                f"  {tag} [skip]NON-FINANCE[/skip] "
                f"[dim]{c.confidence:.0%} — {c.summary}[/dim]"
            )

    # --- Plan ---
    if result.plan:
        p = result.plan
        script_badge = (
            "[bold green]strategy[/bold green]"
            if p.script_type == "strategy"
            else "[bold blue]indicator[/bold blue]"
        )

        table = Table(show_header=False, box=None, padding=(0, 2), expand=False)
        table.add_column("key", style="dim", width=12)
        table.add_column("value")
        table.add_row("Type", script_badge)
        table.add_row("Title", f"[bold]{p.title}[/bold]")
        table.add_row("Ticker", f"[ticker]{p.ticker}[/ticker]")
        table.add_row("Direction", p.direction)
        table.add_row("Timeframe", p.timeframe)
        table.add_row("Indicators", ", ".join(p.indicators) or "[dim]none[/dim]")
        table.add_row("Pattern", p.pattern or "[dim]none[/dim]")
        if p.key_levels:
            levels_str = "  ".join(f"{k}={v}" for k, v in p.key_levels.items())
            table.add_row("Levels", f"[dim]{levels_str}[/dim]")

        console.print(table)

    # --- Validation ---
    if result.validation:
        if result.validation.valid:
            console.print(f"  [success]PASS[/success] Validation passed")
        else:
            console.print(f"  [error]FAIL[/error] Validation failed:")
            for err in result.validation.errors:
                console.print(f"    [error]x[/error] {err}")

        for w in result.validation.warnings:
            console.print(f"    [warning]![/warning] [dim]{w}[/dim]")

    # --- Pine Script ---
    if result.pine_script:
        syntax = Syntax(
            result.pine_script,
            "javascript",  # closest to Pine Script
            theme="monokai",
            line_numbers=True,
            word_wrap=True,
        )
        console.print(Panel(syntax, title="Pine Script v6", border_style="green", expand=False))

    # --- Save path ---
    if result.output_path:
        console.print(f"  [success]Saved[/success] [dim]{result.output_path}[/dim]")


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
        console.print(f"[warning]Cleared {count} cached entries.[/warning]")
        return 0

    if args.cache_stats:
        cache = BookmarkCache()
        stats = cache.stats()
        cache.close()
        table = Table(title="Cache Statistics", show_header=True)
        table.add_column("Metric", style="bold")
        table.add_column("Count", justify="right", style="cyan")
        for k, v in stats.items():
            table.add_row(k, str(v))
        console.print(table)
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
            console.print(f"[step]Resolving user ID for @{args.x_username}...[/step]")
            fetcher.user_id = fetcher.resolve_user_id(args.x_username)
            console.print(f"  [dim]User ID: {fetcher.user_id}[/dim]")

        console.print(f"[step]Fetching up to {args.max_results} bookmarks...[/step]")
        try:
            bookmarks = fetcher.fetch(max_results=args.max_results)
        except Exception as e:
            console.print(f"[error]Failed to fetch bookmarks: {e}[/error]")
            return 1
        console.print(f"  [success]Fetched {len(bookmarks)} bookmark(s)[/success]\n")

        if not bookmarks:
            console.print("[warning]No bookmarks returned.[/warning]")
            return 1

        use_vision = not args.no_vision
        save = not args.no_save
        batch_t0 = time.time()

        def _process_bookmark(i: int, bm) -> tuple[int, PipelineResult, float]:
            """Process a single bookmark. Returns (index, result, elapsed)."""
            t0 = time.time()
            tweet_id = getattr(bm, "tweet_id", None) or _make_tweet_id(bm.text, bm.author)
            tweet_url = f"https://x.com/{bm.author}/status/{tweet_id}" if bm.author else ""

            # Skip if fully cached
            if pipeline.cache and pipeline.cache.has_script(tweet_id):
                result = pipeline.run(
                    tweet_id=tweet_id, tweet_text=bm.text,
                    author=bm.author, tweet_date=bm.date,
                    tweet_url=tweet_url, save=save,
                )
                return i, result, time.time() - t0

            # Vision analysis
            chart_description = ""
            if use_vision and bm.media_urls:
                vision = ClaudeVisionAnalyzer()
                chart_description = vision.analyze_all(bm.media_urls)

            # Full pipeline
            result = pipeline.run(
                tweet_id=tweet_id,
                tweet_text=bm.text,
                image_urls=getattr(bm, "media_urls", []),
                chart_description=chart_description,
                author=bm.author,
                tweet_date=bm.date,
                tweet_url=tweet_url,
                save=save,
            )
            return i, result, time.time() - t0

        # Process all bookmarks in parallel
        max_workers = min(len(bookmarks), 5)
        results: list[tuple[int, PipelineResult, float]] = []

        console.print(Rule(f"Processing {len(bookmarks)} bookmarks ({max_workers} workers)", style="cyan"))

        if len(bookmarks) == 1:
            results.append(_process_bookmark(1, bookmarks[0]))
        else:
            with ThreadPoolExecutor(max_workers=max_workers) as executor:
                futures = {
                    executor.submit(_process_bookmark, i, bm): i
                    for i, bm in enumerate(bookmarks, start=1)
                }
                for future in as_completed(futures):
                    results.append(future.result())

        # Print results in original order
        results.sort(key=lambda x: x[0])
        exit_code = 0
        cached_count = 0
        processed_count = 0
        skipped_count = 0
        failed_count = 0

        for i, result, elapsed in results:
            bm = bookmarks[i - 1]
            console.print()
            console.print(Rule(style="dim"))

            # Bookmark header
            console.print(
                f"  [tag]\\[{i}/{len(bookmarks)}][/tag] "
                f"[author]@{bm.author or 'unknown'}[/author] "
                f"[dim]{bm.date or 'undated'}[/dim]  "
                f"[dim]{elapsed:.1f}s[/dim]"
            )
            tweet_text = bm.text[:140].replace('\n', ' ')
            console.print(f"  [dim]{tweet_text}{'...' if len(bm.text) > 140 else ''}[/dim]")

            _print_result(result, index=i)

            if result.cached:
                cached_count += 1
            elif result.skipped:
                skipped_count += 1
            elif result.error or (result.validation and not result.validation.valid):
                failed_count += 1
                exit_code = 1
            else:
                processed_count += 1

        # Summary
        total_elapsed = time.time() - batch_t0
        console.print()
        console.print(Rule(style="cyan"))
        summary_parts = []
        if processed_count:
            summary_parts.append(f"[success]{processed_count} processed[/success]")
        if cached_count:
            summary_parts.append(f"[cached]{cached_count} cached[/cached]")
        if skipped_count:
            summary_parts.append(f"[skip]{skipped_count} skipped[/skip]")
        if failed_count:
            summary_parts.append(f"[error]{failed_count} failed[/error]")

        console.print(
            f"  {' | '.join(summary_parts)}  "
            f"[dim]in {total_elapsed:.1f}s[/dim]"
        )
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
        tweet_url = bookmark.get("tweet_url", f"https://x.com/{author}/status/{tweet_id}" if author else "")
    elif args.text:
        tweet_text = args.text
        chart_description = args.chart
        chart_url = args.chart_url or ""
        author = args.author
        tweet_date = args.date
        image_urls = [chart_url] if chart_url else []
        tweet_id = _make_tweet_id(tweet_text, author)
        tweet_url = ""
    else:
        parser.error("Provide either --fetch, --text, or --file.")
        return 1

    # Analyze chart image URL via vision if provided
    if chart_url and not chart_description and not args.no_vision:
        from src.generators.vision_analyzer import ClaudeVisionAnalyzer
        console.print(f"[step]Analyzing chart image with Claude vision...[/step]")
        chart_description = ClaudeVisionAnalyzer().analyze(chart_url)

    result = pipeline.run(
        tweet_id=tweet_id,
        tweet_text=tweet_text,
        image_urls=image_urls,
        chart_description=chart_description,
        author=author,
        tweet_date=tweet_date,
        tweet_url=tweet_url,
        save=not args.no_save,
    )
    _print_result(result)
    return 0 if (result.validation and result.validation.valid) else 1


if __name__ == "__main__":
    sys.exit(main())
