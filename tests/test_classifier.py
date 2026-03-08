"""Tests for FinanceClassifier — finance tweet detected, non-finance skipped, image fallback."""
from __future__ import annotations

import json
import pytest
from unittest.mock import patch, MagicMock

from src.classifiers.finance_classifier import (
    FinanceClassifier,
    ClassificationResult,
    ClassificationError,
)
from src.clients.xai_client import XAIClient
from src.clients.base_client import LLMResponse, ClientError


@pytest.fixture
def mock_xai_client():
    return MagicMock(spec=XAIClient)


@pytest.fixture
def classifier(mock_xai_client):
    return FinanceClassifier(client=mock_xai_client)


class TestFinanceClassification:
    def test_finance_tweet_detected_via_text(self, classifier, mock_xai_client):
        mock_xai_client.chat.return_value = LLMResponse(
            content=json.dumps({
                "is_finance": True,
                "confidence": 0.95,
                "has_trading_pattern": True,
                "detected_topic": "crypto",
                "summary": "BTC breakout setup",
            })
        )
        result = classifier.classify("t1", "BTC breakout above $42k")
        assert result.is_finance
        assert result.classification_source == "text"
        assert result.confidence == 0.95
        assert result.detected_topic == "crypto"

    def test_non_finance_tweet_skipped(self, classifier, mock_xai_client):
        mock_xai_client.chat.return_value = LLMResponse(
            content=json.dumps({
                "is_finance": False,
                "confidence": 0.1,
                "has_trading_pattern": False,
                "detected_topic": "none",
                "summary": "Cooking recipe discussion",
            })
        )
        result = classifier.classify("t2", "Best pasta recipe ever")
        assert not result.is_finance
        assert result.classification_source == "none"

    def test_image_fallback_when_text_is_not_finance(self, classifier, mock_xai_client):
        """When text is non-finance but images contain charts, classify via images."""
        # First call (text) returns non-finance
        # Second call (vision) returns finance
        mock_xai_client.chat.return_value = LLMResponse(
            content=json.dumps({
                "is_finance": False,
                "confidence": 0.1,
                "has_trading_pattern": False,
                "detected_topic": "none",
                "summary": "Unclear text",
            })
        )
        mock_xai_client.chat_with_vision.return_value = LLMResponse(
            content=json.dumps({
                "is_finance": True,
                "confidence": 0.85,
                "has_trading_pattern": True,
                "detected_topic": "crypto",
                "summary": "BTC chart with bull flag pattern",
            })
        )
        result = classifier.classify(
            "t3", "Check this out!", image_urls=["https://example.com/chart.png"]
        )
        assert result.is_finance
        assert result.classification_source == "image"
        assert result.confidence == 0.85

    def test_classification_error_on_api_failure(self, classifier, mock_xai_client):
        mock_xai_client.chat.side_effect = ClientError("API timeout")
        with pytest.raises(ClassificationError, match="Text classification failed"):
            classifier.classify("t4", "some text")

    def test_malformed_json_returns_non_finance(self, classifier, mock_xai_client):
        mock_xai_client.chat.return_value = LLMResponse(content="not json at all")
        result = classifier.classify("t5", "some text")
        assert not result.is_finance

    def test_result_stores_metadata(self, classifier, mock_xai_client):
        mock_xai_client.chat.return_value = LLMResponse(
            content=json.dumps({
                "is_finance": True,
                "confidence": 0.9,
                "has_trading_pattern": True,
                "detected_topic": "equities",
                "summary": "AAPL earnings play",
            })
        )
        result = classifier.classify(
            "t6", "AAPL calls $200 strike",
            image_urls=["https://example.com/img.png"],
        )
        assert result.tweet_id == "t6"
        assert result.raw_text == "AAPL calls $200 strike"
        assert result.image_urls == ["https://example.com/img.png"]
