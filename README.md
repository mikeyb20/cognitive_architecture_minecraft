# cognitive_architecture_minecraft

Minecraft agents whose decision-making runs on the **Curiosity Architecture**: a cognitive model in which a single curiosity magnitude `C` drives how much an agent does, four modes (Learning, Exploring, Performing, Optimizing) shape what it does, and an Oracle learns how to weight them.

> **Status:** early build. Step 1 of 9 (repository skeleton and CI). Nothing runs in Minecraft yet.

## Layout

| Path | What it is |
|---|---|
| `crates/host-api` | `World` and `Host` traits plus the shared ontology. The brain sees only these. |
| `crates/core` | The brain. Pure Rust, deterministic given a seed. Modules mirror the definitions doc. |
| `crates/sim` | Fast gridworld with Minecraft rules in miniature, for runs much faster than real time. |
| `crates/baselines` | Comparison arms: a behaviour tree and a minimal LLM loop. |
| `crates/llm` | Provider-agnostic LLM client (local Ollama or cloud), used as a tool by the brain. |
| `crates/runner` | CLI that runs scenarios × arms × seeds and writes logs. |
| `bindings/node` | napi-rs bridge from Rust to Node (later step). |
| `bot` | TypeScript adapter on [Mineflayer](https://github.com/PrismarineJS/mineflayer), targeting Minecraft Java 1.21.11. |
| `configs` | Personalities (archetypes), scenarios and experiments. |

## Build

Requires stable Rust and Node 22+.

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo run -p runner

cd bot && npm ci && npm run typecheck
```

## Testing

Every rule in the definitions has a test, checked three ways:
- **Property tests** (proptest): ranges and invariants hold for thousands of random inputs, e.g. Oracle weights always sum to 1 and never drop below the floor.
- **Exact-value tests** (`crates/core/tests/formulas.rs`): each formula against numbers worked out by hand, so a wrong sign or coefficient fails.
- **Pinned defaults** (`crates/core/tests/spec_defaults.rs`): every default parameter matches the spec, so a silent change breaks a test.

CI enforces two more checks on top of the tests:
- **Coverage:** at least 95% of lines must run under test (`cargo llvm-cov`).
- **Mutation testing:** on every pull request, `cargo mutants` deliberately breaks the changed code and fails if any break goes unnoticed. To run it on the whole brain locally: `cargo mutants -p cognitive-core`.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Issues are welcome; code contributions aren't being accepted yet.

## License

[MIT](LICENSE). Skill code ported from other MIT projects (Mindcraft, minecraft-agent-swarm) keeps its original notice and credit in each file.
