"""Tests for MultiLLMPipeline — full flow, cache hits, non-finance categorization, vision."""
from __future__ import annotations

import json
import pytest
from pathlib import Path
from unittest.mock import patch, MagicMock

from src.pipeline import MultiLLMPipeline, PipelineResult, _sanitize_path, _parse_chart_json
from src.classifiers.finance_classifier import ClassificationResult, ClassificationError
from src.planners.strategy_planner import StrategyPlan, PlanningError
from src.generators.pinescript_generator import GenerationError
from src.validators.pinescript_validator import ValidationResult
from src.config import OPENAI_MODEL
from tests.conftest import VALID_STRATEGY_PINE


@pytest.fixture
def pipeline(tmp_path):
    """Pipeline with mocked API clients."""
    with patch("src.pipeline._make_cerebras_client") as mock_cerebras, \
         patch("src.pipeline._make_xai_client") as mock_xai, \
         patch("src.pipeline._make_anthropic_client") as mock_anthropic, \
         patch("src.pipeline._make_openai_client") as mock_openai:

        mock_cerebras.return_value = MagicMock()
        mock_xai.return_value = MagicMock()
        mock_anthropic.return_value = MagicMock()
        mock_openai_inst = MagicMock()
        mock_openai_inst.model = OPENAI_MODEL
        mock_openai.return_value = mock_openai_inst

        p = MultiLLMPipeline(
            output_dir=str(tmp_path / "output"),
            cache_enabled=True,
            cache_path=str(tmp_path / "test.db"),
            vision_enabled=False,  # disable vision in most tests
        )
        yield p


class TestPipelineFullFlow:
    def test_successful_pipeline_run(self, pipeline):
        """Happy path: classify → plan → generate → validate."""
        classification = ClassificationResult(
            tweet_id="pipe1",
            is_finance=True,
            confidence=0.95,
            classification_source="text",
            has_trading_pattern=True,
            category="finance",
            subcategory="crypto",
            detected_topic="crypto",
            summary="BTC setup",
            raw_text="BTC long $42k",
        )
        pipeline.classifier.classify = MagicMock(return_value=classification)

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
        assert result.classification.is_finance
        assert result.classification.category == "finance"

    def test_non_finance_tweet_saves_meta(self, pipeline, tmp_path):
        """Non-finance tweets are categorized and get .meta.json (no skip)."""
        classification = ClassificationResult(
            tweet_id="pipe2",
            is_finance=False,
            confidence=0.85,
            classification_source="text",
            category="technology",
            subcategory="AI",
            summary="AI model discussion",
            raw_text="The new AI model is great",
        )
        pipeline.classifier.classify = MagicMock(return_value=classification)
        pipeline.planner.plan = MagicMock()
        pipeline.generator.generate = MagicMock()

        result = pipeline.run(
            tweet_id="pipe2",
            tweet_text="The new AI model is great",
            author="techuser",
            tweet_date="2026-03-01",
            save=True,
        )
        # No skip — bookmark is categorized
        assert result.classification.category == "technology"
        assert result.classification.subcategory == "AI"
        # Planner and generator should NOT have been called
        pipeline.planner.plan.assert_not_called()
        pipeline.generator.generate.assert_not_called()
        # .meta.json should exist
        assert result.meta_path is not None
        meta = json.loads(Path(result.meta_path).read_text())
        assert meta["category"] == "technology"
        assert meta["subcategory"] == "AI"
        assert not meta["is_finance"]

    def test_non_finance_no_pine_script(self, pipeline):
        """Non-finance bookmarks should not produce Pine Script."""
        classification = ClassificationResult(
            tweet_id="pipe2b",
            is_finance=False,
            confidence=0.8,
            classification_source="text",
            category="entertainment",
            subcategory="gaming",
            summary="Gaming discussion",
            raw_text="Great game release",
        )
        pipeline.classifier.classify = MagicMock(return_value=classification)

        result = pipeline.run(
            tweet_id="pipe2b",
            tweet_text="Great game release",
            save=False,
        )
        assert result.pine_script == ""
        assert result.plan is None
        assert result.output_path is None

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
            category="finance",
            subcategory="crypto",
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
            category="finance",
            subcategory="crypto",
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
        """When fully completed, no API calls should be made."""
        pipeline.classifier.classify = MagicMock()
        pipeline.planner.plan = MagicMock()
        pipeline.generator.generate = MagicMock()

        # Pre-populate cache
        classification = ClassificationResult(
            tweet_id="cached1",
            is_finance=True,
            confidence=0.95,
            classification_source="text",
            category="finance",
            subcategory="crypto",
            raw_text="BTC long",
        )
        plan = StrategyPlan(tweet_id="cached1", title="Cached Strategy")
        pipeline.cache.save_classification(classification)
        pipeline.cache.save_plan(plan)
        pipeline.cache.save_script("cached1", VALID_STRATEGY_PINE, True, [])
        pipeline.cache.mark_completed("cached1")

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
            category="finance",
            subcategory="crypto",
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


