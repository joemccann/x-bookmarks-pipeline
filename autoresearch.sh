#!/bin/bash
set -euo pipefail

# Measure total token usage across all pipeline prompts.
# Uses tiktoken (cl100k_base) for accurate token counting.
# Outputs METRIC lines for primary + secondary metrics.

python3 benchmark_tokens.py
