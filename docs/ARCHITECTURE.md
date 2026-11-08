# Architecture and maintainability

How this codebase stays easy to change as it grows from one bot in a gridworld to many agents in Minecraft. The rules here are enforced by CI where a machine can check them (`.github/workflows/architecture.yml`), and by review where it can't.

The aim is simple: **no big rewrites.** We get there by paying small costs continuously: keeping layers one-way, growing by adding files rather than editing central ones, and cleaning up at the end of every build step.

## 1. Layers

Dependencies point one way. A crate may use only the crates below it.

```
                        runner                   CLI: scenarios x arms x seeds -> logs
     ┌──────────┬─────────┴┬──────────┬──────────┐
    sim  minecraft  cognitive-core  baselines ─► llm  worlds; the brain; comparison arms; LLM client
     └──────────┴─────────┬┴──────────┴──────────┘
                       host-api                  the contract: World, Host, ontology, tech tree
```

| Crate | May depend on | Why |
|---|---|---|
| `host-api` | nothing in the workspace | It is the contract. Everything else agrees on it. It also holds the tech tree (`craft`) and its planner, which are Minecraft knowledge shared by every world and arm. |
| `cognitive-core` | `host-api` | The brain must not know which world it is in (spec §2). |
| `sim` | `host-api` | A world implements the contract and knows nothing about brains. |
| `minecraft` | `host-api` | The Rust side of the Minecraft world: place memory and candidates from what the TypeScript bot sees. Like `sim`, it knows nothing about brains. |
| `llm` | `host-api` | A tool the brain calls through a trait, never directly. |
| `baselines` | `host-api`, `llm` | Arms run in any world, like the brain. (Tests use `sim` as a dev-dependency.) |
| `runner` | everything | The only place that wires parts together. |

`scripts/check-architecture.sh` fails CI on any edge not in this table. Adding a crate means adding a row here and in the script, so its place is a decision, not an accident.

