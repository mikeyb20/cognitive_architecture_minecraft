//! Experiments (spec §26; plan §5): arms × scenarios × seeds from a TOML
//! file in `configs/experiments`, with each arm's expected direction
//! written down before the run (plan §5.5).
//!
//! Arm `a` is the behaviour tree; `b` and `bn` are the LLM loop without and
//! with the count bonus; arm `c` is the brain, with optional config
//! overrides for the ablations (spec §24 C−X: one flag each).

use baselines::Policy;
use cognitive_core::agent::{BrainConfig, Decision, TickLog};
use host_api::{Candidate, Observation, Outcome};
use llm::{LlmStats, Provider, Tier};
use serde::Deserialize;

use crate::FULL;
use crate::brain::{BrainResult, setup};
use crate::drive::{Driver, run_scenario};
use crate::mc::McArm;
use crate::sweep::{Meter, Row, RunMetrics, summarise, tech_held};

/// Which arm picks the actions (spec §24).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ArmKind {
    /// The behaviour tree.
    A,
    /// The LLM loop (#30).
    B,
    /// The LLM loop with the count bonus.
    BN,
    /// The brain.
    C,
}

impl ArmKind {
    /// The baseline arm, or `None` for the brain.
    fn baseline(self) -> Option<McArm> {
        match self {
            Self::A => Some(McArm::A),
            Self::B => Some(McArm::B),
            Self::BN => Some(McArm::BN),
            Self::C => None,
        }
    }
}

/// The LLM for arms B and B+N: its tier, and a maker run once per job so
/// every run starts with its own provider.
pub struct Llm<'a> {
    pub tier: Tier,
    pub make: &'a (dyn Fn() -> Result<Box<dyn Provider>, String> + Sync),
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
        if arm.arm != ArmKind::C && !arm.config.is_empty() {
            return Err(format!("{}: only arm c has brain settings", arm.label));
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
struct PolicyDriver {
    policy: Box<dyn Policy>,
}

impl Driver for PolicyDriver {
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

/// Metrics for an arm without a brain: C, D, V, freeze and the brain's
/// memory and thinking do not exist (arm B's LLM calls are counted apart).
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
        memory: f32::NAN,
        recall: f32::NAN,
        thinking: f32::NAN,
        plan_lag: f32::NAN,
    }
}

/// One arm on one scenario and seed, and its LLM use.
pub fn run_arm(
    e: &Experiment,
    arm: &ArmSpec,
    scenario: &str,
    seed: u64,
    llm: Option<&Llm>,
) -> Result<(RunMetrics, LlmStats), String> {
    let s = setup(scenario, seed);
    let Some(which) = arm.arm.baseline() else {
        let config = config_for(e, arm, seed)?;
        let mut m = Meter::default();
        let r = crate::brain::run_brain(s, config, seed, |row| m.push(row));
        return Ok((m.finish(&r), LlmStats::default()));
    };
    let (tier, provider) = match (which.needs_llm(), llm) {
        (true, Some(l)) => (l.tier, Some((l.make)()?)),
        (_, l) => (l.map_or(Tier::Local, |l| l.tier), None),
    };
    let mut d = PolicyDriver {
        policy: crate::mc::arm(which, tier, provider)?,
    };
    let no_rows = &mut |_: &TickLog| unreachable!("baseline arms keep no log rows");
    let r = run_scenario(s, &mut d, None, seed, no_rows);
    let used = d.policy.llm_stats().cloned().unwrap_or_default();
    Ok((baseline_metrics(&r), used))
}

/// The results table and each arm's total LLM use (plan §5.5).
#[derive(Debug, Clone, PartialEq)]
pub struct Results {
    pub rows: Vec<Row>,
    pub llm: Vec<(String, LlmStats)>,
}

/// Every arm × scenario × seed, in a fixed order. Runs go `threads` at a
/// time, or one at a time when an arm calls an LLM (one provider, one
/// cache file).
pub fn run(e: &Experiment, threads: usize, llm: Option<&Llm>) -> Result<Results, String> {
    let calls_llm = e
        .arms
        .iter()
        .any(|a| a.arm.baseline().is_some_and(McArm::needs_llm));
    let threads = if calls_llm { 1 } else { threads };
    let jobs: Vec<(usize, &str, u64)> = (0..e.arms.len())
        .flat_map(|ai| {
            e.scenarios
                .iter()
                .flat_map(move |sc| (0..e.seeds).map(move |seed| (ai, sc.as_str(), seed)))
        })
        .collect();
    // `par_map` needs a default value, so an error rides along as a string.
    let out = crate::par::par_map(&jobs, threads, |&(ai, sc, seed)| {
        match run_arm(e, &e.arms[ai], sc, seed, llm) {
            Ok((m, used)) => (m, used, None),
            Err(err) => (
                RunMetrics::default(),
                LlmStats::default(),
                Some(format!("{}: {err}", e.arms[ai].label)),
            ),
        }
    });
    if let Some(err) = out.iter().find_map(|o| o.2.clone()) {
        return Err(err);
    }
    let per = (e.seeds as usize).max(1);
    let rows = out
        .chunks(per)
        .zip(jobs.chunks(per))
        .map(|(runs, js)| Row {
            variant: e.arms[js[0].0].label.clone(),
            scenario: js[0].1.to_string(),
            summary: summarise(&runs.iter().map(|r| r.0).collect::<Vec<_>>()),
        })
        .collect();
    let mut llm_use: Vec<(String, LlmStats)> = Vec::new();
    for ((_, used, _), (ai, _, _)) in out.iter().zip(&jobs) {
        let label = &e.arms[*ai].label;
        match llm_use.iter_mut().find(|(l, _)| l == label) {
            Some((_, total)) => total.add(used),
            None => llm_use.push((label.clone(), used.clone())),
        }
    }
    llm_use.retain(|(_, s)| s.calls + s.fallbacks > 0);
    Ok(Results { rows, llm: llm_use })
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

/// The expected directions, the results table, then LLM use per arm.
pub fn markdown(e: &Experiment, res: &Results) -> String {
    let mut t = format!(
        "## {}: {}\n\nExpected before the run (plan §5.5):\n\n",
        e.id, e.question
    );
    for a in &e.arms {
        t += &format!("- **{}**: {}\n", a.label, a.expected);
    }
    t += "\n";
    t += &crate::sweep::table(&res.rows, e.seeds);
    for (label, u) in &res.llm {
        t += &format!(
            "\nLLM use, {label}: {} calls ({} cached, {} fallbacks, {} unusable), {} input and {} output tokens, ${:.4}.\n",
            u.calls, u.cached, u.fallbacks, u.unparsed, u.input_tokens, u.output_tokens, u.cost_usd
        );
    }
    t
}