class TestPipelineCategoryDirs:
    def test_finance_saves_to_category_dir(self, pipeline, tmp_path):
        """Finance bookmarks save to output/finance/{subcategory}/."""
        classification = ClassificationResult(
            tweet_id="dir1",
            is_finance=True,
            confidence=0.95,
            classification_source="text",
            category="finance",
            subcategory="crypto",
            detected_topic="crypto",
            raw_text="BTC long",
        )
        plan = StrategyPlan(
            tweet_id="dir1",
            script_type="strategy",
            title="BTC Strategy",
            ticker="BTCUSDT",
            direction="long",
            timeframe="D",
            indicators=["RSI"],
            author="testuser",
            tweet_date="2026-03-01",
        )
        pipeline.classifier.classify = MagicMock(return_value=classification)
        pipeline.planner.plan = MagicMock(return_value=plan)
        pipeline.generator.generate = MagicMock(return_value=VALID_STRATEGY_PINE)

        result = pipeline.run(
            tweet_id="dir1",
            tweet_text="BTC long",
            author="testuser",
            tweet_date="2026-03-01",
            save=True,
        )
        assert result.output_path is not None
        assert "/finance/crypto/" in result.output_path
        assert result.output_path.endswith(".pine")
        assert result.meta_path is not None
        assert "/finance/crypto/" in result.meta_path

    def test_non_finance_saves_to_category_dir(self, pipeline, tmp_path):
        """Non-finance bookmarks save meta to output/{category}/{subcategory}/."""
        classification = ClassificationResult(
            tweet_id="dir2",
            is_finance=False,
            confidence=0.88,
            classification_source="text",
            category="technology",
            subcategory="AI",
            summary="AI discussion",
            raw_text="New AI model released",
        )
        pipeline.classifier.classify = MagicMock(return_value=classification)

        result = pipeline.run(
            tweet_id="dir2",
            tweet_text="New AI model released",
            author="techuser",
            tweet_date="2026-03-03",
            save=True,
        )
        assert result.meta_path is not None
        assert "/technology/ai/" in result.meta_path
        assert result.output_path is None  # no .pine for non-finance


# ---------------------------------------------------------------------------
# Vision-enabled pipeline tests
# ---------------------------------------------------------------------------

@pytest.fixture
def vision_pipeline(tmp_path):
    """Pipeline with vision_enabled=True and mocked clients."""
    with patch("src.pipeline._make_cerebras_client") as mock_cerebras, \
         patch("src.pipeline._make_xai_client") as mock_xai, \
         patch("src.pipeline._make_anthropic_client") as mock_anthropic, \
         patch("src.pipeline._make_openai_client") as mock_openai:

        mock_cerebras.return_value = MagicMock()
        mock_xai.return_value = MagicMock()
        mock_anthropic.return_value = MagicMock()
        mock_openai_inst = MagicMock()
        mock_openai_inst.model = OPENAI_MODEL
        mock_openai.return_value = mock_openai_inst

        p = MultiLLMPipeline(
            output_dir=str(tmp_path / "output"),
            cache_enabled=True,
            cache_path=str(tmp_path / "test.db"),
            vision_enabled=True,
        )
        yield p


