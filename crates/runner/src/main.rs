//! Experiment runner CLI (spec §26). With no arguments it prints the versions
//! it was built against. `runner first-night [N]` runs arm A for N seeds
//! (default 100) and prints the survival rate (build step 3 gate).

use baselines::ArmA;

fn main() {
    println!(
        "cognitive_architecture_minecraft runner (defs {}, paper {}, spec {})",
        cognitive_core::DEFS_VERSION,
        cognitive_core::PAPER_VERSION,
        cognitive_core::SPEC_VERSION,
    );
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("first-night") {
        let n = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(100);
        let results = runner::first_night(n, || Box::new(ArmA::default()));
        let survived = results.iter().filter(|r| r.survived).count();
        for r in results.iter().filter(|r| !r.survived) {
            println!("seed {} died at tick {} ({:?})", r.seed, r.ticks, r.death);
        }
        println!("arm A first-night survival: {survived}/{n}");
    }
}
