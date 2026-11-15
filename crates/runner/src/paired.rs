//! Paired comparisons by seed (07 M1, M4; paper §5.7): every arm in an
//! experiment runs the same seeds, and since the sim gives each source of
//! chance its own stream, seed `s` is the same world for every arm until
//! their choices differ. So each arm is compared with the experiment's
//! reference arm seed by seed, and per-run results are written out for
//! later analysis (plan §5.5: results fixed and reported per arm).

use crate::metrics::RunMetrics;

/// One run: which arm, scenario and seed, and what it measured.
#[derive(Debug, Clone, PartialEq)]
pub struct Run {
    pub arm: String,
    pub scenario: String,
    pub seed: u64,
    pub m: RunMetrics,
}

/// Columns of the per-run CSV.
const RUN_HEADER: [&str; 11] = [
    "arm",
    "scenario",
    "seed",
    "survived",
    "ticks",
    "decisions",
    "active",
    "self_started",
    "new_things",
    "tech",
    "shelter_at",
];

/// Every run as one CSV line, in run order.
pub fn runs_csv(runs: &[Run]) -> String {
    let mut t = RUN_HEADER.join(",") + "\n";
    for r in runs {
        let shelter = r.m.shelter_at.map_or(String::new(), |s| s.to_string());
        t += &format!(
            "\"{}\",{},{},{},{},{},{:.4},{:.4},{},{},{}\n",
            r.arm.replace('"', "'"),
            r.scenario,
            r.seed,
            u8::from(r.m.survived),
            r.m.ticks,
            r.m.decisions,
            r.m.active,
            r.m.self_started,
            r.m.new_things,
            r.m.tech,
            shelter,
        );
    }
    t
}

/// An exact two-sided sign test: the chance of a split at least as uneven
/// as `wins` vs `losses` if each pair were a fair coin (ties dropped).
/// 1.0 when there are no untied pairs.
pub fn sign_test(wins: u32, losses: u32) -> f64 {
    let n = wins + losses;
    if n == 0 {
        return 1.0;
    }
    let k = wins.min(losses);
    // P(X <= k) for X ~ Binomial(n, 1/2), summed in log space.
    let ln_half_n = f64::from(n) * 0.5f64.ln();
    let mut ln_choose = 0.0; // ln C(n, 0)
    let mut tail = 0.0;
    for i in 0..=k {
        if i > 0 {
            ln_choose += f64::from(n - i + 1).ln() - f64::from(i).ln();
        }
        tail += (ln_choose + ln_half_n).exp();
    }
    (2.0 * tail).min(1.0)
}

/// One arm against the reference on one scenario, over the seeds both ran.
#[derive(Debug, Clone, PartialEq)]
pub struct Pair {
    pub arm: String,
    pub scenario: String,
    pub n: u32,
    /// Seeds where only this arm survived, and where only the reference did.
    pub only_arm: u32,
    pub only_ref: u32,
    /// Mean of (arm − reference) new things, and seeds above and below.
    pub new_diff: f64,
    pub new_up: u32,
    pub new_down: u32,
}

impl Pair {
    pub fn survival_p(&self) -> f64 {
        sign_test(self.only_arm, self.only_ref)
    }
    pub fn new_p(&self) -> f64 {
        sign_test(self.new_up, self.new_down)
    }
}

/// Every non-reference arm paired with `reference` by scenario and seed,
/// in the order the arms and scenarios first appear.
pub fn pairs(runs: &[Run], reference: &str) -> Vec<Pair> {
    let mut keys: Vec<(&str, &str)> = Vec::new();
    for r in runs.iter().filter(|r| r.arm != reference) {
        let k = (r.arm.as_str(), r.scenario.as_str());
        if !keys.contains(&k) {
            keys.push(k);
        }
    }
    keys.into_iter()
        .map(|(arm, sc)| pair(runs, reference, arm, sc))
        .collect()
}

fn pair(runs: &[Run], reference: &str, arm: &str, sc: &str) -> Pair {
    let find = |a: &str, seed| {
        runs.iter()
            .find(|r| r.arm == a && r.scenario == sc && r.seed == seed)
    };
    let mut p = Pair {
        arm: arm.into(),
        scenario: sc.into(),
        n: 0,
        only_arm: 0,
        only_ref: 0,
        new_diff: 0.0,
        new_up: 0,
        new_down: 0,
    };
    let mut sum = 0.0;
    for x in runs.iter().filter(|r| r.arm == arm && r.scenario == sc) {
        let Some(y) = find(reference, x.seed) else {
            continue;
        };
        p.n += 1;
        p.only_arm += u32::from(x.m.survived && !y.m.survived);
        p.only_ref += u32::from(y.m.survived && !x.m.survived);
        let d = x.m.new_things as f64 - y.m.new_things as f64;
        sum += d;
        p.new_up += u32::from(d > 0.0);
        p.new_down += u32::from(d < 0.0);
    }
    p.new_diff = if p.n == 0 { 0.0 } else { sum / f64::from(p.n) };
    p
}