Inside the bot, only `bot/src/mineflayer/` talks to Mineflayer. It copies what it reads into plain data (for example `perceive/observation.ts`'s `Snapshot`) and calls the library; every decision about that data lives in plain modules that are unit-tested, held to the coverage floors and mutation-tested like the Rust. The glue is checked by the live acceptance tests instead, so keep it thin.

In Minecraft the runner drives and the bot serves: `runner::mc` sends `host_api::link::LinkRequest`s (hello with the bot's settings, perceive, execute, say) as JSON lines over TCP, and the bot answers each with a `LinkReply`. Candidates, place memory and player commands are worked out in Rust (`minecraft`, `host_api::command`), so every arm sees the same choices in both worlds; the bot keeps only the body and the survival interrupt. The bot's tuning lives in the Rust defaults and is sent at connect.

The TypeScript bot does not copy these types by hand: `bot/src/generated/` is written from `host-api` by ts-rs (`cargo test -p host-api --features ts`), and CI fails if the committed copy is stale. To change the contract, change the Rust type and regenerate.

**Why it matters:** the day `cognitive-core` imports `sim`, the brain can no longer move to Minecraft without a rewrite. The same check keeps the TypeScript bot replaceable (spec §31.8).

## 2. Purity

`cognitive-core` and `host-api` do no IO: no files, network, environment, threads, clock, printing, OS randomness or hash-ordered collections (`HashMap`, `HashSet`). Randomness comes from the seeded RNG passed in; ordered maps are `BTreeMap`. `sim` and `baselines` may not use the clock or OS randomness either, and may use hash sets only for lookups, never iteration.

**Why it matters:** every run must replay exactly from its seed (spec §31.3). Determinism lost in one place is very expensive to find later, because the golden traces only tell you *that* a run changed, not where.

## 3. Grow by adding, not by editing

Most sprawl comes from one central function that every new feature edits: a `match` on skills, an `if config.variant == ...` in the tick loop. Every growth point below has (or will have) a seam where a new variant is a **new file plus one registry line**:

| Growth point | Seam | Status |
|---|---|---|
| Open questions with candidate answers (OQ8, OQ18, ...) | a trait in `core`, one impl per candidate, picked by config (spec §31.2) | in place for `g(C)` (`Activation`) |
| Modes (sub-networks) | `ModeNetwork` trait in `core::modes` | in place |
| Skills | one handler per skill group, implementing `Handler` in `sim::skills`, registered in the `HANDLERS` table (table order is candidate order) | in place |
| Worlds (sim, Minecraft) | `World` + `Host` traits | in place |
| Arms (A, B, B+N, brain) | `Policy` trait | in place |
| Thinking tools (plan, progress, own goals) | `ThinkTools` trait in `core::agent::think`; the sim's code tools implement it (`runner::brain::SimTools`) | in place; LLM tiers implement it in step 6 |
| LLM providers and tiers | a provider trait in `llm` | step 6 |
| Scenarios | `sim::scenario::build` by name | in place (moves to TOML with archetypes, step 7) |
| Archetypes, scenarios, experiments | TOML in `configs/`, no code | in place for archetypes (`runner::archetype`, drawn per agent by `core::agent::draw`); scenarios and experiments later |

Rule: if a change needs a new branch in a function that already branches on the same thing, add a seam instead.

## 4. Single source of truth

- **Names** (skills, items, blocks, task types) live once, in `host-api::ontology`. The TypeScript bot must not hand-copy them: its types are generated from Rust into `bot/src/generated/`.
- **Spec numbers** live once, as a default in a params struct, pinned by a test in `tests/spec_defaults.rs`. No literal spec value appears twice.
- **Formulas** live once, in the module that mirrors their 01 section. Other code calls them; it never re-derives them.
- **Duplication** is checked by jscpd (more than 3% of code in copied blocks of 70+ tokens fails CI). Below that, use the rule of three: the second copy is allowed, the third gets extracted. Extract to the lowest crate both callers already depend on; never add a dependency just to share a helper.

### Where the brain loop lives

`cognitive-core::agent` wires the 01 modules into one tick (spec §4). Each file is one phase, so a rule change has one home:

| Phase | File | Spec |
|---|---|---|
| Config and defaults | `agent/config.rs` | §19 |
| Learn from the last outcome (success, R, lenses, Oracle, tenacity, memory writes) | `agent/learn.rs` | §7.1, §8, §10, §13, §17 |
| Sense (chat goals, signals, C/V/D/π drift over dt, subconscious, recall) | `agent/perceive.rs` | §5, §11, §12, §14, §17 |
| Choose (scores, kicks, starts, softmax, intensity) | `agent/choose.rs` | §3, §9, §16 |
| Internal actions and their tools | `agent/think.rs` | §6 |
| Log row | `agent/log.rs` | §25 |

The formulas themselves stay in the module that mirrors their 01 section (`activation`, `control`, `modes`, `memory`, `direction`); the agent files only call them.

## 5. Size budgets

Large files and functions are where duplication and tangles start, so they have limits:

| Thing | Warning | Fails CI |
|---|---|---|
| Source file | 400 lines | 600 lines |
| Test file | 600 lines | 900 lines |
| Function | | 100 lines (`clippy::too_many_lines`) |
| Function complexity | | 25 (`clippy::cognitive_complexity`) |
| Function arguments | | 7 (`clippy::too_many_arguments`) |

Files already over the limit are listed in `scripts/size-budget.txt` with a cap that only goes down (a ratchet). A function allowed to exceed a lint carries `#[expect(clippy::..., reason = "debt Dn: ...")]`; `expect` (not `allow`) fails once the function is fixed, so exceptions can't go stale. Every `allow`/`expect` must give a reason.

## 6. Refactoring without big rewrites

- **Small and often.** Refactor in the PR that first feels the pain, or in a dedicated PR right after. Never save it up.
- **Refactors don't change behaviour.** A refactor PR changes no test expectations and leaves golden traces identical. If a trace changes, it isn't a refactor.
- **One concern per PR.** Feature or refactor, not both. Aim for under ~400 changed non-test lines.
- **Tests are the safety net.** Property, exact-value and golden-trace tests plus mutation testing exist so that refactors are cheap. Keep them that way: test behaviour through public APIs, not private details, so a refactor doesn't break tests that should still pass.
- **Schemas change additively** (spec §31.6): configs and logs gain fields, never repurpose them, so old runs stay readable.

## 7. Maintenance checkpoint (end of every build step)

Before a build step is called done, spend one short pass on:

1. CI warnings from the architecture job (files approaching their size limit).
2. The debt register below: pay anything whose trigger is the next step.
3. `scripts/size-budget.txt`: lower caps for files that shrank.
4. The jscpd report: extract any third copy.
5. Public API of the crates touched: anything `pub` that nothing outside the crate uses becomes `pub(crate)`.

This is how the codebase keeps up with the build instead of falling behind it.

## 8. Debt register

Known shortcuts, each with the point by which it must be paid.

| ID | Debt | Pay before | Plan |
|---|---|---|---|
| D4 | Public items lack doc comments (about 300). | Rolling | Enable `missing_docs` on `host-api` first (the contract), then crate by crate. |
| D5 | The sim offers no `Flee` candidate when the only hostile is diagonal or the way away is blocked (`point_toward`, used by `sim::skills::threats`), so a threatened agent sometimes cannot flee. A behaviour change, so it gets its own PR after the D2 refactor. | Step 6 | In `sim::skills::threats`, pick the best open neighbour away from all hostiles. |
| D6 | Arm B picks only from external candidates; spec §24 also offers it the internal ones (plan, reflect, imagine). Those need the LLM thinking tools. | Step 6 gate | When 6f-3 adds the LLM `ThinkTools`, list the internal actions after the external ones in arm B's prompt. |
