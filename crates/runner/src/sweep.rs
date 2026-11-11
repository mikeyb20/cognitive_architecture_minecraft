//! Open-question sweeps (spec §27 step 5): each OQ's candidate values, run
//! over the sim scenarios and seeds, summarised as one table per question.
//! Sweeps are defined here until experiments move to TOML (step 7).

use cognitive_core::activation::{AffectParams, GShape};
use cognitive_core::agent::{BrainConfig, BudgetPolicy};
use cognitive_core::control::oracle::Exp3;
use sim::scenario::NAMES;

use crate::brain::{run_brain, setup};
pub use crate::metrics::{
    FREEZE_CONFLICT, Meter, PACE, Pace, RunMetrics, Summary, summarise, tech_held,
};

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

pub(crate) fn v(label: impl Into<String>, config: BrainConfig) -> Variant {
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

/// The ids a CLI argument names: `all` of them or one (spec §27).
pub fn select<'a>(which: &'a str, all: &[&'static str]) -> Vec<&'a str> {
    if which == "all" {
        all.to_vec()
    } else {
        vec![which]
    }
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
pub(crate) fn named(label: String, is_default: bool) -> String {
    if is_default {
        format!("{label} (default)")
    } else {
        label
    }
}

pub(crate) fn with(f: impl FnOnce(&mut BrainConfig)) -> BrainConfig {
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

/// Run one variant on one scenario and seed.
pub fn run_one(config: &BrainConfig, scenario: &str, seed: u64) -> RunMetrics {
    let mut m = Meter::default();
    let r = run_brain(setup(scenario, seed), config.clone(), seed, |row| {
        m.push(row)
    });
    m.finish(&r)
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
    let out = crate::par::par_map(&jobs, threads, |&(vi, sc, seed)| {
        run_one(&s.variants[vi].config, sc, seed)
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

const HEADER: [&str; 18] = [
    "variant", "scenario", "survival", "active", "self", "C", "D", "V", "freeze", "stuck",
    "reflexes", "new", "tech", "memory", "recall", "think", "plan lag", "shelter",
];

fn cells(r: &Row) -> [String; 18] {
    let s = &r.summary;
    // A dash for a variable the arm does not have.
    let f = |x: f32, places: usize| {
        if x.is_nan() {
            "–".to_string()
        } else {
            format!("{x:.places$}")
        }
    };
    let f2 = |x: f32| f(x, 2);
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
        f(s.memory, 0),
        f2(s.recall),
        f(s.thinking, 1),
        f(s.plan_lag, 0),
        f2(s.shelter),
    ]
}

/// The legend and the results table, one row per variant and scenario.
pub fn table(rows: &[Row], seeds: u64) -> String {
    let mut t = format!(
        "{seeds} seeds per cell. survival = share alive at the end; active = share of decisions that started something; self = started without a command or danger push; C, D, V = means over decisions; freeze = share of actions with conflict > {FREEZE_CONFLICT}; stuck = stuck kicks per run; new = novelty units found; tech = tech-tree goals held (0–4); memory = items in memory at the end; recall = share of decisions that recalled a linked fact; think = plan, reflect and imagine runs per 1,000 ticks; plan lag = mean ticks from a new goal to its first plan step; shelter = share of runs that stood in a shelter passing the shelter test (on held_out, the transfer score). A dash means the arm has no such variable.\n\n"
    );
    t += &format!("| {} |\n", HEADER.join(" | "));
    t += &format!("|{}\n", "---|".repeat(HEADER.len()));
    for r in rows {
        t += &format!("| {} |\n", cells(r).join(" | "));
    }
    t + &pace_table(rows)
}

/// Progression pace (plan §5.0): per milestone, the share of runs that
/// reached it and the mean ticks into the run over those runs.
fn pace_table(rows: &[Row]) -> String {
    let mut t = "\nProgression pace (plan §5.0): share of runs that reached each milestone @ mean ticks into the run for those runs (a dash if none did). shelter = a shelter that passes the shelter test.\n\n".to_string();
    t += &format!("| variant | scenario | {} |\n", PACE.join(" | "));
    t += &format!("|{}\n", "---|".repeat(PACE.len() + 2));
    for r in rows {
        let cells = r.summary.pace.map(|p| {
            if p.ticks.is_nan() {
                "–".to_string()
            } else {
                format!("{:.2} @ {:.0}", p.reached, p.ticks)
            }
        });
        t += &format!(
            "| {} | {} | {} |\n",
            r.variant,
            r.scenario,
            cells.join(" | ")
        );
    }
    t
}

/// CSV columns after `HEADER`: each pace milestone's share and ticks.
fn pace_header() -> Vec<String> {
    PACE.iter()
        .flat_map(|m| [format!("{m} reached"), format!("{m} ticks")])
        .collect()
}

/// Each milestone's share and mean ticks; a dash for ticks when none did.
fn pace_cells(p: &[Pace; 5]) -> Vec<String> {
    let ticks = |t: f32| {
        if t.is_nan() {
            "–".to_string()
        } else {
            format!("{t:.0}")
        }
    };
    p.iter()
        .flat_map(|p| [format!("{:.2}", p.reached), ticks(p.ticks)])
        .collect()
}

/// The sweep as a Markdown table with a short heading.
pub fn markdown(s: &Sweep, rows: &[Row], seeds: u64) -> String {
    format!("## {}: {}\n\n{}", s.id, s.question, table(rows, seeds))
}

/// Rows as CSV, each line starting with `id` under the column `key`.
pub fn csv_rows(key: &str, id: &str, rows: &[Row]) -> String {
    let mut t = format!("{key},{},{}\n", HEADER.join(","), pace_header().join(","));
    for r in rows {
        let c = cells(r).map(|x| {
            if x.contains(',') {
                format!("\"{x}\"")
            } else {
                x
            }
        });
        let p = pace_cells(&r.summary.pace);
        t += &format!("{id},{},{}\n", c.join(","), p.join(","));
    }
    t
}

/// The sweep as CSV, one line per row.
pub fn csv(s: &Sweep, rows: &[Row]) -> String {
    csv_rows("oq", s.id, rows)
}

/// Every scenario the sim knows (spec §21).
pub fn all_scenarios() -> Vec<&'static str> {
    NAMES.to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    // A unit test so it fails before the CLI tests run every sweep.
    #[test]
    fn select_names_all_or_one() {
        assert_eq!(select("all", &ids()), ids());
        assert_eq!(select("OQ8", &ids()), vec!["OQ8"]);
    }
}
