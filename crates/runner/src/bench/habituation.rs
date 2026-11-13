//! B2 Habituation (benchmarks spec v0.1): h on memory items (01 §5.1;
//! spec §17) and its effect on pull, `g(C)·(1 − h)` (01 §1.1). Signatures
//! from Rankin et al. (2009), Neurobiology of Learning and Memory 92, 135–138.
//! H1–H4 measure the target's own response, 1 − h; H5–H6 measure the
//! agent-level pull, because 01 §1.1 also feeds the current target's h into C.

use super::Check;
use cognitive_core::activation::signals::NoveltyWindow;
use cognitive_core::activation::{
    Activation, Curiosity, CuriosityParams, PowerActivation, SignalWeights, Signals,
};
use cognitive_core::memory::{MemoryParams, MemoryStore, Source, Tier};

pub const ID: &str = "habituation";

/// Exposures per series and the inter-stimulus interval (spec B2).
pub const EXPOSURES: u32 = 30;
pub const ISI: u64 = 10;
/// Recovery gaps for H2, in ticks.
pub const GAPS: [u64; 3] = [0, 500, 5_000];
/// ISIs compared in H3.
pub const ISI_FAST: u64 = 5;
pub const ISI_SLOW: u64 = 50;
/// Rest between H4 series, long enough for full recovery.
pub const RECOVERY: u64 = 5_000;
/// Cap on one H4 series, so a broken model cannot loop forever.
pub const MAX_SERIES: u32 = 1_000;

/// One remembered stimulus in a fresh store.
struct Stim {
    store: MemoryStore,
    id: usize,
}

impl Stim {
    fn new() -> Self {
        let mut store = MemoryStore::new(MemoryParams::default());
        let (id, _) = store.encode(
            "block:a",
            Tier::Episodic,
            Default::default(),
            0,
            Source::Perception,
        );
        Self { store, id }
    }

    /// Present the stimulus at `t` with nothing new in it; returns the
    /// response it met, 1 − h, before this exposure habituates it.
    fn present(&mut self, t: u64) -> f32 {
        let r = 1.0 - self.store.habituation(self.id, t);
        self.store.expose(self.id, t, false);
        r
    }

    /// A series of `n` exposures `isi` apart from `t0`; returns the responses.
    fn series(&mut self, t0: u64, n: u32, isi: u64) -> Vec<f32> {
        (0..u64::from(n))
            .map(|k| self.present(t0 + k * isi))
            .collect()
    }
}

/// H1: responses fall with repetition.
fn h1() -> Check {
    decrement(&Stim::new().series(0, EXPOSURES, ISI))
}

/// H1's judgment on a response series: last ÷ first, passing when it is at
/// most half and never rises.
fn decrement(r: &[f32]) -> Check {
    let monotone = r.windows(2).all(|p| p[1] <= p[0]);
    let ratio = f64::from(r[r.len() - 1] / r[0]);
    check(
        "H1 decrement: last ÷ first response",
        ratio,
        "≤ 0.5 and monotone",
        monotone && ratio <= 0.5,
    )
}

/// H2: share of the lost response recovered after each gap.
fn h2() -> Vec<Check> {
    let rec: Vec<f64> = GAPS
        .iter()
        .map(|&gap| {
            let mut s = Stim::new();
            let end_t = u64::from(EXPOSURES - 1) * ISI;
            let r = s.series(0, EXPOSURES, ISI);
            let floor = 1.0 - s.store.habituation(s.id, end_t);
            let after = s.present(end_t + gap.max(1));
            f64::from((after - floor) / (r[0] - floor))
        })
        .collect();
    let grows = rec.windows(2).all(|p| p[1] >= p[0]);
    let last = rec[rec.len() - 1];
    vec![check(
        "H2 spontaneous recovery after 5,000 ticks (share)",
        last,
        "≥ 0.5, growing with the gap",
        grows && last >= 0.5,
    )]
}

