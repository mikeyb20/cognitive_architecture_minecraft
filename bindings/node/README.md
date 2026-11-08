# Node bridge (napi-rs)

Exposes `cognitive-core` and `llm` to the TypeScript Mineflayer adapter (spec §0, #33). Added in build step 6, when the bot first needs the brain. The sim never uses the bridge: brain and sim run in one Rust process (#34).
