//! Open-question sweeps (spec §27 step 5): each OQ's candidate values, run
//! over the sim scenarios and seeds, summarised as one table per question.
//! Sweeps are defined here until experiments move to TOML (step 7).

use cognitive_core::activation::{AffectParams, GShape};
use cognitive_core::agent::{BrainConfig, BudgetPolicy, TickLog};
use cognitive_core::control::oracle::Exp3;
use sim::planner::TECH_GOALS;
use sim::scenario::NAMES;

use crate::brain::{BrainResult, run_brain, setup};

/// One candidate answer to an open question.
#[derive(Debug, Clone)]
pub struct Variant {
    pub label: String,
    pub config: BrainConfig,
}

#[derive(Debug, Clone)]
pub struct Sweep {
    pub id: &'static str,
    pub question: &'static str,
    pub variants: Vec<Variant>,
}

fn v(label: impl Into<String>, config: BrainConfig) -> Variant {
    Variant {
        label: label.into(),
        config,
    }
}

/// The step 5 sweeps (spec §27): id, question, variants. A new sweep is one
/// function and one line here.
/// (id, question, variants).
type Entry = (&'static str, &'static str, fn() -> Vec<Variant>);

const SWEEPS: [Entry; 6] = [
    ("OQ8", "Shape of g(C) (spec §16)", g_shapes),
    (
        "OQ12",
        "Subconscious selection mix: unresolved, |V_enc|, P (spec §17)",
        selection_mixes,
    ),
    (
        "OQ16",
        "Subconscious budget policy (spec §14)",
        budget_policies,
    ),
    ("OQ17", "D rise and recovery rates (spec §12)", affect_grid),
    ("OQ18", "Oracle learner (spec §8)", oracle_learners),
    (
        "OQ19",
        "Lens randomness: temperature scale (spec §7)",
        temperatures,
    ),
];

/// The sweep ids, in order.
pub fn ids() -> Vec<&'static str> {
    SWEEPS.iter().map(|s| s.0).collect()
}

/// Look up a sweep by id (case-insensitive).
pub fn sweep(id: &str) -> Option<Sweep> {
    SWEEPS
        .iter()
        .find(|s| s.0.eq_ignore_ascii_case(id))
        .map(|&(id, question, variants)| Sweep {
            id,
            question,
            variants: variants(),
        })
}

/// "label (default)" for the default value, else "label".
fn named(label: String, is_default: bool) -> String {
    if is_default {
        format!("{label} (default)")
    } else {
        label
    }
}

fn with(f: impl FnOnce(&mut BrainConfig)) -> BrainConfig {
    let mut c = BrainConfig::default();
    f(&mut c);
    c
}

fn g_shapes() -> Vec<Variant> {
    let mut out: Vec<Variant> = [1.5, 2.0, 3.0]
        .map(|a| v(named(format!("C^{a}"), a == 2.0), with(|c| c.alpha = a)))
        .to_vec();
    out.push(v("log(1+10C)/log 11", with(|c| c.g_shape = GShape::Log)));
    out
}

fn selection_mixes() -> Vec<Variant> {
    [
        ("0.4/0.3/0.3 (default)", [0.4, 0.3, 0.3]),
        ("unresolved only", [1.0, 0.0, 0.0]),
        ("|V_enc| only", [0.0, 1.0, 0.0]),
        ("P only", [0.0, 0.0, 1.0]),
    ]
    .map(|(l, mix)| v(l, with(|c| c.subconscious.mix = mix)))
    .to_vec()
}

fn budget_policies() -> Vec<Variant> {
    [BudgetPolicy::RestBoost, BudgetPolicy::Fixed]
        .map(|p| {
            let label = named(format!("{p:?}"), p == BudgetPolicy::default());
            v(label, with(|c| c.budget_policy = p))
        })
        .to_vec()
}

fn oracle_learners() -> Vec<Variant> {
    let mut out = vec![v("gradient bandit (default)", BrainConfig::default())];
    out.extend([0.05, 0.1, 0.2].map(|g| {
        v(
            format!("EXP3 γ={g}"),
            with(|c| c.oracle_exp3 = Some(Exp3::new(g))),
        )
    }));
    out
}

