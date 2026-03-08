"""Tests for StrategyPlanner — strategy vs indicator detection, plan field population."""
from __future__ import annotations

import json
import pytest
from unittest.mock import MagicMock

from src.planners.strategy_planner import StrategyPlanner, StrategyPlan, PlanningError
from src.classifiers.finance_classifier import ClassificationResult
from src.clients.anthropic_client import AnthropicClient
from src.clients.base_client import LLMResponse, ClientError


@pytest.fixture
def mock_anthropic_client():
    return MagicMock(spec=AnthropicClient)


@pytest.fixture
def planner(mock_anthropic_client):
    return StrategyPlanner(client=mock_anthropic_client)


@pytest.fixture
def finance_classification():
    return ClassificationResult(
        tweet_id="plan_test_1",
        is_finance=True,
        confidence=0.95,
        classification_source="text",
        has_trading_pattern=True,
        detected_topic="crypto",
        summary="BTC breakout with RSI confirmation",
        raw_text="BTC breakout above $42k, RSI oversold on 4h",
        image_urls=[],
    )


class TestStrategyPlanner:
    def test_strategy_plan_from_classification(self, planner, mock_anthropic_client, finance_classification):
        mock_anthropic_client.chat.return_value = LLMResponse(
            content=json.dumps({
                "script_type": "strategy",
                "title": "BTC Breakout Strategy",
                "ticker": "BTCUSDT",
                "direction": "long",
                "timeframe": "240",
                "indicators": ["RSI", "EMA"],
                "entry_conditions": ["RSI crosses above 30"],
                "exit_conditions": ["RSI crosses below 70"],
                "risk_management": {"stop_loss_type": "percentage", "stop_loss_value": 2.0},
                "key_levels": {"entry": 42000, "stop_loss": 40000},
                "pattern": "breakout",
                "visual_signals": ["plotshape for entries"],
                "rationale": "RSI oversold bounce",
            })
        )
        plan = planner.plan(finance_classification, author="testuser", tweet_date="2026-03-01")
        assert isinstance(plan, StrategyPlan)
        assert plan.script_type == "strategy"
        assert plan.ticker == "BTCUSDT"
        assert plan.direction == "long"
        assert "RSI" in plan.indicators
        assert plan.tweet_id == "plan_test_1"
        assert plan.author == "testuser"

    def test_indicator_plan_detection(self, planner, mock_anthropic_client, finance_classification):
        mock_anthropic_client.chat.return_value = LLMResponse(
            content=json.dumps({
                "script_type": "indicator",
                "title": "BTC Support/Resistance Overlay",
                "ticker": "BTCUSDT",
                "direction": "both",
                "timeframe": "D",
                "indicators": ["EMA", "Volume"],
                "key_levels": {"support": 39000, "resistance": 46000},
                "visual_signals": ["hline for levels", "plot for EMA"],
                "rationale": "Key levels visualization",
            })
        )
        plan = planner.plan(finance_classification)
        assert plan.script_type == "indicator"
        assert plan.entry_conditions == []
        assert plan.risk_management == {}

    def test_planning_error_on_api_failure(self, planner, mock_anthropic_client, finance_classification):
        mock_anthropic_client.chat.side_effect = ClientError("Claude API error")
        with pytest.raises(PlanningError, match="Planning failed"):
            planner.plan(finance_classification)

    def test_planning_error_on_bad_json(self, planner, mock_anthropic_client, finance_classification):
        mock_anthropic_client.chat.return_value = LLMResponse(content="not json")
        with pytest.raises(PlanningError, match="Failed to parse"):
            planner.plan(finance_classification)

    def test_plan_includes_chart_description(self, planner, mock_anthropic_client, finance_classification):
        mock_anthropic_client.chat.return_value = LLMResponse(
            content=json.dumps({
                "script_type": "strategy",
                "title": "Test",
                "ticker": "BTCUSDT",
                "direction": "long",
                "timeframe": "D",
                "indicators": ["RSI"],
                "rationale": "test",
            })
        )
        plan = planner.plan(
            finance_classification,
            chart_description="Bull flag pattern on 4h chart",
        )
        assert plan.chart_description == "Bull flag pattern on 4h chart"
