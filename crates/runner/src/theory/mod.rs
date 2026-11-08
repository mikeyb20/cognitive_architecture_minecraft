//! Theory tests (spec §29; build step 5d): each claim is a set of brain
//! variants run on chosen scenarios, read by a probe into one table. A
//! table is data, not a pass/fail gate: a claim that fails is a result.
//!
//! The lifetime claims (T-DEV, tenacity vs age, T-CP, kick absorption,
//! aging) need agents that live for many days, so they wait until survival
//! past day 2 is fixed. P1 and P2 run the archetypes (step 7); P3 (lens swap) is later.

pub mod probes;
pub mod report;

use cognitive_core::agent::BrainConfig;
use cognitive_core::modes::Mode;

use crate::brain::{run_brain, setup};
use crate::sweep::{Variant, v, with};
use probes::{Behaviour, Insight, ModeSuccess, Perform, Probe, Streaks, Traits, Urgency, Windows};
pub use report::{csv, independence, markdown, mode_accuracy, personality_lines};

/// One claim and how it is tested.
pub struct Claim {
    pub id: &'static str,
    /// The claim and its source.
    pub claim: &'static str,
    /// How to read the table.
    pub reading: &'static str,
    pub scenarios: &'static [&'static str],
    /// The probe's columns (survival comes first in every table).
    pub columns: &'static [&'static str],
    pub variants: Vec<Variant>,
    pub probe: fn() -> Box<dyn Probe>,
    /// Variants are archetype means: each seed draws its own agent around
    /// them (spec §19) instead of running the means as they are.
    pub per_agent: bool,
    /// A line computed across cells, if the claim has one.
    pub post: Option<fn(&[Cell]) -> String>,
}

/// The claims by id (spec §29). A new claim is one function and one line.
const CLAIMS: [fn() -> Claim; 8] = [
    t_ind,
    kick,
    t_mode,
    subconscious,
    effort,
    harsh,
    beta_u,
    personality,
];

/// The claim ids, in order.
pub fn ids() -> Vec<&'static str> {
    CLAIMS.iter().map(|c| c().id).collect()
}

/// Look up a claim by id (case-insensitive).
pub fn claim(id: &str) -> Option<Claim> {
    CLAIMS
        .iter()
        .map(|c| c())
        .find(|c| c.id.eq_ignore_ascii_case(id))
}

/// Bias on one mode's Oracle prior that makes it dominate (proposed).
pub const FORCE_BIAS: f32 = 8.0;

/// The Oracle pinned to one mode: a strong prior and no learning.
pub fn forced(m: Mode, mut c: BrainConfig) -> BrainConfig {
    c.oracle_bias = [0.0; 4];
    c.oracle_bias[m.index()] = FORCE_BIAS;
    c.oracle_eta = 0.0;
    c
}

fn t_ind() -> Claim {
    let mut variants = vec![v("all frozen", with(|c| c.lens_frozen = [true; 4]))];
    for m in Mode::ALL {
        let mut f = [true; 4];
        f[m.index()] = false;
        variants.push(v(format!("only {m:?} learns"), with(|c| c.lens_frozen = f)));
    }
    variants.push(v("all learn (default)", BrainConfig::default()));
    Claim {
        id: "T-IND",
        claim: "Sub-networks train independently (00 §9.4.1-2)",
        reading: "Each \"only X learns\" row trains one lens with the others frozen. The claim holds if the frozen lenses' success stays within ±5% of the all-frozen row. s_L..s_O = each mode's mean success on actions.",
        scenarios: &["open_day", "commanded"],
        columns: &["s_L", "s_E", "s_P", "s_O"],
        variants,
        probe: || Box::new(ModeSuccess::default()),
        per_agent: false,
        post: Some(independence),
    }
}

fn kick() -> Claim {
    Claim {
        id: "KICK",
        claim: "The kick prevents traps (00 §9.4.1-3)",
        reading: "No-kick ablation. stuck = streaks of 50+ identical decisions per run; escape = their mean length; longest = longest streak; kicks = stuck kicks fired.",
        scenarios: &["open_day", "harsh"],
        columns: &["stuck", "escape", "longest", "kicks"],
        variants: vec![
            v("kick on (default)", BrainConfig::default()),
            v("kick off", with(|c| c.kick_enabled = false)),
        ],
        probe: || Box::new(Streaks::default()),
        per_agent: false,
        post: None,
    }
}

fn t_mode() -> Claim {
    let variants = Mode::ALL
        .map(|m| v(format!("forced {m:?}"), forced(m, BrainConfig::default())))
        .to_vec();
    Claim {
        id: "T-MODE",
        claim: "Modes are distinct (00 §9.4.1-4, §9.4.4)",
        reading: "Each row pins the Oracle to one mode. Columns are shares of decisions (travel, gather, craft, think, idle) and mean effort on actions. The line below says how often a nearest-centroid classifier recovers the mode from these numbers alone.",
        scenarios: &["open_day", "commanded"],
        columns: &["travel", "gather", "craft", "think", "idle", "effort"],
        variants,
        probe: || Box::new(Behaviour::default()),
        per_agent: false,
        post: Some(mode_accuracy),
    }
}

fn subconscious() -> Claim {
    Claim {
        id: "SUBC",
        claim: "The subconscious helps, especially with incubation (00 §9.4.1-6)",
        reading: "No-subconscious ablation on the incubation scenario. tech = tech-tree goals held (0–4); formed = memory links formed; sub_items = items the subconscious processed; new = novelty units.",
        scenarios: &["incubation"],
        columns: &["tech", "formed", "sub_items", "new"],
        variants: vec![
            v("subconscious on (default)", BrainConfig::default()),
            v("subconscious off", with(|c| c.subconscious.b_base = 0.0)),
        ],
        probe: || Box::new(Insight::default()),
        per_agent: false,
        post: None,
    }
}