class TestPipelineVision:
    def test_vision_runs_for_finance_with_images(self, vision_pipeline):
        """Vision should trigger for finance bookmarks with image URLs."""
        classification = ClassificationResult(
            tweet_id="vis1",
            is_finance=True,
            confidence=0.95,
            classification_source="text",
            category="finance",
            subcategory="crypto",
            raw_text="BTC long",
        )
        vision_pipeline.classifier.classify = MagicMock(return_value=classification)

        plan = StrategyPlan(tweet_id="vis1", title="Test", author="u", tweet_date="2026-03-01")
        vision_pipeline.planner.plan = MagicMock(return_value=plan)
        vision_pipeline.generator.generate = MagicMock(return_value=VALID_STRATEGY_PINE)

        chart_json = json.dumps({"image_type": "chart", "asset": {"ticker": "BTCUSDT"}})
        with patch.object(vision_pipeline, "_run_vision", return_value=chart_json) as mock_vision:
            result = vision_pipeline.run(
                tweet_id="vis1",
                tweet_text="BTC long",
                image_urls=["https://example.com/chart.png"],
                save=False,
            )

        mock_vision.assert_called_once_with(["https://example.com/chart.png"])
        assert result.chart_data is not None
        assert result.chart_data["image_type"] == "chart"

    def test_vision_runs_for_non_finance_with_visual_data(self, vision_pipeline):
        """Vision should trigger for non-finance with has_visual_data=True."""
        classification = ClassificationResult(
            tweet_id="vis2",
            is_finance=False,
            confidence=0.8,
            classification_source="text",
            category="science",
            subcategory="climate",
            has_visual_data=True,
            raw_text="Temperature data",
        )
        vision_pipeline.classifier.classify = MagicMock(return_value=classification)

        chart_json = json.dumps({"image_type": "graph", "description": "temp trends"})
        with patch.object(vision_pipeline, "_run_vision", return_value=chart_json) as mock_vision:
            result = vision_pipeline.run(
                tweet_id="vis2",
                tweet_text="Temperature data",
                image_urls=["https://example.com/graph.png"],
                save=False,
            )

        mock_vision.assert_called_once()
        assert result.chart_data is not None
        assert result.chart_data["image_type"] == "graph"

    def test_vision_skipped_when_chart_description_provided(self, vision_pipeline):
        """Vision should NOT run when chart_description is already provided."""
        classification = ClassificationResult(
            tweet_id="vis3",
            is_finance=True,
            confidence=0.9,
            classification_source="text",
            category="finance",
            subcategory="crypto",
            raw_text="BTC setup",
        )
        vision_pipeline.classifier.classify = MagicMock(return_value=classification)

        plan = StrategyPlan(tweet_id="vis3", title="Test", author="u", tweet_date="2026-03-01")
        vision_pipeline.planner.plan = MagicMock(return_value=plan)
        vision_pipeline.generator.generate = MagicMock(return_value=VALID_STRATEGY_PINE)

        with patch.object(vision_pipeline, "_run_vision") as mock_vision:
            result = vision_pipeline.run(
                tweet_id="vis3",
                tweet_text="BTC setup",
                image_urls=["https://example.com/chart.png"],
                chart_description='{"image_type": "chart"}',
                save=False,
            )

        mock_vision.assert_not_called()
        assert result.chart_data == {"image_type": "chart"}

    def test_vision_skipped_when_no_images(self, vision_pipeline):
        """Vision should NOT run when there are no images."""
        classification = ClassificationResult(
            tweet_id="vis4",
            is_finance=True,
            confidence=0.9,
            classification_source="text",
            category="finance",
            subcategory="crypto",
            raw_text="BTC long",
        )
        vision_pipeline.classifier.classify = MagicMock(return_value=classification)

        plan = StrategyPlan(tweet_id="vis4", title="Test", author="u", tweet_date="2026-03-01")
        vision_pipeline.planner.plan = MagicMock(return_value=plan)
        vision_pipeline.generator.generate = MagicMock(return_value=VALID_STRATEGY_PINE)

        with patch.object(vision_pipeline, "_run_vision") as mock_vision:
            result = vision_pipeline.run(
                tweet_id="vis4",
                tweet_text="BTC long",
                image_urls=[],
                save=False,
            )

        mock_vision.assert_not_called()
        assert result.chart_data is None

    def test_vision_skipped_for_non_finance_without_visual_data(self, vision_pipeline):
        """Vision should NOT run for non-finance without has_visual_data."""
        classification = ClassificationResult(
            tweet_id="vis5",
            is_finance=False,
            confidence=0.8,
            classification_source="text",
            category="other",
            subcategory="general",
            has_visual_data=False,
            raw_text="Random photo",
        )
        vision_pipeline.classifier.classify = MagicMock(return_value=classification)

        with patch.object(vision_pipeline, "_run_vision") as mock_vision:
            result = vision_pipeline.run(
                tweet_id="vis5",
                tweet_text="Random photo",
                image_urls=["https://example.com/photo.jpg"],
                save=False,
            )

        mock_vision.assert_not_called()
        assert result.chart_data is None

    def test_vision_result_cached(self, vision_pipeline):
        """Vision chart_data should be saved to cache."""
        classification = ClassificationResult(
            tweet_id="vis6",
            is_finance=False,
            confidence=0.8,
            classification_source="text",
            category="science",
            subcategory="climate",
            has_visual_data=True,
            raw_text="Data chart",
        )
        vision_pipeline.classifier.classify = MagicMock(return_value=classification)

        chart_json = json.dumps({"image_type": "graph"})
        with patch.object(vision_pipeline, "_run_vision", return_value=chart_json):
            vision_pipeline.run(
                tweet_id="vis6",
                tweet_text="Data chart",
                image_urls=["https://example.com/graph.png"],
                save=False,
            )

        # Verify cache was populated
        assert vision_pipeline.cache.has_chart_data("vis6")
        cached = vision_pipeline.cache.get_chart_data("vis6")
        assert cached["image_type"] == "graph"

    def test_vision_cache_hit_skips_call(self, vision_pipeline):
        """When chart_data is cached, vision should not be called again."""
        classification = ClassificationResult(
            tweet_id="vis7",
            is_finance=True,
            confidence=0.9,
            classification_source="text",
            category="finance",
            subcategory="crypto",
            raw_text="BTC chart",
        )
        # Pre-populate cache
        vision_pipeline.cache.save_classification(classification)
        vision_pipeline.cache.save_chart_data("vis7", {"image_type": "chart", "cached": True})

        vision_pipeline.classifier.classify = MagicMock()

        plan = StrategyPlan(tweet_id="vis7", title="Test", author="u", tweet_date="2026-03-01")
        vision_pipeline.planner.plan = MagicMock(return_value=plan)
        vision_pipeline.generator.generate = MagicMock(return_value=VALID_STRATEGY_PINE)

        with patch.object(vision_pipeline, "_run_vision") as mock_vision:
            result = vision_pipeline.run(
                tweet_id="vis7",
                tweet_text="BTC chart",
                image_urls=["https://example.com/chart.png"],
                save=False,
            )

        mock_vision.assert_not_called()
        assert result.chart_data["cached"] is True

    def test_vision_empty_result_handled(self, vision_pipeline):
        """Vision returning empty string should result in None chart_data."""
        classification = ClassificationResult(
            tweet_id="vis8",
            is_finance=True,
            confidence=0.9,
            classification_source="text",
            category="finance",
            subcategory="crypto",
            raw_text="BTC chart",
        )
        vision_pipeline.classifier.classify = MagicMock(return_value=classification)

        plan = StrategyPlan(tweet_id="vis8", title="Test", author="u", tweet_date="2026-03-01")
        vision_pipeline.planner.plan = MagicMock(return_value=plan)
        vision_pipeline.generator.generate = MagicMock(return_value=VALID_STRATEGY_PINE)

        with patch.object(vision_pipeline, "_run_vision", return_value=""):
            result = vision_pipeline.run(
                tweet_id="vis8",
                tweet_text="BTC chart",
                image_urls=["https://example.com/chart.png"],
                save=False,
            )

        assert result.chart_data is None


