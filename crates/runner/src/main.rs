//! Experiment runner CLI (spec §26). With no arguments it prints the versions
//! it was built against.
//!
//! - `runner first-night [N]`: arm A over N seeds (build step 3 gate).
//! - `runner brain <scenario> [N] [--log FILE]`: the brain over N seeds,
//!   optionally writing one JSONL row per decision (spec §25).
//! - `runner speed [N] [--gate G]`: world ticks per second for the brain
//!   (step 4 gate: at least 5,000); exits non-zero below the gate.
//! - `runner sweep <OQ|all> [--seeds N] [--scenarios a,b] [--out DIR]`: an
//!   open-question sweep (step 5); prints a Markdown table and, with
//!   `--out`, writes `<OQ>.md` and `<OQ>.csv` there.
//! - `runner theory <ID|all> [--seeds N] [--out DIR]`: a theory test (spec
//!   §29, step 5d), printed and written the same way.
//! - `runner mc <a|b|bn> [--tier local|cheap|strong] [--addr HOST:PORT]
//!   [--decisions N] [--log FILE] [--cache FILE [--replay]]`: run an arm in
//!   Minecraft through the bot (`npm start` in bot/ first). Reads
//!   `ANTHROPIC_API_KEY` and `OLLAMA_BASE_URL` from the environment.

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
        Some("speed") => {
            let gate = flag(&args, "--gate").and_then(|g| g.parse().ok());
            speed(n(1, 20), gate.unwrap_or(SPEED_GATE));
        }
        Some("sweep") => sweep(&args),
        Some("theory") => theory(&args),
        Some("mc") => mc(&args),
        _ => {}
    }
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

fn flag<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
}

fn sweep(args: &[String]) {
    use runner::sweep::{all_scenarios, csv, ids, markdown, run_sweep, select, sweep};
    let which = args.get(1).map_or("all", String::as_str);
    let seeds = seeds(args);
    let scenarios: Vec<&str> = match flag(args, "--scenarios") {
        Some(list) => list.split(',').collect(),
        None => all_scenarios(),
    };
    for id in select(which, &ids()) {
        let Some(s) = sweep(id) else {
            eprintln!("unknown sweep {id}; known: {:?}", ids());
            std::process::exit(2);
        };
        let rows = run_sweep(&s, &scenarios, seeds, threads());
        publish(args, s.id, &markdown(&s, &rows, seeds), &csv(&s, &rows));
    }
}

fn seeds(args: &[String]) -> u64 {
    flag(args, "--seeds")
        .and_then(|s| s.parse().ok())
        .unwrap_or(10)
}

fn threads() -> usize {
    std::thread::available_parallelism().map_or(1, usize::from)
}

/// Print a table and, with `--out DIR`, write `<id>.md` and `<id>.csv`.
fn publish(args: &[String], id: &str, md: &str, csv: &str) {
    println!("{md}");
    if let Some(dir) = flag(args, "--out") {
        std::fs::create_dir_all(dir).expect("cannot create the output folder");
        let p = std::path::Path::new(dir);
        std::fs::write(p.join(format!("{id}.md")), md).expect("write failed");
        std::fs::write(p.join(format!("{id}.csv")), csv).expect("write failed");
    }
}

fn theory(args: &[String]) {
    use runner::theory::{claim, csv, ids, markdown, run_claim};
    let which = args.get(1).map_or("all", String::as_str);
    let n = seeds(args);
    for id in runner::sweep::select(which, &ids()) {
        let Some(c) = claim(id) else {
            eprintln!("unknown theory test {id}; known: {:?}", ids());
            std::process::exit(2);
        };
        let rows = run_claim(&c, n, threads());
        publish(args, c.id, &markdown(&c, &rows, n), &csv(&c, &rows));
    }
}

fn mc(args: &[String]) {
    use runner::mc::{Link, McArm, McHost, arm, provider, row, run, tier};
    let fail = |msg: String| -> ! {
        eprintln!("{msg}");
        std::process::exit(2);
    };
    let which = args.get(1).and_then(|a| McArm::parse(a));
    let which = which.unwrap_or_else(|| fail("usage: runner mc <a|b|bn> [flags]".into()));
    let level = tier(flag(args, "--tier").unwrap_or("local"));
    let level = level.unwrap_or_else(|| fail("--tier is local, cheap or strong".into()));
    let cache = flag(args, "--cache").map(|f| (f.into(), args.iter().any(|a| a == "--replay")));
    let llm = which.needs_llm().then(|| {
        let env = |k: &str| std::env::var(k).ok();
        provider(
            level,
            env("OLLAMA_BASE_URL"),
            env("ANTHROPIC_API_KEY"),
            cache,
        )
        .unwrap_or_else(|e| fail(e))
    });
    let mut policy = arm(which, level, llm).unwrap_or_else(|e| fail(e));
    let addr = flag(args, "--addr").unwrap_or("127.0.0.1:7878");
    let link = Link::tcp(addr).unwrap_or_else(|e| fail(format!("no bot at {addr}: {e}")));
    let config = host_api::link::BotConfig::default();
    let params = minecraft::McParams::default();
    let mut host = McHost::connect(link, config, params)
        .unwrap_or_else(|e| fail(format!("the bot did not say hello: {e:?}")));
    let mut out = flag(args, "--log").map(|p| {
        std::io::BufWriter::new(std::fs::File::create(p).expect("cannot create the log file"))
    });
    let n = flag(args, "--decisions").and_then(|d| d.parse().ok());
    let thresholds = host_api::reflex::ReflexThresholds::default();
    let made = run(
        &mut host,
        policy.as_mut(),
        n.unwrap_or(u32::MAX),
        thresholds,
        |s| {
            if let Some(f) = out.as_mut() {
                writeln!(f, "{}", row(s)).expect("log write failed");
            }
        },
    );
    println!("{made} decisions; link error: {:?}", host.error);
    if let Some(stats) = policy.llm_stats() {
        println!("LLM: {stats:?}");
    }
}
