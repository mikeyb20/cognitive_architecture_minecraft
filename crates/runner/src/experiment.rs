//! Experiments (spec §26; plan §5): arms × scenarios × seeds from a TOML
//! file in `configs/experiments`, with each arm's expected direction
//! written down before the run (plan §5.5).
//!
//! Arm `a` is the behaviour tree; arm `c` is the brain, with optional
//! config overrides for the ablations (spec §24 C−X: one flag each).

use baselines::{ArmA, Policy};
use cognitive_core::agent::{BrainConfig, Decision, TickLog};
use host_api::{Candidate, Observation, Outcome};
use serde::Deserialize;

use crate::FULL;
use crate::brain::{BrainResult, setup};
use crate::drive::{Driver, run_scenario};
use crate::sweep::{Meter, Row, RunMetrics, summarise, tech_held};

/// Which arm picks the actions (spec §24).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ArmKind {
    /// The behaviour tree.
    A,
    /// The brain.
    C,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArmSpec {
    pub label: String,
    pub arm: ArmKind,
    /// The direction expected before the run (plan §5.5).
    pub expected: String,
    /// Brain settings changed from the archetype's (arm `c` only).
    #[serde(default)]
    pub config: toml::Table,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Experiment {
    pub id: String,
    pub question: String,
    pub scenarios: Vec<String>,
    #[serde(default = "default_seeds")]
    pub seeds: u64,
    /// The brain's archetype (spec §19); `default` unless set.
    #[serde(default = "default_archetype")]
    pub archetype: String,
    /// Draw a different agent per seed around the archetype (spec §19).
    #[serde(default)]
    pub per_agent: bool,
    pub arms: Vec<ArmSpec>,
}

fn default_seeds() -> u64 {
    10
}

fn default_archetype() -> String {
    "default".into()
}

/// Read an experiment and check every arm's config and every scenario.
pub fn parse(text: &str) -> Result<Experiment, String> {
    let e: Experiment = toml::from_str(text).map_err(|err| err.to_string())?;
    for sc in &e.scenarios {
        if !sim::scenario::NAMES.contains(&sc.as_str()) {
            return Err(format!("unknown scenario {sc:?}"));
        }
    }
    for arm in &e.arms {
        if arm.arm == ArmKind::A && !arm.config.is_empty() {
            return Err(format!("{}: arm a has no brain settings", arm.label));
        }
        config_for(&e, arm, 0)?;
    }
    Ok(e)
}

/// Every key of `over` must already be in `base`, so a typo is an error
/// rather than a silently ignored setting.
fn merge(base: &mut toml::Table, over: &toml::Table, path: &str) -> Result<(), String> {
    for (k, v) in over {
        let at = format!("{path}{k}");
        match (base.get_mut(k), v) {
            (Some(toml::Value::Table(b)), toml::Value::Table(o)) => merge(b, o, &format!("{at}."))?,
            (Some(slot), _) => *slot = v.clone(),
            (None, _) => return Err(format!("unknown setting {at}")),
        }
    }
    Ok(())
}

/// The brain config arm `arm` runs with on `seed`.
pub fn config_for(e: &Experiment, arm: &ArmSpec, seed: u64) -> Result<BrainConfig, String> {
    let mut mean = crate::archetype::means(&e.archetype)?;
    if !arm.config.is_empty() {
        let mut t = toml::Table::try_from(&mean).map_err(|err| err.to_string())?;
        merge(&mut t, &arm.config, "")?;
        mean = t
            .try_into()
            .map_err(|err| format!("{}: {err}", arm.label))?;
    }
    Ok(if e.per_agent {
        crate::archetype::drawn(&mean, seed)
    } else {
        mean
    })
}

/// A baseline arm as a scenario driver: full effort, no log rows.
struct PolicyDriver<P> {
    policy: P,
}

impl<P: Policy> Driver for PolicyDriver<P> {
    fn decide(&mut self, obs: &Observation, cands: &[Candidate]) -> Decision {
        self.policy
            .choose(obs, cands)
            .map_or(Decision::Idle, |i| Decision::Act(i, FULL))
    }
    fn outcome(&mut self, _: Outcome) {}
    fn reflex_fired(&mut self, _: u64) {}
    fn log(&self) -> Option<&TickLog> {
        None
    }
}

/// Metrics for an arm without a brain: C, D, V and freeze do not exist.
pub fn baseline_metrics(r: &BrainResult) -> RunMetrics {
    let acted = r.decisions.saturating_sub(r.idle) as f32 / r.decisions.max(1) as f32;
    RunMetrics {
        survived: r.survived,
        ticks: r.ticks,
        decisions: r.decisions,
        active: acted,
        self_started: acted,
        mean_c: f32::NAN,
        mean_d: f32::NAN,
        mean_v: f32::NAN,
        freeze: f32::NAN,
        stuck_kicks: 0,
        reflexes: r.reflexes,
        new_things: r.new_things,
        tech: tech_held(&r.inventory),
    }
}

/// One arm on one scenario and seed.
pub fn run_arm(e: &Experiment, arm: &ArmSpec, scenario: &str, seed: u64) -> RunMetrics {
    let s = setup(scenario, seed);
    match arm.arm {
        ArmKind::A => {
            let mut d = PolicyDriver {
                policy: ArmA::default(),
            };
            let no_rows = &mut |_: &TickLog| unreachable!("arm A keeps no log rows");
            baseline_metrics(&run_scenario(s, &mut d, None, seed, no_rows))
        }
        ArmKind::C => {
            let config = config_for(e, arm, seed).expect("checked by parse");
            let mut m = Meter::default();
            let r = crate::brain::run_brain(s, config, seed, |row| m.push(row));
            m.finish(&r)
        }
    }
}

/// Every arm × scenario × seed, `threads` at a time, in a fixed order.
pub fn run(e: &Experiment, threads: usize) -> Vec<Row> {
    let jobs: Vec<(usize, &str, u64)> = (0..e.arms.len())
        .flat_map(|ai| {
            e.scenarios
                .iter()
                .flat_map(move |sc| (0..e.seeds).map(move |seed| (ai, sc.as_str(), seed)))
        })
        .collect();
    let out = crate::par::par_map(&jobs, threads, |&(ai, sc, seed)| {
        run_arm(e, &e.arms[ai], sc, seed)
    });
    let per = (e.seeds as usize).max(1);
    out.chunks(per)
        .zip(jobs.chunks(per))
        .map(|(runs, js)| Row {
            variant: e.arms[js[0].0].label.clone(),
            scenario: js[0].1.to_string(),
            summary: summarise(runs),
        })
        .collect()
}

/// The run header (spec §25, §26): versions, the experiment and every
/// arm's expected direction, fixed before the results exist.
pub fn header(e: &Experiment) -> serde_json::Value {
    let arms: Vec<serde_json::Value> = e
        .arms
        .iter()
        .map(|a| serde_json::json!({"label": a.label, "expected": a.expected, "config": a.config}))
        .collect();
    serde_json::json!({
        "header": true,
        "experiment": e.id,
        "question": e.question,
        "scenarios": e.scenarios,
        "seeds": e.seeds,
        "archetype": e.archetype,
        "per_agent": e.per_agent,
        "arms": arms,
        "defs_version": cognitive_core::DEFS_VERSION,
        "paper_version": cognitive_core::PAPER_VERSION,
        "spec_version": cognitive_core::SPEC_VERSION,
    })
}

/// The expected directions, then the results table.
pub fn markdown(e: &Experiment, rows: &[Row]) -> String {
    let mut t = format!(
        "## {}: {}\n\nExpected before the run (plan §5.5):\n\n",
        e.id, e.question
    );
    for a in &e.arms {
        t += &format!("- **{}**: {}\n", a.label, a.expected);
    }
    t += "\n";
    t + &crate::sweep::table(rows, e.seeds)
}
