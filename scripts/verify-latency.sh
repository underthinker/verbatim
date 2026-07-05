#!/usr/bin/env bash
# Portable raw-latency verification for M1 acceptance criterion (docs/ROADMAP.md:27, issue #16).
#
# Measures p50/p95 stop-to-text wall time for the recorded 10 s fixture through a
# resident whisper.cpp model and asserts p50 < 800 ms. This is the same bench the
# CI runs, packaged as a one-command check so the reference Windows/Apple Silicon
# hardware measurement is turnkey.
#
# Usage:
#   scripts/verify-latency.sh [MODEL_PATH]
#
# If MODEL_PATH is omitted the script uses $VERBATIM_WHISPER_MODEL, else downloads
# ggml-base.en into ~/whisper-models (matching CI). Override the budget or sample
# count with VERBATIM_BENCH_MAX_P50_MS / VERBATIM_BENCH_ITERATIONS.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

MODEL_DIR="${HOME}/whisper-models"
MODEL_NAME="ggml-base.en.bin"
MODEL_URL="https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin"

MODEL="${1:-${VERBATIM_WHISPER_MODEL:-}}"
if [ -z "$MODEL" ]; then
  MODEL="${MODEL_DIR}/${MODEL_NAME}"
  if [ ! -f "$MODEL" ]; then
    echo "==> Downloading resident model to ${MODEL}"
    mkdir -p "$MODEL_DIR"
    curl -sSfL -o "$MODEL" "$MODEL_URL"
  fi
fi

if [ ! -f "$MODEL" ]; then
  echo "error: model not found at ${MODEL}" >&2
  exit 1
fi

echo "==> Model:      ${MODEL}"
echo "==> Budget:     p50 < ${VERBATIM_BENCH_MAX_P50_MS:-800} ms"
echo "==> Iterations: ${VERBATIM_BENCH_ITERATIONS:-20}"
echo "==> Running resident-model latency bench (excludes load time by design)..."
echo

VERBATIM_WHISPER_MODEL="$MODEL" \
VERBATIM_BENCH_REQUIRE=1 \
VERBATIM_BENCH_BASELINE="${VERBATIM_BENCH_BASELINE:-}" \
VERBATIM_BENCH_MAX_P50_MS="${VERBATIM_BENCH_MAX_P50_MS:-800}" \
  cargo bench --locked -p verbatim-engines --features whisper-cpp --bench latency

echo
echo "==> Latency check passed. Record the printed p50/p95 in issue #16."
