"""Tests for MultiLLMPipeline — full flow, cache hits, skip non-finance."""
from __future__ import annotations

import json
import pytest
from unittest.mock import patch, MagicMock

from src.pipeline import MultiLLMPipeline, PipelineResult
from src.classifiers.finance_classifier import ClassificationResult, ClassificationError
from src.planners.strategy_planner import StrategyPlan, PlanningError
from src.generators.pinescript_generator import GenerationError
from src.validators.pinescript_validator import ValidationResult
from src.config import OPENAI_MODEL
from tests.conftest import VALID_STRATEGY_PINE


@pytest.fixture
def pipeline(tmp_path):
    """Pipeline with mocked API clients."""
    with patch("src.pipeline._make_xai_client") as mock_xai, \
         patch("src.pipeline._make_anthropic_client") as mock_anthropic, \
         patch("src.pipeline._make_openai_client") as mock_openai:

        mock_xai.return_value = MagicMock()
        mock_anthropic.return_value = MagicMock()
        mock_openai_inst = MagicMock()
        mock_openai_inst.model = OPENAI_MODEL
        mock_openai.return_value = mock_openai_inst

        p = MultiLLMPipeline(
            output_dir=str(tmp_path / "output"),
            cache_enabled=True,
            cache_path=str(tmp_path / "test.db"),
        )
        yield p


class TestPipelineFullFlow:
    def test_successful_pipeline_run(self, pipeline):
        """Happy path: classify → plan → generate → validate."""
        # Mock classifier
        classification = ClassificationResult(
            tweet_id="pipe1",
            is_finance=True,
            confidence=0.95,
            classification_source="text",
            has_trading_pattern=True,
            detected_topic="crypto",
            summary="BTC setup",
            raw_text="BTC long $42k",
        )
        pipeline.classifier.classify = MagicMock(return_value=classification)

        # Mock planner
        plan = StrategyPlan(
            tweet_id="pipe1",
            script_type="strategy",
            title="BTC Strategy",
            ticker="BTCUSDT",
            direction="long",
            timeframe="D",
            indicators=["RSI"],
            author="testuser",
            tweet_date="2026-03-01",
        )
        pipeline.planner.plan = MagicMock(return_value=plan)

        # Mock generator
        pipeline.generator.generate = MagicMock(return_value=VALID_STRATEGY_PINE)

        result = pipeline.run(
            tweet_id="pipe1",
            tweet_text="BTC long $42k",
            author="testuser",
            tweet_date="2026-03-01",
            save=False,
        )
        assert result.validation.valid
        assert "//@version=6" in result.pine_script
        assert not result.skipped
        assert result.classification.is_finance

    def test_non_finance_tweet_skipped(self, pipeline):
        classification = ClassificationResult(
            tweet_id="pipe2",
            is_finance=False,
            confidence=0.1,
            classification_source="none",
            summary="Not finance",
            raw_text="Great pasta recipe",
        )
        pipeline.classifier.classify = MagicMock(return_value=classification)
        pipeline.planner.plan = MagicMock()
        pipeline.generator.generate = MagicMock()

        result = pipeline.run(
            tweet_id="pipe2",
            tweet_text="Great pasta recipe",
            save=False,
        )
        assert result.skipped
        assert "Not finance" in result.skip_reason
        # Planner and generator should NOT have been called
        pipeline.planner.plan.assert_not_called()
        pipeline.generator.generate.assert_not_called()

    def test_classification_error_returns_error_result(self, pipeline):
        pipeline.classifier.classify = MagicMock(
            side_effect=ClassificationError("API timeout")
        )
        result = pipeline.run(
            tweet_id="pipe3",
            tweet_text="some text",
            save=False,
        )
        assert "Classification failed" in result.error
        assert not result.validation.valid

    def test_planning_error_returns_error_result(self, pipeline):
        classification = ClassificationResult(
            tweet_id="pipe4",
            is_finance=True,
            confidence=0.9,
            classification_source="text",
            raw_text="BTC long",
        )
        pipeline.classifier.classify = MagicMock(return_value=classification)
        pipeline.planner.plan = MagicMock(
            side_effect=PlanningError("Claude error")
        )
        result = pipeline.run(
            tweet_id="pipe4",
            tweet_text="BTC long",
            save=False,
        )
        assert "Planning failed" in result.error

    def test_generation_error_returns_error_result(self, pipeline):
        classification = ClassificationResult(
            tweet_id="pipe5",
            is_finance=True,
            confidence=0.9,
            classification_source="text",
            raw_text="BTC long",
        )
        plan = StrategyPlan(tweet_id="pipe5", title="Test")
        pipeline.classifier.classify = MagicMock(return_value=classification)
        pipeline.planner.plan = MagicMock(return_value=plan)
        pipeline.generator.generate = MagicMock(
            side_effect=GenerationError("OpenAI error")
        )
        result = pipeline.run(
            tweet_id="pipe5",
            tweet_text="BTC long",
            save=False,
        )
        assert "Generation failed" in result.error


class TestPipelineCache:
    def test_cache_hit_skips_api_calls(self, pipeline):
        """When full result is cached, no API calls should be made."""
        pipeline.classifier.classify = MagicMock()
        pipeline.planner.plan = MagicMock()
        pipeline.generator.generate = MagicMock()

        # Pre-populate cache
        classification = ClassificationResult(
            tweet_id="cached1",
            is_finance=True,
            confidence=0.95,
            classification_source="text",
            raw_text="BTC long",
        )
        plan = StrategyPlan(tweet_id="cached1", title="Cached Strategy")
        pipeline.cache.save_classification(classification)
        pipeline.cache.save_plan(plan)
        pipeline.cache.save_script("cached1", VALID_STRATEGY_PINE, True, [])

        result = pipeline.run(
            tweet_id="cached1",
            tweet_text="BTC long",
            save=False,
        )
        assert result.cached
        assert result.pine_script == VALID_STRATEGY_PINE
        pipeline.classifier.classify.assert_not_called()

    def test_partial_cache_resumes(self, pipeline):
        """When only classification is cached, pipeline resumes from planning."""
        pipeline.classifier.classify = MagicMock()

        classification = ClassificationResult(
            tweet_id="partial1",
            is_finance=True,
            confidence=0.9,
            classification_source="text",
            raw_text="BTC long",
        )
        pipeline.cache.save_classification(classification)

        plan = StrategyPlan(
            tweet_id="partial1",
            title="Test",
            author="testuser",
            tweet_date="2026-03-01",
        )
        pipeline.planner.plan = MagicMock(return_value=plan)
        pipeline.generator.generate = MagicMock(return_value=VALID_STRATEGY_PINE)

        result = pipeline.run(
            tweet_id="partial1",
            tweet_text="BTC long",
            save=False,
        )
        # Classifier should NOT have been called (cached)
        pipeline.classifier.classify.assert_not_called()
        # But planner and generator should have been called
        pipeline.planner.plan.assert_called_once()
        pipeline.generator.generate.assert_called_once()
