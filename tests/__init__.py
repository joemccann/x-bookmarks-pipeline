"""Test package marker."""

from pathlib import Path

TRADING_TESTS = Path(__file__).resolve().parent.parent / "trading" / "tests"
if str(TRADING_TESTS) not in __path__:
    __path__.append(str(TRADING_TESTS))  # type: ignore[attr-defined]
