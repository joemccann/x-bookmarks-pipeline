"""
Pipeline Orchestrator — end-to-end flow from raw bookmark to validated
Pine Script v6 strategy.
"""

from __future__ import annotations

import json
import os
from dataclasses import dataclass
from pathlib import Path
from typing import Optional

from src.parsers.bookmark_parser import BookmarkParser, TradingSignal
from src.generators.pinescript_generator import PineScriptGenerator
from src.validators.pinescript_validator import PineScriptValidator, ValidationResult


@dataclass
class PipelineResult:
    signal: TradingSignal
    pine_script: str
    validation: ValidationResult
    output_path: Optional[str] = None


class BookmarkToPineScriptPipeline:
    """Orchestrates: parse → generate → validate → save."""

    def __init__(
        self,
        api_key: Optional[str] = None,
        model: str = "grok-4.1",
        output_dir: str = "output",
    ) -> None:
        self.parser = BookmarkParser()
        self.generator = PineScriptGenerator(api_key=api_key, model=model)
        self.validator = PineScriptValidator()
        self.output_dir = Path(output_dir)
        self.output_dir.mkdir(parents=True, exist_ok=True)

    def run(
        self,
        tweet_text: str,
        chart_description: str = "",
        author: str = "",
        tweet_date: str = "",
        save: bool = True,
    ) -> PipelineResult:
        # 1. Parse
        signal = self.parser.parse(tweet_text, chart_description, author, tweet_date)

        # 2. Generate
        pine_code = self.generator.generate(signal)

        # 3. Validate
        validation = self.validator.validate(pine_code)

        # 4. Save
        output_path = None
        if save:
            output_path = self._save(signal, pine_code, validation)

        return PipelineResult(
            signal=signal,
            pine_script=pine_code,
            validation=validation,
            output_path=output_path,
        )

    def _save(
        self,
        signal: TradingSignal,
        pine_code: str,
        validation: ValidationResult,
    ) -> str:
        safe_author = signal.author.replace(" ", "_") or "unknown"
        safe_ticker = signal.ticker.replace("/", "-")
        filename = f"{safe_author}_{safe_ticker}_{signal.tweet_date or 'undated'}.pine"
        filepath = self.output_dir / filename

        with open(filepath, "w") as f:
            f.write(pine_code)

        # Also save metadata
        meta_path = filepath.with_suffix(".meta.json")
        meta = {
            "author": signal.author,
            "date": signal.tweet_date,
            "ticker": signal.ticker,
            "direction": signal.direction,
            "timeframe": signal.timeframe,
            "indicators": signal.indicators,
            "pattern": signal.pattern,
            "key_levels": signal.key_levels,
            "validation_passed": validation.valid,
            "validation_errors": validation.errors,
            "validation_warnings": validation.warnings,
        }
        with open(meta_path, "w") as f:
            json.dump(meta, f, indent=2)

        return str(filepath)
