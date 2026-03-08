"""Tests for BookmarkClassifier — finance tweet detected, non-finance categorized, image fallback."""
from __future__ import annotations

import json
import pytest
from unittest.mock import patch, MagicMock

from src.classifiers.finance_classifier import (
    BookmarkClassifier,
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
    return BookmarkClassifier(client=mock_xai_client)


class TestBookmarkClassification:
    def test_finance_tweet_detected_via_text(self, classifier, mock_xai_client):
        mock_xai_client.chat.return_value = LLMResponse(
            content=json.dumps({
                "is_finance": True,
                "confidence": 0.95,
                "category": "finance",
                "subcategory": "crypto",
                "has_trading_pattern": True,
                "has_visual_data": False,
                "detected_topic": "crypto",
                "summary": "BTC breakout setup",
            })
        )
        result = classifier.classify("t1", "BTC breakout above $42k")
        assert result.is_finance
        assert result.classification_source == "text"
        assert result.confidence == 0.95
        assert result.category == "finance"
        assert result.subcategory == "crypto"
        assert result.detected_topic == "crypto"

    def test_non_finance_tweet_categorized(self, classifier, mock_xai_client):
        mock_xai_client.chat.return_value = LLMResponse(
            content=json.dumps({
                "is_finance": False,
                "confidence": 0.9,
                "category": "other",
                "subcategory": "general",
                "has_trading_pattern": False,
                "has_visual_data": False,
                "detected_topic": "cooking",
                "summary": "Cooking recipe discussion",
            })
        )
        result = classifier.classify("t2", "Best pasta recipe ever")
        assert not result.is_finance
        assert result.classification_source == "text"
        assert result.category == "other"
        assert result.subcategory == "general"

    def test_technology_tweet_categorized(self, classifier, mock_xai_client):
        mock_xai_client.chat.return_value = LLMResponse(
            content=json.dumps({
                "is_finance": False,
                "confidence": 0.92,
                "category": "technology",
                "subcategory": "AI",
                "has_trading_pattern": False,
                "has_visual_data": False,
                "detected_topic": "AI",
                "summary": "New GPT model release discussion",
            })
        )
        result = classifier.classify("t_tech", "The new GPT model is impressive")
        assert not result.is_finance
        assert result.category == "technology"
        assert result.subcategory == "AI"

    def test_image_fallback_when_text_is_not_finance(self, classifier, mock_xai_client):
        """When text is non-finance but images contain charts, classify via images."""
        mock_xai_client.chat.return_value = LLMResponse(
            content=json.dumps({
                "is_finance": False,
                "confidence": 0.1,
                "category": "other",
                "subcategory": "general",
                "has_trading_pattern": False,
                "has_visual_data": False,
                "detected_topic": "none",
                "summary": "Unclear text",
            })
        )
        mock_xai_client.chat_with_vision.return_value = LLMResponse(
            content=json.dumps({
                "is_finance": True,
                "confidence": 0.85,
                "category": "finance",
                "subcategory": "crypto",
                "has_trading_pattern": True,
                "has_visual_data": True,
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
        assert result.category == "finance"
        assert result.has_visual_data

    def test_non_finance_image_with_visual_data(self, classifier, mock_xai_client):
        """Non-finance images with charts/graphs should set has_visual_data."""
        mock_xai_client.chat.return_value = LLMResponse(
            content=json.dumps({
                "is_finance": False,
                "confidence": 0.8,
                "category": "science",
                "subcategory": "climate",
                "has_trading_pattern": False,
                "has_visual_data": False,
                "detected_topic": "climate",
                "summary": "Climate data discussion",
            })
        )
        mock_xai_client.chat_with_vision.return_value = LLMResponse(
            content=json.dumps({
                "is_finance": False,
                "confidence": 0.7,
                "category": "science",
                "subcategory": "climate",
                "has_trading_pattern": False,
                "has_visual_data": True,
                "detected_topic": "climate",
                "summary": "Temperature trend graph",
            })
        )
        result = classifier.classify(
            "t_vis", "Global temperatures over time",
            image_urls=["https://example.com/graph.png"]
        )
        assert not result.is_finance
        assert result.has_visual_data

    def test_classification_error_on_api_failure(self, classifier, mock_xai_client):
        mock_xai_client.chat.side_effect = ClientError("API timeout")
        with pytest.raises(ClassificationError, match="Text classification failed"):
            classifier.classify("t4", "some text")

    def test_malformed_json_returns_non_finance(self, classifier, mock_xai_client):
        mock_xai_client.chat.return_value = LLMResponse(content="not json at all")
        result = classifier.classify("t5", "some text")
        assert not result.is_finance
        assert result.category == "other"  # defaults

    def test_result_stores_metadata(self, classifier, mock_xai_client):
        mock_xai_client.chat.return_value = LLMResponse(
            content=json.dumps({
                "is_finance": True,
                "confidence": 0.9,
                "category": "finance",
                "subcategory": "equities",
                "has_trading_pattern": True,
                "has_visual_data": False,
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
        assert result.category == "finance"
        assert result.subcategory == "equities"

    def test_backward_compatible_alias(self):
        """FinanceClassifier should still work as an alias."""
        assert FinanceClassifier is BookmarkClassifier
