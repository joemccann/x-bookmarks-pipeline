"""
Multi-LLM Pipeline Orchestrator — end-to-end flow from raw bookmark to
categorized output with optional Pine Script v6 generation.

Flow:
  Classify (xAI) → [Vision if images] → [Plan + Generate if finance] → Save all

Every bookmark gets a .meta.json in output/{category}/{subcategory}/.
Finance bookmarks additionally get .pine files.
"""
from __future__ import annotations

import json
import os
import re
import time
from concurrent.futures import ThreadPoolExecutor, as_completed
from dataclasses import dataclass, field
from pathlib import Path
from typing import Optional

from src.classifiers.finance_classifier import (
    BookmarkClassifier,
    FinanceClassifier,
    ClassificationResult,
    ClassificationError,
)
from src.planners.strategy_planner import StrategyPlanner, StrategyPlan, PlanningError
from src.generators.pinescript_generator import PineScriptGenerator, GenerationError
from src.validators.pinescript_validator import PineScriptValidator, ValidationResult
from src.cache.bookmark_cache import BookmarkCache
from src.config import OUTPUT_DIR, CACHE_PATH, MAX_WORKERS


@dataclass
class PipelineResult:
    """Result of a single bookmark through the multi-LLM pipeline."""
    tweet_id: str
    classification: Optional[ClassificationResult] = None
    plan: Optional[StrategyPlan] = None
    pine_script: str = ""
    validation: Optional[ValidationResult] = None
    output_path: Optional[str] = None
    meta_path: Optional[str] = None
    chart_data: Optional[dict] = None
    cached: bool = False
    error: str = ""


