"""
Strategy registry — maps name → strategy module with a run() function.

Each strategy module must expose:
    run(db_path, verbose) -> dict    # {"signals": [...], "backtest": {...}}
    NAME: str
    DESCRIPTION: str
"""
from __future__ import annotations

from trading.strategies import vix_vvix_mean_reversion

REGISTRY: dict[str, object] = {
    vix_vvix_mean_reversion.NAME: vix_vvix_mean_reversion,
}
