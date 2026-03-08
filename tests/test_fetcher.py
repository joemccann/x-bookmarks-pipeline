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


class TestLongFormContent:
    """note_tweet and entities support for long-form posts and articles."""

    def test_note_tweet_preferred_over_text(self, fetcher):
        """When note_tweet is present, its text should be used instead of truncated text."""
        with patch("httpx.Client") as mock_client_cls:
            mock_client = MagicMock()
            mock_client.__enter__ = MagicMock(return_value=mock_client)
            mock_client.__exit__ = MagicMock(return_value=False)
            mock_resp = MagicMock()
            mock_resp.raise_for_status.return_value = None
            mock_resp.json.return_value = {
                "data": [
                    {
                        "id": "long1",
                        "text": "This is truncated... https://t.co/abc123",
                        "author_id": "789",
                        "created_at": "2026-03-08T10:00:00Z",
                        "note_tweet": {
                            "text": "This is the full long-form content of the tweet that exceeds 280 characters. It contains detailed analysis of market conditions and trading setups."
                        },
                    }
                ],
                "includes": {
                    "users": [{"id": "789", "username": "longwriter"}],
                },
                "meta": {},
            }
            mock_client.get.return_value = mock_resp
            mock_client_cls.return_value = mock_client

            bookmarks = fetcher.fetch(max_results=10)
            assert len(bookmarks) == 1
            assert "full long-form content" in bookmarks[0].text
            assert "truncated" not in bookmarks[0].text

    def test_regular_tweet_without_note_tweet(self, fetcher):
        """Regular tweets without note_tweet should use the text field."""
        with patch("httpx.Client") as mock_client_cls:
            mock_client = MagicMock()
            mock_client.__enter__ = MagicMock(return_value=mock_client)
            mock_client.__exit__ = MagicMock(return_value=False)
            mock_resp = MagicMock()
            mock_resp.raise_for_status.return_value = None
            mock_resp.json.return_value = {
                "data": [
                    {
                        "id": "short1",
                        "text": "Short regular tweet",
                        "author_id": "789",
                        "created_at": "2026-03-08T10:00:00Z",
                    }
                ],
                "includes": {
                    "users": [{"id": "789", "username": "shortwriter"}],
                },
                "meta": {},
            }
            mock_client.get.return_value = mock_resp
            mock_client_cls.return_value = mock_client

            bookmarks = fetcher.fetch(max_results=10)
            assert bookmarks[0].text == "Short regular tweet"
            assert not bookmarks[0].is_article

    def test_article_detected_from_entities(self, fetcher):
        """Articles should be detected from entities.urls expanded_url pattern."""
        with patch("httpx.Client") as mock_client_cls:
            mock_client = MagicMock()
            mock_client.__enter__ = MagicMock(return_value=mock_client)
            mock_client.__exit__ = MagicMock(return_value=False)
            mock_resp = MagicMock()
            mock_resp.raise_for_status.return_value = None
            mock_resp.json.return_value = {
                "data": [
                    {
                        "id": "art1",
                        "text": "https://t.co/abc123",
                        "author_id": "789",
                        "created_at": "2026-03-08T10:00:00Z",
                        "entities": {
                            "urls": [
                                {
                                    "url": "https://t.co/abc123",
                                    "expanded_url": "https://x.com/i/articles/987654321",
                                    "display_url": "x.com/i/articles/987654321",
                                }
                            ]
                        },
                    }
                ],
                "includes": {
                    "users": [{"id": "789", "username": "articlewriter"}],
                },
                "meta": {},
            }
            mock_client.get.return_value = mock_resp
            mock_client_cls.return_value = mock_client

            bookmarks = fetcher.fetch(max_results=10)
            assert bookmarks[0].is_article
            assert bookmarks[0].article_url == "https://x.com/i/articles/987654321"

    def test_expanded_urls_extracted(self, fetcher):
        """All expanded URLs from entities should be captured."""
        with patch("httpx.Client") as mock_client_cls:
            mock_client = MagicMock()
            mock_client.__enter__ = MagicMock(return_value=mock_client)
            mock_client.__exit__ = MagicMock(return_value=False)
            mock_resp = MagicMock()
            mock_resp.raise_for_status.return_value = None
            mock_resp.json.return_value = {
                "data": [
                    {
                        "id": "urls1",
                        "text": "Check these links https://t.co/a https://t.co/b",
                        "author_id": "789",
                        "created_at": "2026-03-08T10:00:00Z",
                        "entities": {
                            "urls": [
                                {"url": "https://t.co/a", "expanded_url": "https://example.com/page1"},
                                {"url": "https://t.co/b", "expanded_url": "https://example.com/page2"},
                            ]
                        },
                    }
                ],
                "includes": {
                    "users": [{"id": "789", "username": "linksharer"}],
                },
                "meta": {},
            }
            mock_client.get.return_value = mock_resp
            mock_client_cls.return_value = mock_client

            bookmarks = fetcher.fetch(max_results=10)
            assert len(bookmarks[0].expanded_urls) == 2
            assert "https://example.com/page1" in bookmarks[0].expanded_urls
            assert not bookmarks[0].is_article  # not an article URL

    def test_api_requests_note_tweet_and_entities_fields(self, fetcher):
        """The API request should include note_tweet and entities in tweet.fields."""
        with patch("httpx.Client") as mock_client_cls:
            mock_client = MagicMock()
            mock_client.__enter__ = MagicMock(return_value=mock_client)
            mock_client.__exit__ = MagicMock(return_value=False)
            mock_resp = MagicMock()
            mock_resp.raise_for_status.return_value = None
            mock_resp.json.return_value = {"data": [], "meta": {}}
            mock_client.get.return_value = mock_resp
            mock_client_cls.return_value = mock_client

            fetcher.fetch(max_results=10)

            call_kwargs = mock_client.get.call_args
            params = call_kwargs.kwargs.get("params") or call_kwargs[1].get("params")
            tweet_fields = params["tweet.fields"]
            assert "note_tweet" in tweet_fields
            assert "entities" in tweet_fields
