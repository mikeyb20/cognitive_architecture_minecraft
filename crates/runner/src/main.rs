//! Experiment runner CLI (spec §26). With no arguments it prints the versions
//! it was built against.
//!
//! - `runner first-night [N]`: arm A over N seeds (build step 3 gate).
//! - `runner brain <scenario> [N] [--log FILE]`: the brain over N seeds,
//!   optionally writing one JSONL row per decision (spec §25).
//! - `runner speed [N] [--gate G]`: world ticks per second for the brain
//!   (step 4 gate: at least 5,000); exits non-zero below the gate.

use baselines::ArmA;
use std::io::Write;
use std::time::Instant;

/// Build step 4 speed gate (spec §21, §27).
const SPEED_GATE: f64 = 5000.0;

fn main() {
    println!(
        "cognitive_architecture_minecraft runner (defs {}, paper {}, spec {})",
        cognitive_core::DEFS_VERSION,
        cognitive_core::PAPER_VERSION,
        cognitive_core::SPEC_VERSION,
    );
    let args: Vec<String> = std::env::args().skip(1).collect();
    let n = |i: usize, d: u64| args.get(i).and_then(|s| s.parse().ok()).unwrap_or(d);
    match args.first().map(String::as_str) {
        Some("first-night") => first_night(n(1, 100)),
        Some("brain") => brain(&args),
        Some("speed") => speed(n(1, 20), flag(&args, "--gate").unwrap_or(SPEED_GATE)),
        _ => {}
    }
}

/// The value after `name`, parsed.
fn flag(args: &[String], name: &str) -> Option<f64> {
    let i = args.iter().position(|a| a == name)?;
    args.get(i + 1)?.parse().ok()
}

fn first_night(n: u64) {
    let results = runner::first_night(n, || Box::new(ArmA::default()));
    let survived = results.iter().filter(|r| r.survived).count();
    for r in results.iter().filter(|r| !r.survived) {
        println!("seed {} died at tick {} ({:?})", r.seed, r.ticks, r.death);
    }
    println!("arm A first-night survival: {survived}/{n}");
}

fn brain(args: &[String]) {
    let name = args.get(1).map_or("first_night", String::as_str);
    let n: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(1);
    let log_path = args
        .iter()
        .position(|a| a == "--log")
        .and_then(|i| args.get(i + 1));
    let mut out = log_path.map(|p| {
        std::io::BufWriter::new(std::fs::File::create(p).expect("cannot create the log file"))
    });
    let mut alive = 0;
    for seed in 0..n {
        let setup = runner::brain::setup(name, seed);
        if let Some(f) = out.as_mut() {
            let header = runner::brain::header(name, seed);
            writeln!(f, "{header}").expect("log write failed");
        }
        let r = runner::brain::run_brain(
            setup,
            cognitive_core::agent::BrainConfig::default(),
            seed,
            |row| {
                if let Some(f) = out.as_mut() {
                    let line = serde_json::to_string(row).expect("row serialises");
                    writeln!(f, "{line}").expect("log write failed");
                }
            },
        );
        alive += usize::from(r.survived);
        println!(
            "seed {seed}: {} at tick {} ({:?}); {} decisions, {} idle, {} thoughts, {} reflexes, {} new things; inventory {:?}",
            if r.survived { "survived" } else { "died" },
            r.ticks,
            r.death,
            r.decisions,
            r.idle,
            r.thoughts,
            r.reflexes,
            r.new_things,
            r.inventory
        );
    }
    println!("brain {name}: {alive}/{n} survived");
}

fn speed(n: u64, gate: f64) {
    let start = Instant::now();
    let ticks = runner::brain::speed_ticks(n);
    let secs = start.elapsed().as_secs_f64();
    let rate = runner::brain::rate(ticks, secs);
    println!("brain speed: {ticks} world ticks in {secs:.2} s = {rate:.0} ticks/s (gate {gate})");
    if rate < gate {
        std::process::exit(1);
    }
}