class MultiLLMPipeline:
    """Orchestrates: classify (xAI) → vision (Claude) → plan (Claude) → generate (ChatGPT) → validate → save."""

    def __init__(
        self,
        output_dir: str | None = None,
        cache_enabled: bool = True,
        cache_path: str | None = None,
        vision_enabled: bool = True,
        xai_api_key: Optional[str] = None,
        anthropic_api_key: Optional[str] = None,
        openai_api_key: Optional[str] = None,
    ) -> None:
        self.classifier = BookmarkClassifier(
            client=_make_xai_client(xai_api_key)
        )
        self.planner = StrategyPlanner(
            client=_make_anthropic_client(anthropic_api_key)
        )
        self.generator = PineScriptGenerator(
            client=_make_openai_client(openai_api_key)
        )
        self.validator = PineScriptValidator()
        self.cache = BookmarkCache(cache_path or CACHE_PATH) if cache_enabled else None
        self.output_dir = Path(output_dir or OUTPUT_DIR)
        self.output_dir.mkdir(parents=True, exist_ok=True)
        self.vision_enabled = vision_enabled
        self._anthropic_api_key = anthropic_api_key

    def run(
        self,
        tweet_id: str,
        tweet_text: str,
        image_urls: list[str] | None = None,
        author: str = "",
        tweet_date: str = "",
        chart_description: str = "",
        tweet_url: str = "",
        save: bool = True,
    ) -> PipelineResult:
        """Run the full pipeline for a single bookmark."""
        image_urls = image_urls or []
        result = PipelineResult(tweet_id=tweet_id)

        from src.console import console
        pipeline_t0 = time.time()

        # 0. Check cache for completed result
        if self.cache and self.cache.has_completed(tweet_id):
            return self._load_from_cache(tweet_id)

        # Determine step labels based on whether this is finance (decided after classify)
        # We use dynamic step numbering after classification

        # 1. Classify (xAI Grok) — always runs
        classification = None
        if self.cache and self.cache.has_classification(tweet_id):
            console.print("    [dim]classify — cached[/dim]")
            classification = self.cache.get_classification(tweet_id)
        else:
            console.print("    [bold cyan]1[/bold cyan] [dim]Classifying via xAI Grok...[/dim]")
            t0 = time.time()
            try:
                classification = self.classifier.classify(
                    tweet_id=tweet_id,
                    text=tweet_text,
                    image_urls=image_urls,
                )
            except ClassificationError as e:
                console.print(f"    [bold red]classify FAILED[/bold red] [dim]{time.time() - t0:.1f}s[/dim]: {e}")
                result.error = f"Classification failed: {e}"
                result.validation = ValidationResult()
                result.validation.fail(result.error)
                return result

            elapsed = time.time() - t0
            cat_badge = f"[bold]{classification.category}/{classification.subcategory}[/bold]"
            fin = "[green]FINANCE[/green]" if classification.is_finance else "[dim]non-finance[/dim]"
            console.print(
                f"    [bold cyan]1[/bold cyan] {cat_badge} {fin} "
                f"[dim]{classification.confidence:.0%} ({elapsed:.1f}s)[/dim]"
            )

            if self.cache:
                self.cache.save_classification(classification)

        result.classification = classification

        # 2. Vision (Claude) — if images AND (finance OR has_visual_data)
        chart_data = None
        needs_vision = (
            self.vision_enabled
            and image_urls
            and (classification.is_finance or classification.has_visual_data)
            and not chart_description  # skip if already provided
        )

        if needs_vision:
            if self.cache and self.cache.has_chart_data(tweet_id):
                console.print("    [dim]vision — cached[/dim]")
                chart_data = self.cache.get_chart_data(tweet_id)
            else:
                console.print("    [bold cyan]2[/bold cyan] [dim]Vision analysis via Claude...[/dim]")
                t0 = time.time()
                chart_description = self._run_vision(image_urls)
                elapsed = time.time() - t0
                if chart_description:
                    chart_data = _parse_chart_json(chart_description)
                    console.print(f"    [bold cyan]2[/bold cyan] [dim]vision done ({elapsed:.1f}s)[/dim]")
                    if self.cache and chart_data:
                        self.cache.save_chart_data(tweet_id, chart_data)
                else:
                    console.print(f"    [bold cyan]2[/bold cyan] [dim]vision returned empty ({elapsed:.1f}s)[/dim]")
        elif chart_description:
            chart_data = _parse_chart_json(chart_description)

        result.chart_data = chart_data

        # 3-4. Plan + Generate + Validate — only for finance bookmarks
        if classification.is_finance:
            # 3. Plan (Claude Opus)
            plan = None
            if self.cache and self.cache.has_plan(tweet_id):
                console.print("    [dim]plan — cached[/dim]")
                plan = self.cache.get_plan(tweet_id)
            else:
                console.print("    [bold cyan]3[/bold cyan] [dim]Planning via Claude Opus...[/dim]")
                t0 = time.time()
                try:
                    plan = self.planner.plan(
                        classification=classification,
                        author=author,
                        tweet_date=tweet_date,
                        chart_description=chart_description,
                    )
                except PlanningError as e:
                    console.print(f"    [bold red]plan FAILED[/bold red] [dim]{time.time() - t0:.1f}s[/dim]: {e}")
                    result.error = f"Planning failed: {e}"
                    result.validation = ValidationResult()
                    result.validation.fail(result.error)
                    # Still save meta even on plan failure
                    if save:
                        result.meta_path = self._save_meta(
                            tweet_id, classification, chart_data,
                            author=author, tweet_date=tweet_date,
                            tweet_url=tweet_url, image_urls=image_urls,
                        )
                    return result

                elapsed = time.time() - t0
                console.print(
                    f"    [bold cyan]3[/bold cyan] [bold]{plan.script_type}[/bold] "
                    f"[dim]{plan.title} ({elapsed:.1f}s)[/dim]"
                )

                if self.cache:
                    self.cache.save_plan(plan)

            result.plan = plan

            # 4. Generate (ChatGPT)
            console.print("    [bold cyan]4[/bold cyan] [dim]Generating Pine Script via ChatGPT...[/dim]")
            t0 = time.time()
            try:
                pine_code = self.generator.generate(plan)
            except GenerationError as e:
                console.print(f"    [bold red]generate FAILED[/bold red] [dim]{time.time() - t0:.1f}s[/dim]: {e}")
                result.error = f"Generation failed: {e}"
                result.validation = ValidationResult()
                result.validation.fail(result.error)
                if save:
                    result.meta_path = self._save_meta(
                        tweet_id, classification, chart_data,
                        author=author, tweet_date=tweet_date,
                        tweet_url=tweet_url, image_urls=image_urls,
                        plan=plan,
                    )
                return result

            elapsed = time.time() - t0
            lines = pine_code.count("\n") + 1
            console.print(f"    [bold cyan]4[/bold cyan] [dim]{lines} lines generated ({elapsed:.1f}s)[/dim]")
            result.pine_script = pine_code

            # Validate
            validation = self.validator.validate(pine_code, script_type=plan.script_type)
            result.validation = validation
            if validation.valid:
                console.print(f"    [bold green]VALID[/bold green]")
            else:
                console.print(f"    [bold red]INVALID[/bold red]")

            # Cache script
            if self.cache:
                self.cache.save_script(
                    tweet_id=tweet_id,
                    pine_script=pine_code,
                    validation_passed=validation.valid,
                    validation_errors=validation.errors,
                )

        total = time.time() - pipeline_t0
        console.print(f"    [dim]pipeline total: {total:.1f}s[/dim]")

        # 5. Save — ALL bookmarks get .meta.json; finance also gets .pine
        if save:
            _tweet_url = tweet_url or (f"https://x.com/{author}/status/{tweet_id}" if author else "")

            if classification.is_finance and result.plan:
                result.output_path, result.meta_path = self._save_finance(
                    result.plan, result.pine_script, result.validation or ValidationResult(),
                    classification=classification,
                    chart_data=chart_data,
                    tweet_url=_tweet_url,
                    image_urls=image_urls,
                )
            else:
                result.meta_path = self._save_meta(
                    tweet_id, classification, chart_data,
                    author=author, tweet_date=tweet_date,
                    tweet_url=_tweet_url, image_urls=image_urls,
                )

        # Mark completed in cache
        if self.cache:
            self.cache.mark_completed(tweet_id)

        return result

    def run_batch(
        self,
        bookmarks: list[dict],
        save: bool = True,
        max_workers: int = 3,
    ) -> list[PipelineResult]:
        """Process multiple bookmarks in parallel."""
        results: list[PipelineResult] = []

        with ThreadPoolExecutor(max_workers=max_workers) as executor:
            futures = {
                executor.submit(
                    self.run,
                    tweet_id=bm["tweet_id"],
                    tweet_text=bm.get("text", ""),
                    image_urls=bm.get("image_urls", []),
                    author=bm.get("author", ""),
                    tweet_date=bm.get("date", ""),
                    chart_description=bm.get("chart_description", ""),
                    save=save,
                ): bm
                for bm in bookmarks
            }

            for future in as_completed(futures):
                results.append(future.result())

        return results

    # ------------------------------------------------------------------
    # Vision
    # ------------------------------------------------------------------

    def _run_vision(self, image_urls: list[str]) -> str:
        """Run Claude vision analysis on images."""
        from src.generators.vision_analyzer import ClaudeVisionAnalyzer
        analyzer = ClaudeVisionAnalyzer(
            client=_make_anthropic_client(self._anthropic_api_key)
        )
        return analyzer.analyze_all(image_urls)

    # ------------------------------------------------------------------
    # Cache loading
    # ------------------------------------------------------------------

    def _load_from_cache(self, tweet_id: str) -> PipelineResult:
        """Load a complete result from cache."""
        result = PipelineResult(tweet_id=tweet_id, cached=True)
        result.classification = self.cache.get_classification(tweet_id)
        result.plan = self.cache.get_plan(tweet_id)
        result.chart_data = self.cache.get_chart_data(tweet_id)

        row = self.cache.get(tweet_id)
        result.pine_script = row.get("pine_script", "") or ""

        if result.pine_script:
            validation = ValidationResult()
            if not row.get("validation_passed"):
                errors = json.loads(row.get("validation_errors", "[]"))
                for err in errors:
                    validation.fail(err)
            result.validation = validation

        return result

    # ------------------------------------------------------------------
    # Save helpers
    # ------------------------------------------------------------------

    def _category_dir(self, classification: ClassificationResult) -> Path:
        """Build output/{category}/{subcategory}/ directory path."""
        cat = _sanitize_path(classification.category or "other")
        sub = _sanitize_path(classification.subcategory or "general")
        d = self.output_dir / cat / sub
        d.mkdir(parents=True, exist_ok=True)
        return d

    def _save_finance(
        self,
        plan: StrategyPlan,
        pine_code: str,
        validation: ValidationResult,
        classification: ClassificationResult,
        chart_data: Optional[dict] = None,
        tweet_url: str = "",
        image_urls: list[str] | None = None,
    ) -> tuple[str, str]:
        """Save .pine + .meta.json for finance bookmarks. Returns (pine_path, meta_path)."""
        out_dir = self._category_dir(classification)

        safe_author = _sanitize_path(plan.author or "unknown")
        safe_ticker = plan.ticker.replace("/", "-")
        filename = f"{safe_author}_{safe_ticker}_{plan.tweet_date or 'undated'}.pine"
        filepath = out_dir / filename

        if pine_code:
            with open(filepath, "w") as f:
                f.write(pine_code)

        meta_path = filepath.with_suffix(".meta.json")
        meta = {
            "tweet_id": plan.tweet_id,
            "tweet_url": tweet_url,
            "category": classification.category,
            "subcategory": classification.subcategory,
            "is_finance": True,
            "script_type": plan.script_type,
            "author": plan.author,
            "date": plan.tweet_date,
            "ticker": plan.ticker,
            "direction": plan.direction,
            "timeframe": plan.timeframe,
            "indicators": plan.indicators,
            "pattern": plan.pattern,
            "key_levels": plan.key_levels,
            "rationale": plan.rationale,
            "image_urls": image_urls or [],
            "chart_data": chart_data,
            "validation_passed": validation.valid,
            "validation_errors": validation.errors,
            "validation_warnings": validation.warnings,
        }
        with open(meta_path, "w") as f:
            json.dump(meta, f, indent=2)

        return str(filepath), str(meta_path)

    def _save_meta(
        self,
        tweet_id: str,
        classification: ClassificationResult,
        chart_data: Optional[dict] = None,
        author: str = "",
        tweet_date: str = "",
        tweet_url: str = "",
        image_urls: list[str] | None = None,
        plan: Optional[StrategyPlan] = None,
    ) -> str:
        """Save .meta.json for any bookmark. Returns meta_path."""
        out_dir = self._category_dir(classification)

        safe_author = _sanitize_path(author or "unknown")
        filename = f"{safe_author}_{tweet_date or 'undated'}.meta.json"
        # Append tweet_id suffix to avoid collisions
        stem = f"{safe_author}_{tweet_date or 'undated'}_{tweet_id[:8]}"
        meta_path = out_dir / f"{stem}.meta.json"

        meta = {
            "tweet_id": tweet_id,
            "tweet_url": tweet_url,
            "category": classification.category,
            "subcategory": classification.subcategory,
            "is_finance": classification.is_finance,
            "confidence": classification.confidence,
            "has_visual_data": classification.has_visual_data,
            "detected_topic": classification.detected_topic,
            "summary": classification.summary,
            "author": author,
            "date": tweet_date,
            "image_urls": image_urls or [],
            "chart_data": chart_data,
        }
        with open(meta_path, "w") as f:
            json.dump(meta, f, indent=2)

        return str(meta_path)