/// H3: shorter ISI habituates faster.
fn h3() -> Check {
    let fast = Stim::new().series(0, EXPOSURES, ISI_FAST);
    let slow = Stim::new().series(0, EXPOSURES, ISI_SLOW);
    let d = f64::from(slow[slow.len() - 1] - fast[fast.len() - 1]);
    above_zero("H3 frequency: final response, slow − fast ISI", d)
}

/// A signature that passes when its measured difference is positive.
fn above_zero(name: &str, d: f64) -> Check {
    check(name, d, "> 0", d > 0.0)
}

/// H4: exposures to halve the response in three series with full recovery
/// between them; potentiation means the count falls.
fn h4() -> Check {
    potentiation(halving_counts(RECOVERY))
}

/// Exposures before the response falls to half its start, per series,
/// with `gap` ticks of rest between series.
fn halving_counts(gap: u64) -> [u32; 3] {
    let mut s = Stim::new();
    let mut t = 0;
    let mut counts = Vec::new();
    for _ in 0..3 {
        let start = 1.0 - s.store.habituation(s.id, t);
        let mut n = 0;
        while n < MAX_SERIES && s.present(t) > 0.5 * start {
            n += 1;
            t += ISI;
        }
        counts.push(n);
        t += gap;
    }
    [counts[0], counts[1], counts[2]]
}

/// H4's judgment: series 1 − series 3 exposures to half.
fn potentiation(counts: [u32; 3]) -> Check {
    let drop = f64::from(counts[0]) - f64::from(counts[2]);
    above_zero(
        "H4 potentiation: exposures to 50%, series 1 − series 3",
        drop,
    )
}

/// Agent-level pull model: one C with only habituation and novelty signals
/// (defaults of spec §19), g(C) = C² (01 §1.4).
struct Agent {
    c: Curiosity,
    k: SignalWeights,
    nov: NoveltyWindow,
    g: PowerActivation,
}

impl Agent {
    fn new() -> Self {
        Self {
            c: Curiosity::new(CuriosityParams::default()),
            k: SignalWeights::default(),
            nov: NoveltyWindow::default(),
            g: PowerActivation::default(),
        }
    }

    /// One tick with the current target's h and whether something new was seen.
    fn tick(&mut self, h_current: f32, new: bool) {
        let s = Signals {
            novelty: self.nov.record(u32::from(new), 1),
            habituation: h_current,
            energy: 1.0,
            ..Signals::default()
        };
        let target = self.c.target(self.k.weighted(&s), 0.0);
        self.c.drift(target, 0.0);
    }

    fn pull(&self, h_target: f32) -> f32 {
        self.g.g(self.c.value()) * (1.0 - h_target)
    }
}

/// 300 ticks attending to A, exposed every ISI ticks (or nothing, as control).
fn attend_a(with_a: bool) -> (Agent, Stim) {
    let mut agent = Agent::new();
    let mut a = Stim::new();
    for t in 0..300 {
        if with_a && t % ISI == 0 {
            a.present(t);
        }
        let h = if with_a {
            a.store.habituation(a.id, t)
        } else {
            0.0
        };
        agent.tick(h, false);
    }
    (agent, a)
}

/// H5: pull of a fresh B right after habituating to A, vs no prior A.
fn h5() -> Check {
    let (after_a, _) = attend_a(true);
    let (control, _) = attend_a(false);
    let ratio = f64::from(after_a.pull(0.0) / control.pull(0.0));
    check(
        "H5 stimulus specificity: pull of fresh B after A ÷ without A",
        ratio,
        "≥ 0.95",
        ratio >= 0.95,
    )
}

/// H6: response to A after one novel B, vs the same ticks without B.
fn h6() -> Check {
    let retest = |show_b: bool| {
        let (mut agent, a) = attend_a(true);
        for t in 300..305 {
            let b_now = show_b && t == 300;
            let h = if b_now {
                0.0
            } else {
                a.store.habituation(a.id, t)
            };
            agent.tick(h, b_now);
        }
        agent.pull(a.store.habituation(a.id, 305))
    };
    let ratio = f64::from(retest(true) / retest(false));
    check(
        "H6 dishabituation: pull of A after novel B ÷ without B",
        ratio,
        "≥ 1.05",
        ratio >= 1.05,
    )
}

