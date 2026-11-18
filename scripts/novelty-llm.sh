#!/usr/bin/env bash
# The paper §9 LLM comparison on a local Ollama (Tier A), in chunks.
# Works from any folder: it finds the repo from its own location, and
# every path it uses is inside the repo, so moving the checkout is fine.
#
#   scripts/novelty-llm.sh [SEEDS=10]
#   MODEL (default qwen2.5:3b-instruct) and OLLAMA_BASE_URL pass through.
#
# Resumable: every reply is saved to runs/novelty-llm-<model>.jsonl as it
# arrives, so after Ctrl-C the same command replays those in seconds and
# carries on. Results go to results/novelty-llm-<model>-<seeds>/.
# Start Ollama first, on the cores you can spare, e.g.
#   OLLAMA_NUM_PARALLEL=1 OLLAMA_MAX_LOADED_MODELS=1 taskset -c 0-5 nice -n 10 ollama serve
set -euo pipefail

seeds=${1:-10}
model=${MODEL:-qwen2.5:3b-instruct}
url=${OLLAMA_BASE_URL:-http://localhost:11434}
if ! [[ $seeds =~ ^[1-9][0-9]*$ ]]; then
  echo "SEEDS must be a whole number, 1 or more" >&2
  exit 2
fi
cd "$(dirname "${BASH_SOURCE[0]}")/.."

if ! curl -fsS "$url/api/tags" >/dev/null 2>&1; then
  echo "Ollama is not answering at $url; start 'ollama serve' first" >&2
  exit 1
fi
if ! curl -fsS "$url/api/tags" | grep -q "\"$model\""; then
  echo "model $model is not pulled; run: ollama pull $model" >&2
  exit 1
fi

cargo build --release -j 4 -p runner
tag=${model//[:\/]/-}
mkdir -p runs
exec env OLLAMA_MODEL="$model" OLLAMA_BASE_URL="$url" nice -n 5 \
  ./target/release/runner run --experiment configs/experiments/novelty-llm.toml \
  --tier local --seeds "$seeds" \
  --cache "runs/novelty-llm-$tag.jsonl" --out "results/novelty-llm-$tag-$seeds"