/// The paired table in Markdown.
pub fn markdown(pairs: &[Pair], reference: &str) -> String {
    let mut t = format!(
        "\nPaired by seed against **{reference}** (exact two-sided sign test; ties dropped):\n\n\
         | arm | scenario | n | survived only arm / only ref | p | new things, mean diff | seeds up / down | p |\n\
         |---|---|---|---|---|---|---|---|\n"
    );
    for p in pairs {
        t += &format!(
            "| {} | {} | {} | {} / {} | {:.3} | {:+.1} | {} / {} | {:.3} |\n",
            p.arm,
            p.scenario,
            p.n,
            p.only_arm,
            p.only_ref,
            p.survival_p(),
            p.new_diff,
            p.new_up,
            p.new_down,
            p.new_p()
        );
    }
    t
}

/// A short, stable hash of a config's text (FNV-1a, 64 bits), so a
/// results folder records exactly which file it ran.
pub fn config_hash(text: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(arm: &str, seed: u64, survived: bool, new_things: usize) -> Run {
        Run {
            arm: arm.into(),
            scenario: "s".into(),
            seed,
            m: RunMetrics {
                survived,
                new_things,
                ..RunMetrics::default()
            },
        }
    }

    #[test]
    fn sign_test_matches_hand_worked_binomials() {
        // 5 vs 0: 2 · (1/2)^5 = 0.0625.
        assert!((sign_test(5, 0) - 0.0625).abs() < 1e-12);
        assert!((sign_test(0, 5) - 0.0625).abs() < 1e-12);
        // 8 vs 2: 2 · (1 + 10 + 45) / 1024 = 0.109375.
        assert!((sign_test(8, 2) - 0.109_375).abs() < 1e-12);
        // 1 vs 1: capped at 1.
        assert_eq!(sign_test(1, 1), 1.0);
        assert_eq!(sign_test(0, 0), 1.0);
        // 20 vs 0: 2 / 2^20.
        assert!((sign_test(20, 0) - 2.0 / 1_048_576.0).abs() < 1e-15);
    }

    #[test]
    fn sign_test_is_a_probability_and_falls_as_the_split_widens() {
        let mut last = 1.0;
        for w in 50..=100 {
            let p = sign_test(w, 100 - w);
            assert!((0.0..=1.0).contains(&p));
            assert!(p <= last + 1e-12, "{w}: {p} > {last}");
            last = p;
        }
        // Large n stays finite (no overflow in the binomial).
        assert!(sign_test(400, 600) > 0.0);
    }

    #[test]
    fn pairs_count_discordant_seeds_and_paired_differences() {
        let runs = [
            run("ref", 0, true, 10),
            run("ref", 1, false, 10),
            run("ref", 2, true, 10),
            run("x", 0, false, 13),
            run("x", 1, true, 7),
            run("x", 2, true, 10),
            run("x", 3, true, 99), // no reference run: skipped
        ];
        let ps = pairs(&runs, "ref");
        assert_eq!(ps.len(), 1);
        let p = &ps[0];
        assert_eq!((p.arm.as_str(), p.scenario.as_str(), p.n), ("x", "s", 3));
        assert_eq!((p.only_arm, p.only_ref), (1, 1));
        assert_eq!((p.new_up, p.new_down), (1, 1));
        assert!(p.new_diff.abs() < 1e-12);
        assert_eq!(p.survival_p(), 1.0);
        assert_eq!(p.new_p(), 1.0);
        let md = markdown(&ps, "ref");
        assert!(
            md.contains("| x | s | 3 | 1 / 1 | 1.000 | +0.0 | 1 / 1 | 1.000 |"),
            "{md}"
        );
        assert!(
            pairs(&runs, "nobody").len() == 2,
            "every arm is then paired"
        );
    }

    #[test]
    fn a_lopsided_pair_gives_small_p_values() {
        // Five seeds where only the arm survives and finds 2 more things,
        // and one where both survive and tie: a 5 vs 0 split, p = 0.0625.
        let mut runs: Vec<Run> = (0..5).map(|s| run("ref", s, false, 10)).collect();
        runs.extend((0..5).map(|s| run("x", s, true, 12)));
        runs.push(run("ref", 5, true, 10));
        runs.push(run("x", 5, true, 10));
        let p = &pairs(&runs, "ref")[0];
        assert_eq!((p.only_arm, p.only_ref, p.new_up, p.new_down), (5, 0, 5, 0));
        assert!((p.survival_p() - 0.0625).abs() < 1e-12);
        assert!((p.new_p() - 0.0625).abs() < 1e-12);
        assert!((p.new_diff - 10.0 / 6.0).abs() < 1e-12);
    }

    #[test]
    fn a_pair_with_no_shared_seeds_is_empty() {
        let p = pair(&[run("x", 4, true, 1)], "ref", "x", "s");
        assert_eq!((p.n, p.new_diff), (0, 0.0));
    }

    #[test]
    fn runs_csv_has_one_line_per_run() {
        let mut r = run("C \"full\"", 3, true, 42);
        r.m.shelter_at = Some(1100);
        let t = runs_csv(&[r, run("A", 4, false, 0)]);
        let lines: Vec<&str> = t.lines().collect();
        assert_eq!(lines[0], RUN_HEADER.join(","));
        assert_eq!(lines[1], "\"C 'full'\",s,3,1,0,0,0.0000,0.0000,42,0,1100");
        assert_eq!(lines[2], "\"A\",s,4,0,0,0,0.0000,0.0000,0,0,");
    }

    #[test]
    fn config_hash_is_fnv1a() {
        // FNV-1a 64 of "" and "a", published test vectors.
        assert_eq!(config_hash(""), "cbf29ce484222325");
        assert_eq!(config_hash("a"), "af63dc4c8601ec8c");
    }
}