fn check(name: &str, measured: f64, target: &str, pass: bool) -> Check {
    Check {
        bench: ID,
        name: name.into(),
        measured,
        target: target.into(),
        pass,
    }
}

/// The B2 signatures (deterministic, so no seeds).
pub fn checks() -> Vec<Check> {
    let mut out = vec![h1()];
    out.extend(h2());
    out.extend([h3(), h4(), h5(), h6()]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_exposures_match_hand_worked_values() {
        // h: 0 → 0.1 → 0.1·0.999^10 + 0.1·(1 − ...) between exposures.
        let r = Stim::new().series(0, 3, 10);
        assert!((r[0] - 1.0).abs() < 1e-6);
        let h1 = 0.1f32 * 0.999f32.powi(10);
        assert!((r[1] - (1.0 - h1)).abs() < 1e-5);
        let h2 = (h1 + 0.1 * (1.0 - h1)) * 0.999f32.powi(10);
        assert!((r[2] - (1.0 - h2)).abs() < 1e-5);
    }

    #[test]
    fn responses_stay_in_unit_interval() {
        for isi in [1, 5, 50, 500] {
            for r in Stim::new().series(0, 200, isi) {
                assert!((0.0..=1.0).contains(&r));
            }
        }
    }

    #[test]
    fn pull_without_prior_target_is_g_of_base() {
        let (control, _) = attend_a(false);
        assert!(
            (control.pull(0.0) - 0.16).abs() < 1e-4,
            "C_base 0.4, g = C²"
        );
        assert!((control.pull(0.5) - 0.08).abs() < 1e-4);
    }

    /// Independent re-statement of spec §17: responses 1 − h to `n`
    /// exposures `isi` apart, h decaying by 0.999 per tick between them.
    fn model(n: u32, isi: u64, h0: f64) -> (Vec<f64>, f64) {
        let mut h = h0;
        let mut r = Vec::new();
        for k in 0..n {
            if k > 0 {
                h *= 0.999f64.powi(isi as i32);
            }
            r.push(1.0 - h);
            h += 0.1 * (1.0 - h);
        }
        (r, h)
    }

    #[test]
    fn h1_to_h4_match_the_independent_model() {
        let rows = checks();
        let (r, _) = model(EXPOSURES, ISI, 0.0);
        assert!((rows[0].measured - r[29] / r[0]).abs() < 1e-4, "H1");
        assert!(
            (rows[1].measured - (1.0 - 0.999f64.powi(5000))).abs() < 1e-4,
            "H2"
        );
        let (fast, _) = model(EXPOSURES, ISI_FAST, 0.0);
        let (slow, _) = model(EXPOSURES, ISI_SLOW, 0.0);
        assert!(
            (rows[2].measured - (slow[29] - fast[29])).abs() < 1e-4,
            "H3"
        );
        // H4: count exposures above half the starting response, per series.
        let count = |h0: f64| {
            let start = 1.0 - h0;
            let (r, _) = model(200, ISI, h0);
            r.iter().take_while(|&&x| x > 0.5 * start).count()
        };
        // Each series ends after its count of exposures plus the one that
        // met a half response, then decays for the gap.
        let counts = |gap: u64| {
            let mut h0 = 0.0;
            let mut want = Vec::new();
            for _ in 0..3 {
                let n = count(h0);
                want.push(n as u32);
                h0 = model(n as u32 + 1, ISI, h0).1 * 0.999f64.powi((gap + ISI) as i32);
            }
            want
        };
        assert_eq!(halving_counts(RECOVERY).to_vec(), counts(RECOVERY), "H4");
        assert_eq!(counts(RECOVERY), [7, 7, 7]);
        // A short rest leaves h high, so later series start lower and halve
        // sooner; this pins the start level and the rest.
        assert_eq!(halving_counts(200).to_vec(), counts(200), "H4 short rest");
        assert_ne!(counts(200), [7, 7, 7]);
        assert_eq!(rows[3].measured, 0.0);
        let pass: Vec<bool> = rows.iter().map(|r| r.pass).collect();
        assert_eq!(pass, [true, true, true, false, false, true]);
    }

    #[test]
    fn h2_recovery_grows_with_the_gap() {
        // Shares for gaps 0 (read as 1 tick), 500 and 5,000.
        let shares: Vec<f64> = [1, 500, 5000]
            .iter()
            .map(|&g| 1.0 - 0.999f64.powi(g))
            .collect();
        assert!(shares.windows(2).all(|p| p[1] > p[0]));
        assert!((checks()[1].measured - shares[2]).abs() < 1e-4);
    }

    /// Independent agent-level model: C drifts at 0.1 toward
    /// 0.4 − 0.3·h_current + 0.4·novelty (other signals idle, spec §19).
    fn agent_model(show_b: bool, retest: bool) -> (f64, f64) {
        let mut c = 0.4f64;
        let mut h_stored = 0.0f64;
        let mut last = 0u64;
        let h_at = |hs: f64, last: u64, t: u64| hs * 0.999f64.powi((t - last) as i32);
        let mut window: Vec<u32> = Vec::new();
        let step = |c: &mut f64, h: f64, new: bool, window: &mut Vec<u32>| {
            window.push(u32::from(new));
            if window.len() > 20 {
                window.remove(0);
            }
            let nov = f64::from(window.iter().sum::<u32>()) / window.len() as f64;
            let target = (0.4 - 0.3 * h + 0.4 * nov).clamp(0.0, 1.0);
            *c += 0.1 * (target - *c);
        };
        for t in 0..300u64 {
            if t % ISI == 0 {
                let h = h_at(h_stored, last, t);
                h_stored = h + 0.1 * (1.0 - h);
                last = t;
            }
            step(&mut c, h_at(h_stored, last, t), false, &mut window);
        }
        let pull_b = c * c;
        if !retest {
            return (pull_b, 0.0);
        }
        for t in 300..305u64 {
            let b_now = show_b && t == 300;
            let h = if b_now { 0.0 } else { h_at(h_stored, last, t) };
            step(&mut c, h, b_now, &mut window);
        }
        (pull_b, c * c * (1.0 - h_at(h_stored, last, 305)))
    }

    #[test]
    fn h5_and_h6_match_the_independent_model() {
        let rows = checks();
        let (pull_b, _) = agent_model(false, false);
        assert!(
            (rows[4].measured - pull_b / 0.16).abs() < 1e-3,
            "H5 {}",
            rows[4].measured
        );
        let (_, with_b) = agent_model(true, true);
        let (_, without_b) = agent_model(false, true);
        assert!(
            (rows[5].measured - with_b / without_b).abs() < 1e-3,
            "H6 {}",
            rows[5].measured
        );
    }

    #[test]
    fn judgments_follow_their_targets() {
        let d = decrement(&[2.0, 1.0]);
        assert_eq!((d.measured, d.pass), (0.5, true));
        assert!(!decrement(&[1.0, 0.6]).pass, "too little decrement");
        assert!(!decrement(&[1.0, 0.1, 0.2]).pass, "not monotone");
        let p = potentiation([9, 8, 6]);
        assert_eq!((p.measured, p.pass), (3.0, true));
        assert!(!potentiation([6, 6, 6]).pass);
        assert!(!above_zero("x", 0.0).pass);
    }

    #[test]
    fn a_broken_response_cannot_loop_forever() {
        // A series that never halves stops at the cap.
        assert_eq!(MAX_SERIES, 1_000);
    }

    #[test]
    fn every_signature_is_reported_once() {
        let rows = checks();
        let names: Vec<&str> = rows.iter().map(|r| &r.name[..2]).collect();
        assert_eq!(names, ["H1", "H2", "H3", "H4", "H5", "H6"]);
    }
}
