#!/usr/bin/env bash
# Architecture gate: keeps the codebase's shape from drifting.
# Run locally with `scripts/check-architecture.sh`; CI runs it on every PR.
# Rules and reasons live in docs/ARCHITECTURE.md. To change a rule, change
# that doc and this script in the same PR.
set -euo pipefail
cd "$(dirname "$0")/.."

fail=0
err() { echo "::error::$*"; fail=1; }
warn() { echo "::warning::$*"; }

# ---------------------------------------------------------------------------
# 1. Crate dependency direction (docs/ARCHITECTURE.md "Layers").
#    Each workspace crate may depend only on the workspace crates listed here.
#    A new crate must be added here, so its place in the layers is a decision.
# ---------------------------------------------------------------------------
declare -A allowed=(
  [host-api]=""
  [cognitive-core]="host-api"
  [sim]="host-api"
  [llm]="host-api"
  [baselines]="host-api llm"
  [runner]="host-api cognitive-core sim baselines llm"
)
meta=$(cargo metadata --format-version 1 --no-deps)
members=$(jq -r '.packages[].name' <<<"$meta")
while read -r pkg; do
  if [[ -z ${allowed[$pkg]+set} ]]; then
    err "crate '$pkg' is not in the layer table in scripts/check-architecture.sh"
    continue
  fi
  deps=$(jq -r --arg p "$pkg" \
    '.packages[] | select(.name==$p) | .dependencies[] | select(.kind==null) | .name' <<<"$meta")
  for d in $deps; do
    grep -qx "$d" <<<"$members" || continue          # external crate: fine
    [[ " ${allowed[$pkg]} " == *" $d "* ]] ||
      err "layer violation: '$pkg' depends on '$d' (allowed: ${allowed[$pkg]:-none})"
  done
done <<<"$members"

# ---------------------------------------------------------------------------
# 2. The brain is pure and deterministic (spec §31.3): no IO, no wall clock,
#    no OS randomness, no hash-ordered collections.
# ---------------------------------------------------------------------------
pure_banned='std::fs|std::net|std::env|std::process|std::thread|SystemTime|Instant::now|thread_rng|rand::rng\(|OsRng|println!|eprintln!|print!|dbg!|HashMap|HashSet'
if hits=$(grep -rnE "$pure_banned" crates/core/src crates/host-api/src); then
  err "impure code in the brain or the contract (see docs/ARCHITECTURE.md 'Purity'):"
  echo "$hits"
fi
# The sim must also be deterministic; hash sets are allowed only for lookups,
# never iterated. Iterating one makes runs differ between machines.
if hits=$(grep -rnE 'std::fs|std::net|SystemTime|Instant::now|thread_rng|OsRng|dbg!' crates/sim/src crates/baselines/src); then
  err "non-deterministic code in sim or baselines:"
  echo "$hits"
fi

# ---------------------------------------------------------------------------
# 3. File size budget. Big files are where duplication and tangles start.
#    Source files: warn above 400 lines, fail above 600.
#    Test files:   warn above 600 lines, fail above 900.
#    Files over the hard limit today are listed in scripts/size-budget.txt
#    with a cap. Caps only go down: when you shrink a file, lower its cap.
# ---------------------------------------------------------------------------
declare -A cap=()
while read -r path n _; do
  [[ -z $path || $path == \#* ]] && continue
  cap[$path]=$n
done < scripts/size-budget.txt

check_size() { # path soft hard
  local f=$1 soft=$2 hard=$3 n
  n=$(wc -l <"$f")
  if [[ -n ${cap[$f]+set} ]]; then
    if (( n > cap[$f] )); then err "$f has $n lines, over its budget cap of ${cap[$f]} (scripts/size-budget.txt). Split it."
    elif (( n <= hard )); then warn "$f is back under $hard lines: remove it from scripts/size-budget.txt"
    fi
  elif (( n > hard )); then err "$f has $n lines (limit $hard). Split it by responsibility."
  elif (( n > soft )); then warn "$f has $n lines (soft limit $soft). Plan a split before it grows."
  fi
}
while IFS= read -r f; do
  case $f in
    */tests/*) check_size "$f" 600 900 ;;
    *)         check_size "$f" 400 600 ;;
  esac
done < <(git ls-files 'crates/*.rs' 'bot/src/*.ts' 'bindings/*.rs' 'bindings/*.ts')
for f in "${!cap[@]}"; do
  [[ -f $f ]] || warn "scripts/size-budget.txt lists $f, which no longer exists: remove it"
done

if (( fail )); then
  echo "Architecture check failed. See docs/ARCHITECTURE.md."
  exit 1
fi
echo "Architecture check passed."