# ---------------------------------------------------------------------------
# Cache chart_data in _load_from_cache
# ---------------------------------------------------------------------------

class TestPipelineCacheChartData:
    def test_cache_hit_loads_chart_data(self, pipeline):
        """Cache hit should include chart_data when it exists."""
        classification = ClassificationResult(
            tweet_id="cd1",
            is_finance=True,
            confidence=0.95,
            classification_source="text",
            category="finance",
            subcategory="crypto",
            raw_text="BTC long",
        )
        plan = StrategyPlan(tweet_id="cd1", title="Cached")
        pipeline.cache.save_classification(classification)
        pipeline.cache.save_plan(plan)
        pipeline.cache.save_script("cd1", VALID_STRATEGY_PINE, True, [])
        pipeline.cache.save_chart_data("cd1", {"image_type": "chart", "asset": {"ticker": "BTC"}})
        pipeline.cache.mark_completed("cd1")

        pipeline.classifier.classify = MagicMock()

        result = pipeline.run(tweet_id="cd1", tweet_text="BTC long", save=False)
        assert result.cached
        assert result.chart_data is not None
        assert result.chart_data["image_type"] == "chart"

    def test_cache_hit_without_chart_data(self, pipeline):
        """Cache hit should work even if no chart_data was stored."""
        classification = ClassificationResult(
            tweet_id="cd2",
            is_finance=True,
            confidence=0.95,
            classification_source="text",
            category="finance",
            subcategory="crypto",
            raw_text="BTC long",
        )
        plan = StrategyPlan(tweet_id="cd2", title="Cached")
        pipeline.cache.save_classification(classification)
        pipeline.cache.save_plan(plan)
        pipeline.cache.save_script("cd2", VALID_STRATEGY_PINE, True, [])
        pipeline.cache.mark_completed("cd2")

        result = pipeline.run(tweet_id="cd2", tweet_text="BTC long", save=False)
        assert result.cached
        assert result.chart_data is None