fn temperatures() -> Vec<Variant> {
    [0.5, 1.0, 2.0]
        .map(|t| v(named(format!("τ×{t}"), t == 1.0), with(|c| c.tau_scale = t)))
        .to_vec()
}

/// D rise × D recovery, each at half, default and double (OQ17).
fn affect_grid() -> Vec<Variant> {
    let base = AffectParams::default();
    let mut out = Vec::new();
    for rise in [0.5, 1.0, 2.0] {
        for rec in [0.5, 1.0, 2.0] {
            let label = named(
                format!("rise×{rise} recover×{rec}"),
                rise == 1.0 && rec == 1.0,
            );
            out.push(v(
                label,
                with(|c| {
                    c.affect.d_rise = base.d_rise * rise;
                    c.affect.d_recover = base.d_recover * rec;
                }),
            ));
        }
    }
    out
}

/// What one run shows, from its log rows and result.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct RunMetrics {
    pub survived: bool,
    pub ticks: u64,
    pub decisions: u32,
    /// Share of decisions that started something (not idle).
    pub active: f32,
    /// Share of decisions that started something without external drive.
    pub self_started: f32,
    pub mean_c: f32,
    pub mean_d: f32,
    pub mean_v: f32,
    /// Share of decisions where the modes nearly cancel (conflict > 0.9).
    pub freeze: f32,
    pub stuck_kicks: u32,
    pub reflexes: u32,
    pub new_things: usize,
    /// Tech-tree goals held at the end (spec §21), 0..4.
    pub tech: u32,
}

/// Conflict above which a decision counts as near-freeze (01 §1.5).
pub const FREEZE_CONFLICT: f32 = 0.9;

/// Accumulates `RunMetrics` from rows as they come.
#[derive(Debug, Default)]
pub struct Meter {
    n: u32,
    active: u32,
    self_started: u32,
    c: f64,
    d: f64,
    v: f64,
    freeze: u32,
    stuck: u32,
}

impl Meter {
    pub fn push(&mut self, r: &TickLog) {
        self.n += 1;
        let acted = r.chosen != "idle";
        self.active += u32::from(acted);
        self.self_started += u32::from(acted && !r.external_drive);
        self.c += f64::from(r.c);
        self.d += f64::from(r.d);
        self.v += f64::from(r.v);
        self.freeze += u32::from(acted && r.conflict > FREEZE_CONFLICT);
        self.stuck += u32::from(r.stuck_kick);
    }

    pub fn finish(&self, r: &BrainResult) -> RunMetrics {
        let n = self.n.max(1) as f32;
        let mean = |x: f64| (x / f64::from(self.n.max(1))) as f32;
        let held = |g: &str| r.inventory.iter().any(|(i, k)| i == g && *k > 0);
        RunMetrics {
            survived: r.survived,
            ticks: r.ticks,
            decisions: self.n,
            active: self.active as f32 / n,
            self_started: self.self_started as f32 / n,
            mean_c: mean(self.c),
            mean_d: mean(self.d),
            mean_v: mean(self.v),
            freeze: self.freeze as f32 / n,
            stuck_kicks: self.stuck,
            reflexes: r.reflexes,
            new_things: r.new_things,
            tech: TECH_GOALS.iter().filter(|g| held(g)).count() as u32,
        }
    }
}

/// Run one variant on one scenario and seed.
pub fn run_one(config: &BrainConfig, scenario: &str, seed: u64) -> RunMetrics {
    let mut m = Meter::default();
    let r = run_brain(setup(scenario, seed), config.clone(), seed, |row| {
        m.push(row)
    });
    m.finish(&r)
}

/// Mean of each metric over seeds, and the survival rate.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Summary {
    pub runs: u32,
    pub survival: f32,
    pub active: f32,
    pub self_started: f32,
    pub mean_c: f32,
    pub mean_d: f32,
    pub mean_v: f32,
    pub freeze: f32,
    pub stuck_kicks: f32,
    pub reflexes: f32,
    pub new_things: f32,
    pub tech: f32,
}

