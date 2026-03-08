"""
Multi-LLM Pipeline Orchestrator — end-to-end flow from raw bookmark to validated
Pine Script v6 strategy or indicator.

Flow: Classify (xAI) → Plan (Claude) → Generate (ChatGPT) → Validate → Cache → Save
"""
from __future__ import annotations

import json
import os
import time
from concurrent.futures import ThreadPoolExecutor, as_completed
from dataclasses import dataclass, field
from pathlib import Path
from typing import Optional

from src.classifiers.finance_classifier import (
    FinanceClassifier,
    ClassificationResult,
    ClassificationError,
)
from src.planners.strategy_planner import StrategyPlanner, StrategyPlan, PlanningError
from src.generators.pinescript_generator import PineScriptGenerator, GenerationError
from src.validators.pinescript_validator import PineScriptValidator, ValidationResult
from src.cache.bookmark_cache import BookmarkCache


@dataclass
class PipelineResult:
    """Result of a single bookmark through the multi-LLM pipeline."""
    tweet_id: str
    classification: Optional[ClassificationResult] = None
    plan: Optional[StrategyPlan] = None
    pine_script: str = ""
    validation: Optional[ValidationResult] = None
    output_path: Optional[str] = None
    skipped: bool = False
    skip_reason: str = ""
    cached: bool = False
    error: str = ""


