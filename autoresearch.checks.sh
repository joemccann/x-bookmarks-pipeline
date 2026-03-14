#!/bin/bash
set -euo pipefail
# Run all 151 pipeline tests — only show failures
python3 -m pytest tests/ -x -q --tb=short 2>&1 | tail -50
