#!/usr/bin/env bash
# Renders benchmarks/eval-suite/RESULTS.md from .last-run.json
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
JSON="${1:-${ROOT}/benchmarks/eval-suite/.last-run.json}"
OUT="${ROOT}/benchmarks/eval-suite/RESULTS.md"

python3 "${ROOT}/benchmarks/eval-suite/results_tool.py" render "$JSON" "$OUT"
