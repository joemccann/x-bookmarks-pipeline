"""Tests for PineScriptValidator — strategy and indicator support."""
from __future__ import annotations

import pytest

from src.validators.pinescript_validator import PineScriptValidator
from tests.conftest import VALID_STRATEGY_PINE, VALID_INDICATOR_PINE


@pytest.fixture
def validator():
    return PineScriptValidator()


class TestValidatorStrategy:
    def test_valid_strategy_passes(self, validator):
        result = validator.validate(VALID_STRATEGY_PINE)
        assert result.valid
        assert result.errors == []

    def test_missing_version_fails(self, validator):
        code = VALID_STRATEGY_PINE.replace("//@version=6", "")
        result = validator.validate(code)
        assert not result.valid
        assert any("version" in e.lower() for e in result.errors)

    def test_missing_strategy_fails(self, validator):
        code = VALID_STRATEGY_PINE.replace(
            'strategy("BTC Breakout Strategy", overlay=true, default_qty_type=strategy.percent_of_equity, default_qty_value=10)',
            ""
        )
        result = validator.validate(code)
        assert not result.valid

    def test_missing_exit_fails(self, validator):
        code = VALID_STRATEGY_PINE.replace(
            'strategy.exit("Exit", "Long", stop=stop_loss, limit=take_profit)', ""
        )
        result = validator.validate(code)
        assert not result.valid
        assert any("strategy.exit" in e for e in result.errors)


class TestValidatorIndicator:
    def test_valid_indicator_passes(self, validator):
        result = validator.validate(VALID_INDICATOR_PINE, script_type="indicator")
        assert result.valid
        assert result.errors == []

    def test_indicator_without_strategy_exit_passes(self, validator):
        """Indicators should NOT require strategy.exit()."""
        result = validator.validate(VALID_INDICATOR_PINE, script_type="indicator")
        assert result.valid
        assert not any("strategy.exit" in e for e in result.errors)

    def test_indicator_missing_declaration_fails(self, validator):
        code = VALID_INDICATOR_PINE.replace(
            'indicator("BTC Support/Resistance", overlay=true)', ""
        )
        result = validator.validate(code, script_type="indicator")
        assert not result.valid
        assert any("indicator()" in e for e in result.errors)


class TestCitationHeaderCheck:
    def test_citation_with_source_keyword(self, validator):
        result = validator.validate(VALID_STRATEGY_PINE)
        assert not any("Citation" in w for w in result.warnings)

    def test_citation_with_at_sign_before_strategy(self, validator):
        code = VALID_STRATEGY_PINE.replace("// Source: @testuser", "// @testuser's idea")
        result = validator.validate(code)
        assert not any("Citation" in w for w in result.warnings)

    def test_no_citation_emits_warning(self, validator):
        code = VALID_STRATEGY_PINE.replace("// Source: @testuser", "// A trading strategy")
        result = validator.validate(code)
        assert any("Citation" in w or "citation" in w.lower() for w in result.warnings)

    def test_citation_check_without_strategy_declaration(self, validator):
        code = "//@version=6\n// just some code\ninput.float(1.0)\n"
        result = validator.validate(code)
        assert isinstance(result.errors, list)

    def test_indicator_citation_check(self, validator):
        result = validator.validate(VALID_INDICATOR_PINE, script_type="indicator")
        assert not any("Citation" in w for w in result.warnings)
