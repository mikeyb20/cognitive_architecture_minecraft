# Working in this repository

Rust brain + Rust sim world + TypeScript Mineflayer adapter for agents built on the Curiosity Architecture. Read `docs/ARCHITECTURE.md` before structural changes; it holds the reasons behind the rules below.

## Before you write code

1. **Find the home.** Brain rules go in the `crates/core` module that mirrors their 01 section. World behaviour goes in `crates/sim`. Shared names go in `host-api::ontology`. Wiring goes in `runner` only.
2. **Search before you write.** Grep for an existing helper, formula or constant first. If one exists, call it. If a near-copy exists, the third copy gets extracted (rule of three).
3. **Check the layer table** (`docs/ARCHITECTURE.md` §1). Never add a workspace dependency that isn't in it. If you think one is needed, stop and raise it.
4. **Look for the seam.** If the change adds another branch to a function that already branches on the same thing (skill, variant, OQ candidate), add a trait impl and a registry entry instead.

## Rules

- `cognitive-core` and `host-api` are pure: no IO, clock, threads, printing, OS randomness, `HashMap`/`HashSet`. Use the seeded RNG and `BTreeMap`.
- Every spec value is a default in a params struct, pinned in `tests/spec_defaults.rs`. No spec literal appears twice.
- Every public function cites its source: `01 §N`, `spec §N`, `00 §N` or decision `#N`. Never change a definition from 01; propose a `DEF CHANGE: old → new, reason` in the PR instead.
- Size limits: source files 600 lines, test files 900, functions 100 lines, complexity 25, 7 arguments. Split by responsibility; don't raise a cap in `scripts/size-budget.txt`.
- `#[allow]`/`#[expect]` need `reason = "..."`. Prefer `expect`. A lint exception for size or complexity needs a debt entry in `docs/ARCHITECTURE.md` §8.
- Prefer `pub(crate)`; make an item `pub` only when another crate uses it.
- Configs and log schemas change additively only.
- The theory documents (paper, definitions, spec) stay out of this repo.

## Testing (every change)

- New rule or formula: a property test (ranges, invariants) and an exact-value test against hand-worked numbers.
- New default: pin it in `spec_defaults.rs`.
- Behaviour change in the sim or brain: update golden traces deliberately and say why in the PR.
- Coverage stays ≥ 95% of lines; mutation testing must kill every mutant on changed lines. A mutant that cannot change behaviour goes in `.cargo/mutants.toml` with its reason; anything else needs a test.
- Test through public APIs so refactors don't break tests that should pass.

## Before you open a PR

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
scripts/check-architecture.sh
(cd bot && npm ci && npm run typecheck)
```

- One concern per PR: a feature or a refactor, not both. Aim for under ~400 changed non-test lines.
- Base new work on `main`. Stack a PR on another only when it needs that one's code, and keep a stack to three PRs at most. When a stack's bottom PR merges, delete its branch so GitHub moves the next PR onto `main`.
- A refactor changes no test expectations and no golden traces.
- If you added a shortcut, add it to the debt register with the step by which it must be paid. If you paid one, delete its row and any `#[expect]` or size-budget line that referred to it.
- At the end of a build step, do the maintenance checkpoint in `docs/ARCHITECTURE.md` §7.
