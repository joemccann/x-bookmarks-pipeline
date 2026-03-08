"""Tests for main.py CLI."""
from __future__ import annotations

import json
import pytest
from pathlib import Path
from unittest.mock import patch, MagicMock

from src.fetchers.x_bookmark_fetcher import FetchError


class TestFetchModeErrorHandling:
    """--fetch mode should print a clear error message on auth failure, not crash."""

    def test_401_prints_error_and_exits_1(self):
        with patch("sys.argv", ["main.py", "--fetch", "--no-save"]):
            with patch.dict("os.environ", {
                "X_USER_ACCESS_TOKEN": "bad-token",
                "X_USER_ID": "123",
                "XAI_API_KEY": "test-xai",
                "ANTHROPIC_API_KEY": "test-anthropic",
                "OPENAI_API_KEY": "test-openai",
            }):
                with patch(
                    "src.fetchers.x_bookmark_fetcher.XBookmarkFetcher.fetch",
                    side_effect=FetchError("X API returned status 401: Unauthorized"),
                ):
                    from main import main
                    result = main()
                    assert result == 1


class TestCacheCommands:
    def test_cache_stats_exits_0(self, tmp_path):
        with patch("sys.argv", ["main.py", "--cache-stats"]):
            with patch("src.cache.bookmark_cache.BookmarkCache.__init__", return_value=None):
                with patch("src.cache.bookmark_cache.BookmarkCache.stats", return_value={
                    "total": 0, "classified": 0, "planned": 0, "scripted": 0, "valid": 0, "completed": 0,
                }):
                    with patch("src.cache.bookmark_cache.BookmarkCache.close"):
                        from main import main
                        result = main()
                        assert result == 0

    def test_clear_cache_exits_0(self):
        with patch("sys.argv", ["main.py", "--clear-cache"]):
            with patch("src.cache.bookmark_cache.BookmarkCache.__init__", return_value=None):
                with patch("src.cache.bookmark_cache.BookmarkCache.clear", return_value=5):
                    with patch("src.cache.bookmark_cache.BookmarkCache.close"):
                        from main import main
                        result = main()
                        assert result == 0


class TestMakeTweetId:
    def test_deterministic(self):
        from main import _make_tweet_id
        id1 = _make_tweet_id("BTC long $42k", "trader")
        id2 = _make_tweet_id("BTC long $42k", "trader")
        assert id1 == id2

    def test_different_text_different_id(self):
        from main import _make_tweet_id
        id1 = _make_tweet_id("BTC long", "trader")
        id2 = _make_tweet_id("ETH short", "trader")
        assert id1 != id2

    def test_different_author_different_id(self):
        from main import _make_tweet_id
        id1 = _make_tweet_id("BTC long", "alice")
        id2 = _make_tweet_id("BTC long", "bob")
        assert id1 != id2

    def test_returns_16_char_hex(self):
        from main import _make_tweet_id
        tid = _make_tweet_id("test", "author")
        assert len(tid) == 16
        assert all(c in "0123456789abcdef" for c in tid)


class TestTextMode:
    def test_text_mode_runs_pipeline(self, tmp_path):
        """--text mode should run the pipeline and return 0 for non-finance."""
        from src.pipeline import PipelineResult
        from src.classifiers.finance_classifier import ClassificationResult

        mock_result = PipelineResult(
            tweet_id="text1",
            classification=ClassificationResult(
                tweet_id="text1",
                is_finance=False,
                confidence=0.8,
                classification_source="text",
                category="technology",
                subcategory="AI",
                summary="AI discussion",
                raw_text="New AI model",
            ),
        )

        with patch("sys.argv", [
            "main.py", "--text", "New AI model",
            "--author", "techuser", "--date", "2026-03-01",
            "--no-save", "--output-dir", str(tmp_path / "out"),
        ]):
            with patch("main.MultiLLMPipeline") as mock_pipeline_cls:
                mock_pipeline = MagicMock()
                mock_pipeline.run.return_value = mock_result
                mock_pipeline_cls.return_value = mock_pipeline

                from main import main
                result = main()

        assert result == 0
        mock_pipeline.run.assert_called_once()

    def test_text_mode_finance_valid_returns_0(self, tmp_path):
        """--text mode with valid finance result should return 0."""
        from src.pipeline import PipelineResult
        from src.classifiers.finance_classifier import ClassificationResult
        from src.validators.pinescript_validator import ValidationResult

        validation = ValidationResult()  # valid by default

        mock_result = PipelineResult(
            tweet_id="text2",
            classification=ClassificationResult(
                tweet_id="text2",
                is_finance=True,
                confidence=0.95,
                classification_source="text",
                category="finance",
                subcategory="crypto",
                raw_text="BTC long",
            ),
            pine_script="//@version=6\nstrategy('test')",
            validation=validation,
        )

        with patch("sys.argv", [
            "main.py", "--text", "BTC long $42k",
            "--no-save", "--output-dir", str(tmp_path / "out"),
        ]):
            with patch("main.MultiLLMPipeline") as mock_pipeline_cls:
                mock_pipeline = MagicMock()
                mock_pipeline.run.return_value = mock_result
                mock_pipeline_cls.return_value = mock_pipeline

                from main import main
                result = main()

        assert result == 0


