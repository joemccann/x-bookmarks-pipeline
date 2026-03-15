# Autoresearch Ideas — Token Efficiency (Session Complete)

## Summary
Achieved 63.9% token reduction (7,972 → 2,880) and 61.7% cost reduction (23.1¢ → 8.9¢ per bookmark) across 54 experiments. All low-hanging and medium-effort optimizations exhausted.

## Remaining Ideas (diminishing returns)
- **Anthropic prompt caching** — can't implement without modifying tests (test asserts `payload["system"]` is a plain string). Would save ~1.5¢/bookmark on Claude Opus calls.
- **Merge Claude vision + planning** into single API call — saves ~400 tokens + API latency but requires pipeline.py changes (off limits).
- **Pine Script template approach** — provide skeleton code with placeholders instead of generating from scratch. High complexity, risky quality trade-off.
- **Cheaper model for planning** — Claude Sonnet instead of Opus. Architectural change, not token optimization.
