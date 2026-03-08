"""Shared test fixtures for the x-bookmarks-pipeline test suite."""
from __future__ import annotations

import pytest

from src.classifiers.finance_classifier import ClassificationResult
from src.planners.strategy_planner import StrategyPlan


@pytest.fixture
def sample_classification():
    """A finance-positive classification result."""
    return ClassificationResult(
        tweet_id="test123",
        is_finance=True,
        confidence=0.95,
        classification_source="text",
        has_trading_pattern=True,
        detected_topic="crypto",
        summary="BTC breakout above $42k with RSI confirmation",
        raw_text="BTC breakout above $42k, RSI oversold on 4h. Target $45k, SL $40k",
        image_urls=[],
    )


@pytest.fixture
def sample_plan():
    """A strategy plan for Pine Script generation."""
    return StrategyPlan(
        tweet_id="test123",
        script_type="strategy",
        title="BTC Breakout Strategy",
        ticker="BTCUSDT",
        direction="long",
        timeframe="240",
        indicators=["RSI", "EMA"],
        indicator_params={"RSI": {"length": 14, "oversold": 30}, "EMA": {"lengths": [20, 50]}},
        entry_conditions=["RSI crosses above 30", "Price above $42k"],
        exit_conditions=["RSI crosses below 70", "Price hits $45k"],
        risk_management={
            "stop_loss_type": "fixed",
            "stop_loss_value": 40000,
            "take_profit_type": "fixed",
            "take_profit_value": 45000,
        },
        key_levels={"entry": 42000, "stop_loss": 40000, "take_profit": 45000},
        pattern="breakout",
        visual_signals=["plotshape for entries"],
        rationale="RSI oversold bounce with breakout confirmation",
        author="testuser",
        tweet_date="2026-03-01",
        raw_tweet_text="BTC breakout above $42k, RSI oversold on 4h. Target $45k, SL $40k",
        chart_description="",
    )


@pytest.fixture
def non_finance_classification():
    """A non-finance classification result."""
    return ClassificationResult(
        tweet_id="test456",
        is_finance=False,
        confidence=0.1,
        classification_source="none",
        has_trading_pattern=False,
        detected_topic="none",
        summary="Discussion about cooking recipes",
        raw_text="Just made the best pasta carbonara! Recipe in thread.",
        image_urls=[],
    )


VALID_STRATEGY_PINE = """\
//@version=6
// Source: @testuser
strategy("BTC Breakout Strategy", overlay=true, default_qty_type=strategy.percent_of_equity, default_qty_value=10)
entry = input.float(42000, title="Entry Price")
stop_loss = input.float(40000, title="Stop Loss")
take_profit = input.float(45000, title="Take Profit")
rsiLen = input.int(14, title="RSI Length")
rsiVal = ta.rsi(close, rsiLen)
if ta.crossover(close, entry) and barstate.isconfirmed
    strategy.entry("Long", strategy.long)
strategy.exit("Exit", "Long", stop=stop_loss, limit=take_profit)
plotshape(true, title="Signal", style=shape.triangleup, location=location.belowbar)
"""

VALID_INDICATOR_PINE = """\
//@version=6
// Source: @testuser
indicator("BTC Support/Resistance", overlay=true)
emaLen = input.int(20, title="EMA Length")
emaVal = ta.ema(close, emaLen)
support = input.float(39000, title="Support Level")
resistance = input.float(46000, title="Resistance Level")
plot(emaVal, title="EMA", color=color.blue)
hline(support, title="Support", color=color.green)
hline(resistance, title="Resistance", color=color.red)
plotshape(ta.crossover(close, emaVal), title="Cross Up", style=shape.triangleup, location=location.belowbar)
"""
