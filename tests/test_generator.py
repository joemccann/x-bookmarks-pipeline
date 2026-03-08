"""Tests for PineScriptGenerator — OpenAI-backed, StrategyPlan input."""
from __future__ import annotations

import pytest
from unittest.mock import MagicMock

from src.generators.pinescript_generator import PineScriptGenerator, GenerationError
from src.planners.strategy_planner import StrategyPlan
from src.clients.openai_client import OpenAIClient
from src.clients.base_client import LLMResponse, ClientError
from src.config import OPENAI_MODEL


@pytest.fixture
def mock_openai_client():
    return MagicMock(spec=OpenAIClient)


@pytest.fixture
def generator(mock_openai_client):
    mock_openai_client.model = OPENAI_MODEL
    return PineScriptGenerator(client=mock_openai_client)


@pytest.fixture
def sample_plan():
    return StrategyPlan(
        tweet_id="gen_test_1",
        script_type="strategy",
        title="BTC Breakout Strategy",
        ticker="BTCUSDT",
        direction="long",
        timeframe="D",
        indicators=["RSI", "EMA"],
        entry_conditions=["RSI crosses above 30"],
        exit_conditions=["RSI crosses below 70"],
        risk_management={"stop_loss_type": "percentage", "stop_loss_value": 2.0},
        key_levels={"entry": 42000, "stop_loss": 40000},
        author="testuser",
        tweet_date="2026-03-01",
        raw_tweet_text="BTC breakout above $42k",
    )


class TestPineScriptGenerator:
    def test_successful_generation(self, generator, mock_openai_client, sample_plan):
        fake_pine = "//@version=6\nstrategy('BTC Breakout', overlay=true)\n"
        mock_openai_client.chat.return_value = LLMResponse(
            content=f"```pinescript\n{fake_pine}```"
        )
        result = generator.generate(sample_plan)
        assert "//@version=6" in result
        assert "strategy(" in result

    def test_api_error_raises_generation_error(self, generator, mock_openai_client, sample_plan):
        mock_openai_client.chat.side_effect = ClientError("API timeout")
        with pytest.raises(GenerationError, match="Pine Script generation failed"):
            generator.generate(sample_plan)

    def test_extract_pinescript_from_fenced_block(self, generator):
        response = "Here's the code:\n```pinescript\n//@version=6\nstrategy('test')\n```\nDone."
        result = generator._extract_pinescript(response)
        assert result.startswith("//@version=6")
        assert "Here's the code" not in result

    def test_extract_pinescript_raw_version(self, generator):
        response = "//@version=6\nstrategy('test')"
        result = generator._extract_pinescript(response)
        assert result == response

    def test_extract_pinescript_fallback(self, generator):
        response = "some random text without code blocks"
        result = generator._extract_pinescript(response)
        assert result == response

    def test_indicator_plan_prompt(self, generator, mock_openai_client):
        plan = StrategyPlan(
            tweet_id="gen_test_2",
            script_type="indicator",
            title="BTC Levels",
            ticker="BTCUSDT",
            indicators=["EMA"],
        )
        mock_openai_client.chat.return_value = LLMResponse(
            content="```pinescript\n//@version=6\nindicator('test')\n```"
        )
        result = generator.generate(plan)
        # Verify the prompt included indicator instructions
        call_args = mock_openai_client.chat.call_args
        messages = call_args.kwargs.get("messages") or call_args[1].get("messages")
        user_msg = [m for m in messages if m["role"] == "user"][0]
        assert "indicator" in user_msg["content"].lower()

    def test_build_user_prompt_includes_plan_fields(self, generator, sample_plan):
        prompt = generator._build_user_prompt(sample_plan)
        assert "BTCUSDT" in prompt
        assert "testuser" in prompt
        assert "RSI" in prompt
        assert "entry" in prompt.lower()