class MultiLLMPipeline:
    """Orchestrates: classify (xAI) → plan (Claude) → generate (ChatGPT) → validate → cache."""

    def __init__(
        self,
        output_dir: str = "output",
        cache_enabled: bool = True,
        cache_path: str = "cache/bookmarks.db",
        xai_api_key: Optional[str] = None,
        anthropic_api_key: Optional[str] = None,
        openai_api_key: Optional[str] = None,
    ) -> None:
        self.classifier = FinanceClassifier(
            client=_make_xai_client(xai_api_key)
        )
        self.planner = StrategyPlanner(
            client=_make_anthropic_client(anthropic_api_key)
        )
        self.generator = PineScriptGenerator(
            client=_make_openai_client(openai_api_key)
        )
        self.validator = PineScriptValidator()
        self.cache = BookmarkCache(cache_path) if cache_enabled else None
        self.output_dir = Path(output_dir)
        self.output_dir.mkdir(parents=True, exist_ok=True)

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

        pipeline_t0 = time.time()

        # 0. Check cache for complete result
        if self.cache and self.cache.has_script(tweet_id):
            print(f"  [pipeline] Cache hit for {tweet_id} — skipping all API calls")
            return self._load_from_cache(tweet_id)

        # 1. Classify (xAI Grok)
        classification = None
        if self.cache and self.cache.has_classification(tweet_id):
            print(f"  [pipeline] Classification cached — skipping xAI call")
            classification = self.cache.get_classification(tweet_id)
        else:
            print(f"  [pipeline] Step 1/4: Classifying tweet via xAI Grok...")
            t0 = time.time()
            try:
                classification = self.classifier.classify(
                    tweet_id=tweet_id,
                    text=tweet_text,
                    image_urls=image_urls,
                )
            except ClassificationError as e:
                print(f"  [pipeline] Classification FAILED ({time.time() - t0:.1f}s): {e}")
                result.error = f"Classification failed: {e}"
                result.validation = ValidationResult()
                result.validation.fail(result.error)
                return result

            elapsed = time.time() - t0
            print(f"  [pipeline] Classification done ({elapsed:.1f}s): "
                  f"{'FINANCE' if classification.is_finance else 'NOT FINANCE'} "
                  f"({classification.confidence:.0%} confidence, "
                  f"topic={classification.detected_topic})")

            if self.cache:
                self.cache.save_classification(classification)

        result.classification = classification

        # Skip non-finance tweets
        if not classification.is_finance:
            result.skipped = True
            result.skip_reason = f"Not finance-related: {classification.summary}"
            result.validation = ValidationResult()
            print(f"  [pipeline] Skipping: {result.skip_reason}")
            return result

        # 2. Plan (Claude Opus)
        plan = None
        if self.cache and self.cache.has_plan(tweet_id):
            print(f"  [pipeline] Plan cached — skipping Claude call")
            plan = self.cache.get_plan(tweet_id)
        else:
            print(f"  [pipeline] Step 2/4: Creating strategy plan via Claude Opus...")
            t0 = time.time()
            try:
                plan = self.planner.plan(
                    classification=classification,
                    author=author,
                    tweet_date=tweet_date,
                    chart_description=chart_description,
                )
            except PlanningError as e:
                print(f"  [pipeline] Planning FAILED ({time.time() - t0:.1f}s): {e}")
                result.error = f"Planning failed: {e}"
                result.validation = ValidationResult()
                result.validation.fail(result.error)
                return result

            elapsed = time.time() - t0
            print(f"  [pipeline] Plan done ({elapsed:.1f}s): "
                  f"{plan.script_type} — {plan.title} "
                  f"({plan.ticker}, {plan.direction}, {plan.timeframe})")

            if self.cache:
                self.cache.save_plan(plan)

        result.plan = plan

        # 3. Generate (ChatGPT)
        print(f"  [pipeline] Step 3/4: Generating Pine Script via ChatGPT...")
        t0 = time.time()
        try:
            pine_code = self.generator.generate(plan)
        except GenerationError as e:
            print(f"  [pipeline] Generation FAILED ({time.time() - t0:.1f}s): {e}")
            result.error = f"Generation failed: {e}"
            result.validation = ValidationResult()
            result.validation.fail(result.error)
            return result

        elapsed = time.time() - t0
        lines = pine_code.count("\n") + 1
        print(f"  [pipeline] Generation done ({elapsed:.1f}s): {lines} lines of Pine Script")
        result.pine_script = pine_code

        # 4. Validate
        print(f"  [pipeline] Step 4/4: Validating Pine Script...")
        validation = self.validator.validate(pine_code, script_type=plan.script_type)
        result.validation = validation
        if validation.valid:
            print(f"  [pipeline] Validation PASSED")
        else:
            print(f"  [pipeline] Validation FAILED: {validation.errors}")
        if validation.warnings:
            print(f"  [pipeline] Warnings: {validation.warnings}")

        # 5. Cache
        if self.cache:
            self.cache.save_script(
                tweet_id=tweet_id,
                pine_script=pine_code,
                validation_passed=validation.valid,
                validation_errors=validation.errors,
            )

        total = time.time() - pipeline_t0
        print(f"  [pipeline] Total pipeline time: {total:.1f}s")

        # 6. Save
        if save:
            _tweet_url = tweet_url or f"https://x.com/{author}/status/{tweet_id}" if author else ""
            result.output_path = self._save(
                plan, pine_code, validation,
                tweet_url=_tweet_url,
                chart_description=chart_description,
                image_urls=image_urls,
            )

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

    def _load_from_cache(self, tweet_id: str) -> PipelineResult:
        """Load a complete result from cache."""
        result = PipelineResult(tweet_id=tweet_id, cached=True)
        result.classification = self.cache.get_classification(tweet_id)
        result.plan = self.cache.get_plan(tweet_id)

        row = self.cache.get(tweet_id)
        result.pine_script = row.get("pine_script", "")

        validation = ValidationResult()
        if not row.get("validation_passed"):
            errors = json.loads(row.get("validation_errors", "[]"))
            for err in errors:
                validation.fail(err)
        result.validation = validation

        return result

    def _save(
        self,
        plan: StrategyPlan,
        pine_code: str,
        validation: ValidationResult,
        tweet_url: str = "",
        chart_description: str = "",
        image_urls: list[str] | None = None,
    ) -> str:
        safe_author = (plan.author or "unknown").replace(" ", "_")
        safe_ticker = plan.ticker.replace("/", "-")
        filename = f"{safe_author}_{safe_ticker}_{plan.tweet_date or 'undated'}.pine"
        filepath = self.output_dir / filename

        with open(filepath, "w") as f:
            f.write(pine_code)

        # Parse chart_description as JSON (vision returns structured data)
        chart_data = None
        if chart_description:
            chart_data = _parse_chart_json(chart_description)

        meta_path = filepath.with_suffix(".meta.json")
        meta = {
            "tweet_id": plan.tweet_id,
            "tweet_url": tweet_url,
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

        return str(filepath)


def _parse_chart_json(text: str) -> dict | None:
    """Parse JSON from vision response, stripping markdown fences if present."""
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