def _parse_chart_json(text: str | None) -> dict | None:
    """Parse JSON from vision response, stripping markdown fences if present."""
    if not text:
        return None
    cleaned = text.strip()
    if cleaned.startswith("```"):
        lines = cleaned.split("\n")
        # Drop first line (```json) and last line (```)
        lines = [l for l in lines[1:] if l.strip() != "```"]
        cleaned = "\n".join(lines)
    try:
        return json.loads(cleaned)
    except (json.JSONDecodeError, TypeError):
        return None


def _sanitize_path(s: str) -> str:
    """Sanitize a string for use as a directory/file name component."""
    s = s.lower().strip()
    s = s.replace(" ", "_")
    s = re.sub(r"[^a-z0-9_-]", "", s)
    return s or "unknown"


# ---------------------------------------------------------------------------
# Client factory helpers (allow dependency injection for testing)
# ---------------------------------------------------------------------------

def _make_xai_client(api_key: Optional[str] = None):
    from src.clients.xai_client import XAIClient
    return XAIClient(api_key=api_key) if api_key else XAIClient()


def _make_anthropic_client(api_key: Optional[str] = None):
    from src.clients.anthropic_client import AnthropicClient
    return AnthropicClient(api_key=api_key) if api_key else AnthropicClient()


def _make_openai_client(api_key: Optional[str] = None):
    from src.clients.openai_client import OpenAIClient
    return OpenAIClient(api_key=api_key) if api_key else OpenAIClient()
