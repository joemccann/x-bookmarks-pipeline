"""
Indicator registry — maps name → indicator module with a run() function.

Each indicator module must expose:
    run(db_path, verbose) -> list[dict]   # emits rows to signals table
    NAME: str
    DESCRIPTION: str
"""
from __future__ import annotations

from trading.indicators import move_psp_spread

REGISTRY: dict[str, object] = {
    move_psp_spread.NAME: move_psp_spread,
}
