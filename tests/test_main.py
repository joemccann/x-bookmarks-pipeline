"""Tests for main.py CLI."""
from __future__ import annotations

import pytest
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
                    "total": 0, "classified": 0, "planned": 0, "scripted": 0, "valid": 0,
                }):
                    with patch("src.cache.bookmark_cache.BookmarkCache.close"):
                        from main import main
                        result = main()
                        assert result == 0
