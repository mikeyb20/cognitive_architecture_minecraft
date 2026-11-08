//! Archetypes (spec §19, #44): TOML files in `configs/archetypes` that
//! override the brain's defaults. They are built into the binary, so a run
//! does not depend on the working directory.

use cognitive_core::agent::{BrainConfig, Spread, draw};

/// Every archetype file, by name. `default` is spec §19's default column.
const FILES: [(&str, &str); 5] = [
    (
        "default",
        include_str!("../../../configs/archetypes/default.toml"),
    ),
    (
        "explorer",
        include_str!("../../../configs/archetypes/explorer.toml"),
    ),
    (
        "builder",
        include_str!("../../../configs/archetypes/builder.toml"),
    ),
    (
        "guardian",
        include_str!("../../../configs/archetypes/guardian.toml"),
    ),
    (
        "helper",
        include_str!("../../../configs/archetypes/helper.toml"),
    ),
];

/// The four archetypes compared in the personality tests (#44).
pub const ARCHETYPES: [&str; 4] = ["explorer", "builder", "guardian", "helper"];

/// An archetype's parameter means (spec §19).
pub fn means(name: &str) -> Result<BrainConfig, String> {
    let (_, text) = FILES
        .iter()
        .find(|(n, _)| *n == name)
        .ok_or_else(|| format!("no archetype called {name:?}"))?;
    toml::from_str(text).map_err(|e| format!("archetype {name}: {e}"))
}

/// Mixed into the run seed so an agent's personality draw does not reuse
/// the brain's own random stream (both are seeded ChaCha8).
pub const DRAW_SALT: u64 = 0x9E37_79B9_7F4A_7C15;

/// Agent `seed` drawn around `mean` with the spec §19 spread.
pub fn drawn(mean: &BrainConfig, seed: u64) -> BrainConfig {
    draw(mean, Spread::default(), seed ^ DRAW_SALT)
}

/// Agent `seed` of an archetype.
pub fn agent(name: &str, seed: u64) -> Result<BrainConfig, String> {
    Ok(drawn(&means(name)?, seed))
}