# ---------------------------------------------------------------------------
# run_batch
# ---------------------------------------------------------------------------

class TestPipelineBatch:
    def test_run_batch_processes_multiple(self, pipeline):
        """run_batch should process all bookmarks and return results."""
        classification_finance = ClassificationResult(
            tweet_id="batch1",
            is_finance=True,
            confidence=0.9,
            classification_source="text",
            category="finance",
            subcategory="crypto",
            raw_text="BTC long",
        )
        classification_tech = ClassificationResult(
            tweet_id="batch2",
            is_finance=False,
            confidence=0.8,
            classification_source="text",
            category="technology",
            subcategory="AI",
            raw_text="AI news",
        )

        call_count = {"n": 0}

        def mock_classify(tweet_id, text, image_urls=None):
            call_count["n"] += 1
            if tweet_id == "batch1":
                return classification_finance
            return classification_tech

        pipeline.classifier.classify = MagicMock(side_effect=mock_classify)

        plan = StrategyPlan(tweet_id="batch1", title="Test", author="u", tweet_date="2026-03-01")
        pipeline.planner.plan = MagicMock(return_value=plan)
        pipeline.generator.generate = MagicMock(return_value=VALID_STRATEGY_PINE)

        bookmarks = [
            {"tweet_id": "batch1", "text": "BTC long", "author": "u", "date": "2026-03-01"},
            {"tweet_id": "batch2", "text": "AI news", "author": "t", "date": "2026-03-02"},
        ]

        results = pipeline.run_batch(bookmarks, save=False, max_workers=2)
        assert len(results) == 2

        tweet_ids = {r.tweet_id for r in results}
        assert "batch1" in tweet_ids
        assert "batch2" in tweet_ids


# ---------------------------------------------------------------------------
# Utility functions
# ---------------------------------------------------------------------------

class TestSanitizePath:
    def test_basic_lowercase(self):
        assert _sanitize_path("Finance") == "finance"

    def test_spaces_to_underscores(self):
        assert _sanitize_path("web dev") == "web_dev"

    def test_special_chars_stripped(self):
        assert _sanitize_path("c++/sharp!") == "csharp"

    def test_empty_string_returns_unknown(self):
        assert _sanitize_path("") == "unknown"

    def test_whitespace_only_returns_unknown(self):
        assert _sanitize_path("   ") == "unknown"

    def test_mixed_case_and_chars(self):
        assert _sanitize_path("AI & Machine Learning") == "ai__machine_learning"

    def test_hyphens_preserved(self):
        assert _sanitize_path("web-dev") == "web-dev"

    def test_numbers_preserved(self):
        assert _sanitize_path("gpt4") == "gpt4"

    def test_unicode_stripped(self):
        result = _sanitize_path("caf\u00e9")
        assert result == "caf"


class TestParseChartJson:
    def test_valid_json(self):
        result = _parse_chart_json('{"image_type": "chart"}')
        assert result == {"image_type": "chart"}

    def test_markdown_fenced_json(self):
        text = '```json\n{"image_type": "chart"}\n```'
        result = _parse_chart_json(text)
        assert result == {"image_type": "chart"}

    def test_markdown_fenced_no_language(self):
        text = '```\n{"image_type": "graph"}\n```'
        result = _parse_chart_json(text)
        assert result == {"image_type": "graph"}

    def test_invalid_json_returns_none(self):
        assert _parse_chart_json("not json at all") is None

    def test_empty_string_returns_none(self):
        assert _parse_chart_json("") is None

    def test_none_returns_none(self):
        assert _parse_chart_json(None) is None

    def test_nested_json(self):
        text = '{"asset": {"ticker": "BTC", "name": "Bitcoin"}, "indicators": []}'
        result = _parse_chart_json(text)
        assert result["asset"]["ticker"] == "BTC"
