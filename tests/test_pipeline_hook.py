"""Tests for MultiLLMPipeline.on_meta_saved hook."""
from __future__ import annotations

from unittest.mock import MagicMock, patch, mock_open
from pathlib import Path

import pytest

from src.pipeline import MultiLLMPipeline


def _make_pipeline(hook=None):
    """Build a pipeline with all LLM clients mocked out."""
    with patch("src.pipeline._make_cerebras_client"), \
         patch("src.pipeline._make_xai_client"), \
         patch("src.pipeline._make_anthropic_client"), \
         patch("src.pipeline._make_openai_client"), \
         patch("src.pipeline.BookmarkCache"):
        return MultiLLMPipeline(
            output_dir="/tmp/test_output",
            cache_enabled=False,
            on_meta_saved=hook,
        )


def test_on_meta_saved_called_after_save_meta(tmp_path):
    hook = MagicMock()
    pipeline = _make_pipeline(hook)
    pipeline.output_dir = tmp_path

    from src.classifiers.finance_classifier import ClassificationResult
    clf = ClassificationResult(tweet_id="abc123", category="technology", subcategory="ai")
    clf.is_finance = False

    pipeline._save_meta(
        tweet_id="abc123",
        classification=clf,
        author="testuser",
        tweet_date="2026-01-01",
    )

    hook.assert_called_once()
    call_arg = hook.call_args[0][0]
    assert call_arg.endswith(".meta.json")


def test_on_meta_saved_called_after_save_finance(tmp_path):
    hook = MagicMock()
    pipeline = _make_pipeline(hook)
    pipeline.output_dir = tmp_path

    from src.classifiers.finance_classifier import ClassificationResult
    from src.planners.strategy_planner import StrategyPlan
    from src.validators.pinescript_validator import ValidationResult

    clf = ClassificationResult(tweet_id="fin123", category="finance", subcategory="equities")
    clf.is_finance = True

    plan = StrategyPlan(
        tweet_id="fin123",
        script_type="strategy",
        author="trader",
        tweet_date="2026-01-01",
        ticker="SPY",
        direction="long",
        timeframe="D",
        indicators=["RSI"],
        pattern=None,
        key_levels={},
        rationale="Test",
    )
    validation = ValidationResult(valid=True, errors=[], warnings=[])

    pipeline._save_finance(
        plan=plan,
        pine_code="//@version=6\nstrategy('T',overlay=true)\n",
        validation=validation,
        classification=clf,
    )

    hook.assert_called_once()
    call_arg = hook.call_args[0][0]
    assert call_arg.endswith(".meta.json")


def test_on_meta_saved_not_called_when_none(tmp_path):
    """Pipeline works normally with no hook set."""
    pipeline = _make_pipeline(hook=None)
    pipeline.output_dir = tmp_path

    from src.classifiers.finance_classifier import ClassificationResult
    clf = ClassificationResult(tweet_id="xyz", category="other", subcategory="general")
    # Should not raise
    pipeline._save_meta(tweet_id="xyz", classification=clf, author="u", tweet_date="2026-01-01")


def test_on_meta_saved_hook_error_does_not_crash_pipeline(tmp_path):
    """A failing hook must not propagate and break the pipeline."""
    hook = MagicMock(side_effect=RuntimeError("hook failed"))
    pipeline = _make_pipeline(hook)
    pipeline.output_dir = tmp_path

    from src.classifiers.finance_classifier import ClassificationResult
    clf = ClassificationResult(tweet_id="err123", category="other", subcategory="general")
    # Must not raise
    pipeline._save_meta(tweet_id="err123", classification=clf, author="u", tweet_date="2026-01-01")
    hook.assert_called_once()