fn effort() -> Claim {
    let variants = [0.2, 0.4, 0.6, 0.8]
        .map(|b| {
            let c = with(|c| c.curiosity.base = b);
            v(format!("C_base {b}"), forced(Mode::Performing, c))
        })
        .to_vec();
    Claim {
        id: "EFFORT",
        claim: "High C improves Performing (00 §4.3)",
        reading: "Oracle pinned to Performing; resting curiosity varied. C = mean C; effort = mean effort on actions; s_P = Performing's mean success on actions; tech = goals held.",
        scenarios: &["commanded"],
        columns: &["C", "effort", "s_P", "tech"],
        variants,
        probe: || Box::new(Perform::default()),
        per_agent: false,
        post: None,
    }
}

fn harsh() -> Claim {
    Claim {
        id: "HARSH",
        claim: "Hostile environments drop C (00 §2.6)",
        reading: "Mean C and D during ticks 1,200–2,400 (the harsh episode) and 2,400–4,800 (after it). open_day is the control with no attack.",
        scenarios: &["harsh", "open_day"],
        columns: &["C_during", "C_after", "D_during", "D_after"],
        variants: vec![v("default", BrainConfig::default())],
        probe: || Box::new(Windows::default()),
        per_agent: false,
        post: None,
    }
}

fn beta_u() -> Claim {
    let variants = [0.0, 0.2, 0.4]
        .map(|b| {
            let label = crate::sweep::named(format!("β_U {b}"), b == 0.2);
            v(label, with(|c| c.beta_u = b))
        })
        .to_vec();
    Claim {
        id: "BETA-U",
        claim: "Mortality raises C and exploring (00 §7.9; OQ22)",
        reading: "β_U ablation on long_horizon. Means of C, U and w_E, and the share of decisions that travel.",
        scenarios: &["long_horizon"],
        columns: &["C", "U", "w_E", "travel"],
        variants,
        probe: || Box::new(Urgency::default()),
        per_agent: false,
        post: None,
    }
}

fn personality() -> Claim {
    let variants = crate::archetype::ARCHETYPES
        .map(|a| {
            let mean = crate::archetype::means(a).expect("built-in archetype");
            v(a, mean)
        })
        .to_vec();
    Claim {
        id: "P1-P2",
        claim: "Archetypes are distinct, agents are individuals (00 §7.10; plan §5.3; #44)",
        reading: "One row per archetype; each seed is a different agent drawn around the archetype's means. Columns are shares of decisions (travel, gather, craft, survive, think, idle), the share driven by a player's command, mean effort on actions, and new things found per day. P1 below: how often a nearest-centroid classifier tells the archetype from these numbers alone (chance 0.25). P2: spread within an archetype over spread between archetypes; it should be above 0 and below 1.",
        scenarios: &["open_day", "commanded", "harsh"],
        columns: &[
            "travel",
            "gather",
            "craft",
            "survive",
            "think",
            "idle",
            "commanded",
            "effort",
            "new/day",
        ],
        variants,
        probe: || Box::new(Traits::default()),
        per_agent: true,
        post: Some(personality_lines),
    }
}

/// One variant on one scenario: every seed's numbers, survival first.
#[derive(Debug, Clone, PartialEq)]
pub struct Cell {
    pub variant: String,
    pub scenario: String,
    pub runs: Vec<Vec<f64>>,
}

impl Cell {
    /// Mean of each column over seeds, leaving out NaN.
    pub fn means(&self) -> Vec<f64> {
        let cols = self.runs.first().map_or(0, Vec::len);
        (0..cols)
            .map(|i| {
                let xs: Vec<f64> = self
                    .runs
                    .iter()
                    .map(|r| r[i])
                    .filter(|x| !x.is_nan())
                    .collect();
                if xs.is_empty() {
                    f64::NAN
                } else {
                    xs.iter().sum::<f64>() / xs.len() as f64
                }
            })
            .collect()
    }
}

/// One run read by the claim's probe, with survival in front.
pub fn run_probe(c: &Claim, variant: usize, scenario: &str, seed: u64) -> Vec<f64> {
    let mut p = (c.probe)();
    let mean = &c.variants[variant].config;
    let config = if c.per_agent {
        crate::archetype::drawn(mean, seed)
    } else {
        mean.clone()
    };
    let r = run_brain(setup(scenario, seed), config, seed, |row| p.push(row));
    let mut out = vec![f64::from(u8::from(r.survived))];
    out.extend(p.finish(&r));
    out
}

/// Every variant × scenario × seed, `threads` at a time, in a fixed order.
pub fn run_claim(c: &Claim, seeds: u64, threads: usize) -> Vec<Cell> {
    let jobs: Vec<(usize, &str, u64)> = (0..c.variants.len())
        .flat_map(|vi| {
            c.scenarios
                .iter()
                .flat_map(move |sc| (0..seeds).map(move |seed| (vi, *sc, seed)))
        })
        .collect();
    let runs = crate::par::par_map(&jobs, threads, |&(vi, sc, seed)| run_probe(c, vi, sc, seed));
    let per = (seeds as usize).max(1);
    runs.chunks(per)
        .zip(jobs.chunks(per))
        .map(|(rs, js)| Cell {
            variant: c.variants[js[0].0].label.clone(),
            scenario: js[0].1.to_string(),
            runs: rs.to_vec(),
        })
        .collect()
}
