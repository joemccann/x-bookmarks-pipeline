"""
Pine Script v6 Validator — static checks to catch common issues before
the user pastes the strategy into TradingView.
"""

from __future__ import annotations

import re
from dataclasses import dataclass, field


@dataclass
class ValidationResult:
    valid: bool = True
    errors: list[str] = field(default_factory=list)
    warnings: list[str] = field(default_factory=list)

    def fail(self, msg: str) -> None:
        self.valid = False
        self.errors.append(msg)

    def warn(self, msg: str) -> None:
        self.warnings.append(msg)


class PineScriptValidator:
    """Run static validation rules against generated Pine Script v6 code."""

    def validate(self, code: str) -> ValidationResult:
        result = ValidationResult()

        self._check_version(code, result)
        self._check_strategy_declaration(code, result)
        self._check_inputs(code, result)
        self._check_risk_management(code, result)
        self._check_visual_signals(code, result)
        self._check_citation_header(code, result)
        self._check_no_repainting(code, result)

        return result

    # ------------------------------------------------------------------
    # Individual checks
    # ------------------------------------------------------------------

    @staticmethod
    def _check_version(code: str, r: ValidationResult) -> None:
        if not re.search(r"^//\s*@version\s*=\s*6", code, re.MULTILINE):
            r.fail("Missing or incorrect version pragma. Must start with //@version=6")

    @staticmethod
    def _check_strategy_declaration(code: str, r: ValidationResult) -> None:
        if "strategy(" not in code:
            r.fail("No strategy() declaration found.")

    @staticmethod
    def _check_inputs(code: str, r: ValidationResult) -> None:
        if "input." not in code and "input(" not in code:
            r.warn("No input.*() calls found. Strategy parameters should be user-tunable.")

    @staticmethod
    def _check_risk_management(code: str, r: ValidationResult) -> None:
        has_sl = bool(re.search(r"stop_loss|stop\.loss|sl_pct|sl_atr", code, re.IGNORECASE))
        has_tp = bool(re.search(r"take_profit|take\.profit|tp_pct|tp_atr", code, re.IGNORECASE))
        has_exit = "strategy.exit" in code

        if not has_sl:
            r.warn("No stop-loss parameter detected.")
        if not has_tp:
            r.warn("No take-profit parameter detected.")
        if not has_exit:
            r.fail("No strategy.exit() call found. Risk management exits are required.")

    @staticmethod
    def _check_visual_signals(code: str, r: ValidationResult) -> None:
        if "plotshape" not in code and "plotchar" not in code:
            r.warn("No plotshape() or plotchar() found. Visual signals are recommended.")

    @staticmethod
    def _check_citation_header(code: str, r: ValidationResult) -> None:
        if "Source" not in code and "@" not in code.split("strategy(")[0] if "strategy(" in code else "":
            r.warn("Citation header with tweet author not detected.")

    @staticmethod
    def _check_no_repainting(code: str, r: ValidationResult) -> None:
        if "request.security(" in code or "security(" in code:
            if "lookahead" not in code:
                r.warn(
                    "security()/request.security() used without explicit lookahead parameter. "
                    "This may cause repainting."
                )