class TestFileMode:
    def test_file_mode_reads_json(self, tmp_path):
        """--file mode should parse JSON and pass fields to pipeline."""
        from src.pipeline import PipelineResult
        from src.classifiers.finance_classifier import ClassificationResult

        bookmark_file = tmp_path / "bookmark.json"
        bookmark_file.write_text(json.dumps({
            "text": "BTC breakout",
            "author": "trader",
            "date": "2026-03-01",
            "image_urls": ["https://example.com/chart.png"],
            "tweet_id": "file_test_1",
        }))

        mock_result = PipelineResult(
            tweet_id="file_test_1",
            classification=ClassificationResult(
                tweet_id="file_test_1",
                is_finance=False,
                confidence=0.5,
                classification_source="text",
                category="other",
                subcategory="general",
                raw_text="BTC breakout",
            ),
        )

        with patch("sys.argv", [
            "main.py", "--file", str(bookmark_file),
            "--no-save", "--output-dir", str(tmp_path / "out"),
        ]):
            with patch("main.MultiLLMPipeline") as mock_pipeline_cls:
                mock_pipeline = MagicMock()
                mock_pipeline.run.return_value = mock_result
                mock_pipeline_cls.return_value = mock_pipeline

                from main import main
                result = main()

        assert result == 0
        call_kwargs = mock_pipeline.run.call_args
        assert call_kwargs.kwargs["tweet_id"] == "file_test_1"
        assert call_kwargs.kwargs["tweet_text"] == "BTC breakout"
        assert call_kwargs.kwargs["image_urls"] == ["https://example.com/chart.png"]


class TestPrintResult:
    """Test _print_result rendering paths don't crash."""

    def test_cached_result_with_category(self):
        from main import _print_result
        from src.pipeline import PipelineResult
        from src.classifiers.finance_classifier import ClassificationResult

        result = PipelineResult(
            tweet_id="pr1",
            cached=True,
            classification=ClassificationResult(
                tweet_id="pr1",
                is_finance=True,
                category="finance",
                subcategory="crypto",
            ),
        )
        # Should not raise
        _print_result(result)

    def test_non_finance_result_renders_category(self):
        from main import _print_result
        from src.pipeline import PipelineResult
        from src.classifiers.finance_classifier import ClassificationResult

        result = PipelineResult(
            tweet_id="pr2",
            classification=ClassificationResult(
                tweet_id="pr2",
                is_finance=False,
                confidence=0.8,
                category="technology",
                subcategory="AI",
                summary="AI discussion",
            ),
        )
        # Should not raise
        _print_result(result)

    def test_error_result_renders(self):
        from main import _print_result
        from src.pipeline import PipelineResult

        result = PipelineResult(tweet_id="pr3", error="Something broke")
        _print_result(result)

    def test_meta_path_rendered(self):
        from main import _print_result
        from src.pipeline import PipelineResult

        result = PipelineResult(
            tweet_id="pr4",
            meta_path="/tmp/output/tech/ai/user_2026-03-01.meta.json",
        )
        # Should not raise
        _print_result(result)

    def test_cached_without_classification(self):
        from main import _print_result
        from src.pipeline import PipelineResult

        result = PipelineResult(tweet_id="pr5", cached=True)
        # Should not raise even without classification
        _print_result(result)