pub fn summarise(runs: &[RunMetrics]) -> Summary {
    let n = runs.len().max(1) as f32;
    let avg = |f: &dyn Fn(&RunMetrics) -> f32| runs.iter().map(f).sum::<f32>() / n;
    Summary {
        runs: runs.len() as u32,
        survival: avg(&|r| f32::from(u8::from(r.survived))),
        active: avg(&|r| r.active),
        self_started: avg(&|r| r.self_started),
        mean_c: avg(&|r| r.mean_c),
        mean_d: avg(&|r| r.mean_d),
        mean_v: avg(&|r| r.mean_v),
        freeze: avg(&|r| r.freeze),
        stuck_kicks: avg(&|r| r.stuck_kicks as f32),
        reflexes: avg(&|r| r.reflexes as f32),
        new_things: avg(&|r| r.new_things as f32),
        tech: avg(&|r| r.tech as f32),
    }
}

/// One table row: a variant on a scenario.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub variant: String,
    pub scenario: String,
    pub summary: Summary,
}

/// Run every variant × scenario × seed, `threads` at a time. Results are
/// in a fixed order whatever the thread count, and each run is seeded, so
/// the table replays exactly.
pub fn run_sweep(s: &Sweep, scenarios: &[&str], seeds: u64, threads: usize) -> Vec<Row> {
    let jobs: Vec<(usize, &str, u64)> = (0..s.variants.len())
        .flat_map(|vi| {
            scenarios
                .iter()
                .flat_map(move |sc| (0..seeds).map(move |seed| (vi, *sc, seed)))
        })
        .collect();
    let mut out = vec![RunMetrics::default(); jobs.len()];
    let chunk = jobs.len().div_ceil(threads.max(1)).max(1);
    std::thread::scope(|scope| {
        for (js, os) in jobs.chunks(chunk).zip(out.chunks_mut(chunk)) {
            scope.spawn(move || {
                for (&(vi, sc, seed), o) in js.iter().zip(os) {
                    *o = run_one(&s.variants[vi].config, sc, seed);
                }
            });
        }
    });
    let per = seeds as usize;
    out.chunks(per.max(1))
        .zip(jobs.chunks(per.max(1)))
        .map(|(runs, js)| Row {
            variant: s.variants[js[0].0].label.clone(),
            scenario: js[0].1.to_string(),
            summary: summarise(runs),
        })
        .collect()
}

const HEADER: [&str; 13] = [
    "variant", "scenario", "survival", "active", "self", "C", "D", "V", "freeze", "stuck",
    "reflexes", "new", "tech",
];

fn cells(r: &Row) -> [String; 13] {
    let s = &r.summary;
    let f2 = |x: f32| format!("{x:.2}");
    [
        r.variant.clone(),
        r.scenario.clone(),
        f2(s.survival),
        f2(s.active),
        f2(s.self_started),
        f2(s.mean_c),
        f2(s.mean_d),
        f2(s.mean_v),
        f2(s.freeze),
        format!("{:.1}", s.stuck_kicks),
        format!("{:.1}", s.reflexes),
        format!("{:.0}", s.new_things),
        f2(s.tech),
    ]
}

/// The sweep as a Markdown table with a short heading.
pub fn markdown(s: &Sweep, rows: &[Row], seeds: u64) -> String {
    let mut t = format!(
        "## {}: {}\n\n{} seeds per cell. survival = share alive at the end; active = share of decisions that started something; self = started without a command or danger push; C, D, V = means over decisions; freeze = share of actions with conflict > {FREEZE_CONFLICT}; stuck = stuck kicks per run; new = novelty units found; tech = tech-tree goals held (0–4).\n\n",
        s.id, s.question, seeds
    );
    t += &format!("| {} |\n", HEADER.join(" | "));
    t += &format!("|{}\n", "---|".repeat(HEADER.len()));
    for r in rows {
        t += &format!("| {} |\n", cells(r).join(" | "));
    }
    t
}

/// The sweep as CSV, one line per row.
pub fn csv(s: &Sweep, rows: &[Row]) -> String {
    let mut t = format!("oq,{}\n", HEADER.join(","));
    for r in rows {
        let c = cells(r).map(|x| {
            if x.contains(',') {
                format!("\"{x}\"")
            } else {
                x
            }
        });
        t += &format!("{},{}\n", s.id, c.join(","));
    }
    t
}

/// Every scenario the sim knows (spec §21).
pub fn all_scenarios() -> Vec<&'static str> {
    NAMES.to_vec()
}
