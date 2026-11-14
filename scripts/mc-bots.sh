#!/usr/bin/env bash
# Launch N brain bots (arm C) into a running Minecraft server (spec §22).
# Each bot gets its own Mineflayer process (user curiousI, link port
# 7877+I) and its own runner, seeded I. Logs go to runs/mc-<stamp>/.
#
#   scripts/mc-bots.sh [N=1] [DECISIONS=500]
#   MC_HOST, MC_PORT, MC_VERSION pass through to the bots.
#   ARM=a runs the behaviour tree instead of the brain.
#
# Needs: a server (scripts/mc-server.sh), cd bot && npm ci, and
# cargo build --release -p runner. Ctrl-C stops all.
set -euo pipefail

n=${1:-1}
decisions=${2:-500}
arm=${ARM:-c}
if ! [[ $n =~ ^[1-9][0-9]*$ ]] || ((n > 5)); then
  echo "N must be 1 to 5" >&2
  exit 2
fi
root=$(cd "$(dirname "$0")/.." && pwd)
out="$root/runs/mc-$(date +%Y%m%d-%H%M%S)"
mkdir -p "$out"
runner="$root/target/release/runner"
[[ -x $runner ]] || (cd "$root" && cargo build --release -p runner)
(cd "$root/bot" && npx tsc -p .)

# Record the server's difficulty and time with the logs, if RCON is on
# (scripts/mc-server.sh turns it on).
python3 -I "$root/scripts/rcon.py" difficulty "time query daytime" >"$out/server.txt" 2>&1 || true

pids=()
runs=()
trap 'kill "${pids[@]}" 2>/dev/null || true' EXIT INT TERM

for i in $(seq 1 "$n"); do
  port=$((7877 + i))
  MC_USER="curious$i" LINK_PORT=$port \
    node "$root/bot/dist/mineflayer/main.js" >"$out/bot$i.log" 2>&1 &
  pids+=($!)
  # Wait for the bot to join and open its link (up to 60 s).
  for _ in $(seq 1 120); do
    grep -q "ready; runner link" "$out/bot$i.log" && break
    if ! kill -0 "${pids[-1]}" 2>/dev/null; then
      echo "bot $i exited; see $out/bot$i.log" >&2
      exit 1
    fi
    sleep 0.5
  done
  "$runner" mc "$arm" --seed "$i" --addr "127.0.0.1:$port" \
    --decisions "$decisions" --log "$out/run$i.jsonl" >"$out/run$i.out" 2>&1 &
  pids+=($!)
  runs+=($!)
  echo "bot $i: link $port, logs $out/bot$i.log, $out/run$i.jsonl"
done

echo "running $n bot(s); Ctrl-C to stop"
wait "${runs[@]}" || true
tail -n 1 "$out"/run*.out
