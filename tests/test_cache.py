"""Tests for BookmarkCache — save/retrieve/miss/clear/stats using tmp_path."""
from __future__ import annotations

import pytest

from src.cache.bookmark_cache import BookmarkCache
from src.classifiers.finance_classifier import ClassificationResult
from src.planners.strategy_planner import StrategyPlan


@pytest.fixture
def cache(tmp_path):
    db_path = tmp_path / "test.db"
    c = BookmarkCache(db_path=db_path)
    yield c
    c.close()


@pytest.fixture
def classification():
    return ClassificationResult(
        tweet_id="cache_test_1",
        is_finance=True,
        confidence=0.95,
        classification_source="text",
        has_trading_pattern=True,
        has_visual_data=False,
        category="finance",
        subcategory="crypto",
        detected_topic="crypto",
        summary="BTC breakout",
        raw_text="BTC long $42k",
        image_urls=[],
    )


@pytest.fixture
def plan():
    return StrategyPlan(
        tweet_id="cache_test_1",
        script_type="strategy",
        title="BTC Strategy",
        ticker="BTCUSDT",
        direction="long",
        timeframe="D",
        indicators=["RSI"],
        author="testuser",
    )


class TestBookmarkCache:
    def test_empty_cache_returns_none(self, cache):
        assert cache.get("nonexistent") is None
        assert not cache.has_classification("nonexistent")
        assert not cache.has_plan("nonexistent")
        assert not cache.has_script("nonexistent")
        assert not cache.has_chart_data("nonexistent")
        assert not cache.has_completed("nonexistent")

    def test_save_and_retrieve_classification(self, cache, classification):
        cache.save_classification(classification)
        assert cache.has_classification("cache_test_1")
        result = cache.get_classification("cache_test_1")
        assert result.is_finance
        assert result.tweet_id == "cache_test_1"
        assert result.detected_topic == "crypto"
        assert result.category == "finance"
        assert result.subcategory == "crypto"

    def test_save_and_retrieve_plan(self, cache, plan):
        cache.save_plan(plan)
        assert cache.has_plan("cache_test_1")
        result = cache.get_plan("cache_test_1")
        assert result.script_type == "strategy"
        assert result.ticker == "BTCUSDT"

    def test_save_and_retrieve_script(self, cache):
        cache.save_script(
            tweet_id="cache_test_1",
            pine_script="//@version=6\nstrategy('test')",
            validation_passed=True,
            validation_errors=[],
        )
        assert cache.has_script("cache_test_1")
        assert cache.get_script("cache_test_1") == "//@version=6\nstrategy('test')"

    def test_save_and_retrieve_chart_data(self, cache):
        chart_data = {"image_type": "chart", "asset": {"ticker": "BTCUSDT"}}
        cache.save_chart_data("cache_test_1", chart_data)
        assert cache.has_chart_data("cache_test_1")
        result = cache.get_chart_data("cache_test_1")
        assert result["image_type"] == "chart"
        assert result["asset"]["ticker"] == "BTCUSDT"

    def test_mark_completed(self, cache):
        assert not cache.has_completed("cache_test_1")
        cache.mark_completed("cache_test_1")
        assert cache.has_completed("cache_test_1")

    def test_cache_miss_returns_none(self, cache):
        assert cache.get_classification("missing") is None
        assert cache.get_plan("missing") is None
        assert cache.get_script("missing") is None
        assert cache.get_chart_data("missing") is None

    def test_clear_removes_all_entries(self, cache, classification, plan):
        cache.save_classification(classification)
        cache.save_plan(plan)
        count = cache.clear()
        assert count == 1  # one tweet_id
        assert cache.get("cache_test_1") is None

    def test_stats_returns_counts(self, cache, classification, plan):
        cache.save_classification(classification)
        cache.save_plan(plan)
        cache.save_script("cache_test_1", "code", True, [])
        cache.mark_completed("cache_test_1")
        stats = cache.stats()
        assert stats["total"] == 1
        assert stats["classified"] == 1
        assert stats["planned"] == 1
        assert stats["scripted"] == 1
        assert stats["valid"] == 1
        assert stats["completed"] == 1

    def test_upsert_updates_existing(self, cache, classification):
        cache.save_classification(classification)
        classification.confidence = 0.99
        cache.save_classification(classification)
        result = cache.get_classification("cache_test_1")
        assert result.confidence == 0.99

    def test_script_with_validation_errors(self, cache):
        cache.save_script(
            tweet_id="cache_test_2",
            pine_script="bad code",
            validation_passed=False,
            validation_errors=["Missing version", "No strategy()"],
        )
        row = cache.get("cache_test_2")
        assert row["validation_passed"] == 0
