"""Tests for XBookmarkFetcher — error handling."""
from __future__ import annotations

import pytest
from unittest.mock import patch, MagicMock

import httpx

from src.fetchers.x_bookmark_fetcher import XBookmarkFetcher, FetchError


@pytest.fixture
def fetcher():
    """Fetcher with dummy credentials."""
    with patch.dict("os.environ", {"X_USER_ACCESS_TOKEN": "test-token"}):
        f = XBookmarkFetcher(user_id="12345")
    return f


class TestFetcherErrorHandling:
    """Fetcher should raise FetchError on API failures, not crash with raw httpx errors."""

    def test_missing_token_raises_value_error(self):
        """Missing access token should raise ValueError at init."""
        with patch.dict("os.environ", {"X_USER_ACCESS_TOKEN": ""}, clear=False):
            with pytest.raises(ValueError, match="X_USER_ACCESS_TOKEN"):
                XBookmarkFetcher()

    def test_missing_user_id_raises_value_error(self, fetcher):
        """Missing user_id should raise ValueError at fetch time."""
        fetcher.user_id = ""
        with pytest.raises(ValueError, match="user_id"):
            fetcher.fetch()

    def test_401_raises_fetch_error(self, fetcher):
        """A 401 Unauthorized should raise FetchError."""
        with patch("httpx.Client") as mock_client_cls:
            mock_client = MagicMock()
            mock_client.__enter__ = MagicMock(return_value=mock_client)
            mock_client.__exit__ = MagicMock(return_value=False)
            mock_resp = MagicMock()
            mock_resp.status_code = 401
            mock_resp.text = "Unauthorized"
            mock_resp.raise_for_status.side_effect = httpx.HTTPStatusError(
                "401", request=MagicMock(), response=mock_resp
            )
            mock_client.get.return_value = mock_resp
            mock_client_cls.return_value = mock_client

            with pytest.raises(FetchError, match="401"):
                fetcher.fetch()

    def test_timeout_raises_fetch_error(self, fetcher):
        """A timeout should raise FetchError."""
        with patch("httpx.Client") as mock_client_cls:
            mock_client = MagicMock()
            mock_client.__enter__ = MagicMock(return_value=mock_client)
            mock_client.__exit__ = MagicMock(return_value=False)
            mock_client.get.side_effect = httpx.ReadTimeout("timeout")
            mock_client_cls.return_value = mock_client

            with pytest.raises(FetchError, match="timeout"):
                fetcher.fetch()

    def test_successful_fetch_returns_bookmarks(self, fetcher):
        """Happy path: returns a list of XBookmark objects."""
        with patch("httpx.Client") as mock_client_cls:
            mock_client = MagicMock()
            mock_client.__enter__ = MagicMock(return_value=mock_client)
            mock_client.__exit__ = MagicMock(return_value=False)
            mock_resp = MagicMock()
            mock_resp.raise_for_status.return_value = None
            mock_resp.json.return_value = {
                "data": [
                    {
                        "id": "123",
                        "text": "BTC to the moon",
                        "author_id": "456",
                        "created_at": "2026-03-01T12:00:00Z",
                    }
                ],
                "includes": {
                    "users": [{"id": "456", "username": "trader99"}],
                },
                "meta": {},
            }
            mock_client.get.return_value = mock_resp
            mock_client_cls.return_value = mock_client

            bookmarks = fetcher.fetch(max_results=10)
            assert len(bookmarks) == 1
            assert bookmarks[0].text == "BTC to the moon"
            assert bookmarks[0].author == "trader99"
