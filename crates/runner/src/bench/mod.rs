//! Psychology benchmarks: each tests one 01 relation against a result that
//! psychology has already measured, without needing the agent to survive
//! (decision #60; benchmarks spec v0.1, B1–B3). A failure is a finding, not a
//! bug to tune away: changing a definition still needs a DEF CHANGE.

pub mod habituation;
pub mod matching;

/// One measured signature and whether the model shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct Check {
    pub bench: &'static str,
    pub name: String,
    pub measured: f64,
    pub target: String,
    pub pass: bool,
}

/// Benchmark ids, in report order.
pub fn ids() -> Vec<&'static str> {
    vec![matching::ID, habituation::ID]
}

/// Run one benchmark by id over `seeds` seeds (benchmarks spec v0.1).
pub fn run(id: &str, seeds: u64) -> Option<Vec<Check>> {
    match id {
        matching::ID => Some(matching::checks(seeds)),
        habituation::ID => Some(habituation::checks()),
        _ => None,
    }
}

/// Markdown table: one row per signature.
pub fn markdown(id: &str, checks: &[Check]) -> String {
    let passed = checks.iter().filter(|c| c.pass).count();
    let mut s = format!(
        "## BENCH {id}: {passed} of {} signatures pass\n\n| signature | measured | target | pass |\n|---|---|---|---|\n",
        checks.len()
    );
    for c in checks {
        s.push_str(&format!(
            "| {} | {:.3} | {} | {} |\n",
            c.name,
            c.measured,
            c.target,
            if c.pass { "yes" } else { "**no**" }
        ));
    }
    s
}

/// CSV with the same rows.
pub fn csv(checks: &[Check]) -> String {
    let mut s = String::from("bench,signature,measured,target,pass\n");
    for c in checks {
        s.push_str(&format!(
            "{},{},{:.6},\"{}\",{}\n",
            c.bench, c.name, c.measured, c.target, c.pass
        ));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_counts_passes_and_flags_failures() {
        let rows = vec![
            Check {
                bench: "x",
                name: "a".into(),
                measured: 0.5,
                target: "≥ 0.4".into(),
                pass: true,
            },
            Check {
                bench: "x",
                name: "b".into(),
                measured: 0.1,
                target: "≥ 0.4".into(),
                pass: false,
            },
        ];
        let md = markdown("x", &rows);
        assert!(md.contains("1 of 2 signatures pass"));
        assert!(md.contains("| a | 0.500 | ≥ 0.4 | yes |"));
        assert!(md.contains("| b | 0.100 | ≥ 0.4 | **no** |"));
        let c = csv(&rows);
        assert_eq!(c.lines().count(), 3);
        assert!(c.contains("x,b,0.100000,\"≥ 0.4\",false"));
    }

    #[test]
    fn run_dispatches_by_id() {
        assert_eq!(run(habituation::ID, 1).unwrap().len(), 6);
        assert_eq!(run(matching::ID, 1).unwrap().len(), 9);
    }

    #[test]
    fn unknown_id_is_none() {
        assert!(run("nope", 1).is_none());
        assert_eq!(ids(), vec!["matching", "habituation"]);
    }
}
