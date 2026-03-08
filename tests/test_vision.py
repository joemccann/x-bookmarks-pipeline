"""Tests for ClaudeVisionAnalyzer — image analysis, download failures, API errors."""
from __future__ import annotations

import json
import pytest
from unittest.mock import patch, MagicMock

from src.generators.vision_analyzer import (
    ClaudeVisionAnalyzer,
    GrokVisionAnalyzer,
    _fetch_image_as_base64,
)
from src.clients.anthropic_client import AnthropicClient
from src.clients.base_client import LLMResponse, ClientError


@pytest.fixture
def mock_anthropic_client():
    return MagicMock(spec=AnthropicClient)


@pytest.fixture
def analyzer(mock_anthropic_client):
    return ClaudeVisionAnalyzer(client=mock_anthropic_client, timeout=5.0)


class TestClaudeVisionAnalyzer:
    def test_analyze_returns_description(self, analyzer, mock_anthropic_client):
        """Successful analysis returns the response content."""
        vision_json = json.dumps({"image_type": "chart", "asset": {"ticker": "BTCUSDT"}})
        mock_anthropic_client.chat.return_value = LLMResponse(content=vision_json)

        with patch(
            "src.generators.vision_analyzer._fetch_image_as_base64",
            return_value=("base64data", "image/png"),
        ):
            result = analyzer.analyze("https://example.com/chart.png")

        assert result == vision_json
        mock_anthropic_client.chat.assert_called_once()

    def test_analyze_empty_url_returns_empty(self, analyzer):
        """Empty URL should return empty string without API call."""
        result = analyzer.analyze("")
        assert result == ""

    def test_analyze_download_failure_returns_empty(self, analyzer, mock_anthropic_client):
        """Download failure should return empty string, not crash."""
        with patch(
            "src.generators.vision_analyzer._fetch_image_as_base64",
            side_effect=Exception("Connection refused"),
        ):
            result = analyzer.analyze("https://example.com/chart.png")

        assert result == ""
        mock_anthropic_client.chat.assert_not_called()

    def test_analyze_api_failure_returns_empty(self, analyzer, mock_anthropic_client):
        """Claude API failure should return empty string."""
        mock_anthropic_client.chat.side_effect = ClientError("API error")

        with patch(
            "src.generators.vision_analyzer._fetch_image_as_base64",
            return_value=("base64data", "image/jpeg"),
        ):
            result = analyzer.analyze("https://example.com/chart.png")

        assert result == ""

    def test_analyze_all_concatenates_results(self, analyzer, mock_anthropic_client):
        """analyze_all should concatenate results from multiple images."""
        mock_anthropic_client.chat.return_value = LLMResponse(content="chart data")

        with patch(
            "src.generators.vision_analyzer._fetch_image_as_base64",
            return_value=("base64data", "image/png"),
        ):
            result = analyzer.analyze_all([
                "https://example.com/chart1.png",
                "https://example.com/chart2.png",
            ])

        assert "chart data" in result
        assert mock_anthropic_client.chat.call_count == 2

    def test_analyze_all_skips_empty_urls(self, analyzer, mock_anthropic_client):
        """analyze_all should skip empty/falsy URLs."""
        mock_anthropic_client.chat.return_value = LLMResponse(content="data")

        with patch(
            "src.generators.vision_analyzer._fetch_image_as_base64",
            return_value=("base64data", "image/png"),
        ):
            result = analyzer.analyze_all(["https://example.com/chart.png", "", None])

        # Only the first valid URL should be analyzed
        assert mock_anthropic_client.chat.call_count == 1

    def test_analyze_sends_base64_content(self, analyzer, mock_anthropic_client):
        """Verify the message format sent to Claude includes base64 image."""
        mock_anthropic_client.chat.return_value = LLMResponse(content="{}")

        with patch(
            "src.generators.vision_analyzer._fetch_image_as_base64",
            return_value=("dGVzdGRhdGE=", "image/png"),
        ):
            analyzer.analyze("https://example.com/chart.png")

        call_kwargs = mock_anthropic_client.chat.call_args
        messages = call_kwargs.kwargs.get("messages") or call_kwargs[1].get("messages")
        content = messages[0]["content"]
        assert content[0]["type"] == "image"
        assert content[0]["source"]["data"] == "dGVzdGRhdGE="
        assert content[0]["source"]["media_type"] == "image/png"
        assert content[1]["type"] == "text"


class TestGrokVisionAlias:
    def test_alias_is_same_class(self):
        """GrokVisionAnalyzer should be the same as ClaudeVisionAnalyzer."""
        assert GrokVisionAnalyzer is ClaudeVisionAnalyzer


class TestFetchImageAsBase64:
    def test_jpeg_content_type(self):
        """JPEG content type should be normalized."""
        mock_response = MagicMock()
        mock_response.headers = {"content-type": "image/jpeg"}
        mock_response.content = b"fake-image-data"

        with patch("httpx.Client") as mock_client_cls:
            mock_client = MagicMock()
            mock_client.__enter__ = MagicMock(return_value=mock_client)
            mock_client.__exit__ = MagicMock(return_value=False)
            mock_client.get.return_value = mock_response
            mock_client_cls.return_value = mock_client

            b64, media_type = _fetch_image_as_base64("https://example.com/img.jpg")

        assert media_type == "image/jpeg"
        assert len(b64) > 0

    def test_png_content_type(self):
        """PNG content type should be normalized."""
        mock_response = MagicMock()
        mock_response.headers = {"content-type": "image/png"}
        mock_response.content = b"fake-png-data"

        with patch("httpx.Client") as mock_client_cls:
            mock_client = MagicMock()
            mock_client.__enter__ = MagicMock(return_value=mock_client)
            mock_client.__exit__ = MagicMock(return_value=False)
            mock_client.get.return_value = mock_response
            mock_client_cls.return_value = mock_client

            b64, media_type = _fetch_image_as_base64("https://example.com/img.png")

        assert media_type == "image/png"

    def test_webp_content_type(self):
        """WebP content type should be normalized."""
        mock_response = MagicMock()
        mock_response.headers = {"content-type": "image/webp"}
        mock_response.content = b"fake-webp-data"

        with patch("httpx.Client") as mock_client_cls:
            mock_client = MagicMock()
            mock_client.__enter__ = MagicMock(return_value=mock_client)
            mock_client.__exit__ = MagicMock(return_value=False)
            mock_client.get.return_value = mock_response
            mock_client_cls.return_value = mock_client

            b64, media_type = _fetch_image_as_base64("https://example.com/img.webp")

        assert media_type == "image/webp"
