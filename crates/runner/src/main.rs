//! Experiment runner CLI (spec §26). Step 1 only prints the versions it was
//! built against; scenarios x arms x seeds come in later build steps.

fn main() {
    println!(
        "cognitive_architecture_minecraft runner (defs {}, paper {}, spec {})",
        cognitive_core::DEFS_VERSION,
        cognitive_core::PAPER_VERSION,
        cognitive_core::SPEC_VERSION,
    );
}
