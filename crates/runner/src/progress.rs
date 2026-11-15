//! A progress line for experiments with LLM arms (plan §5.5; Tier A runs
//! take hours on a CPU): which LLM run, calls so far, seconds per call,
//! time elapsed and an estimate of the time left. The CLI prints it to
//! stderr, so results on stdout and in files are unchanged.

use llm::{LlmError, Prompt, Provider, Reply};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Counts of LLM runs and calls; the arithmetic behind the line.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Tally {
    /// LLM runs in the whole experiment, and how many have started.
    pub total_runs: u64,
    pub started: u64,
    /// Calls in the runs that have finished, and in the current one.
    pub finished_calls: u64,
    pub run_calls: u64,
    /// Seconds spent waiting on calls, over every call so far.
    pub call_secs: f64,
}

impl Tally {
    pub fn new(total_runs: u64) -> Self {
        Self {
            total_runs,
            ..Self::default()
        }
    }

    /// A new LLM run starts (the previous one, if any, has finished).
    pub fn start_run(&mut self) {
        self.finished_calls += self.run_calls;
        self.run_calls = 0;
        self.started += 1;
    }

    /// One call took `secs`.
    pub fn record(&mut self, secs: f64) {
        self.run_calls += 1;
        self.call_secs += secs;
    }

    pub fn calls(&self) -> u64 {
        self.finished_calls + self.run_calls
    }

    /// Seconds left: the calls a run makes on average (from finished runs)
    /// times the runs still to go, less the current run's calls so far,
    /// at the mean seconds per call. `None` until one run has finished.
    pub fn eta_secs(&self) -> Option<f64> {
        let finished = self.started.saturating_sub(1);
        if finished == 0 || self.calls() == 0 {
            return None;
        }
        let per_run = self.finished_calls as f64 / finished as f64;
        let to_go = self.total_runs.saturating_sub(self.started) as f64;
        let this_run = (per_run - self.run_calls as f64).max(0.0);
        Some((this_run + to_go * per_run) * self.call_secs / self.calls() as f64)
    }

    /// The progress line, after `elapsed` seconds.
    pub fn line(&self, elapsed: f64) -> String {
        let per_call = self.call_secs / self.calls().max(1) as f64;
        let left = self.eta_secs().map_or_else(
            || "time left known after the first run".to_string(),
            |s| format!("about {} left", clock(s)),
        );
        format!(
            "LLM run {}/{}, call {} | {per_call:.1} s/call | {} elapsed | {left}",
            self.started,
            self.total_runs,
            self.calls(),
            clock(elapsed)
        )
    }
}

/// Seconds as "42s", "3m05s" or "2h07m".
pub fn clock(secs: f64) -> String {
    let s = secs.max(0.0).round() as u64;
    match (s / 3600, s / 60 % 60, s % 60) {
        (0, 0, sec) => format!("{sec}s"),
        (0, m, sec) => format!("{m}m{sec:02}s"),
        (h, m, _) => format!("{h}h{m:02}m"),
    }
}

/// The tally shared by every run's provider, and when the experiment began.
pub type Shared = Arc<Mutex<(Tally, Instant)>>;

/// A provider that times each call and prints the progress line.
pub struct Counted<P> {
    inner: P,
    shared: Shared,
}

impl<P: Provider> Counted<P> {
    /// Wraps the provider for one new LLM run.
    pub fn start(inner: P, shared: Shared) -> Self {
        if let Ok(mut g) = shared.lock() {
            g.0.start_run();
        }
        Self { inner, shared }
    }
}

impl<P: Provider> Provider for Counted<P> {
    fn id(&self) -> String {
        self.inner.id()
    }

    fn complete(&mut self, prompt: &Prompt) -> Result<Reply, LlmError> {
        let t = Instant::now();
        let reply = self.inner.complete(prompt);
        if let Ok(mut g) = self.shared.lock() {
            g.0.record(t.elapsed().as_secs_f64());
            eprint!("\r{}   ", g.0.line(g.1.elapsed().as_secs_f64()));
        }
        reply
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eta_waits_for_a_finished_run_then_uses_its_mean() {
        let mut t = Tally::new(4);
        assert_eq!(t.eta_secs(), None);
        t.start_run();
        for _ in 0..10 {
            t.record(2.0);
        }
        assert_eq!(t.eta_secs(), None, "no run has finished yet");
        assert_eq!(t.calls(), 10);
        t.start_run();
        for _ in 0..4 {
            t.record(4.0);
        }
        // 10 calls per run; this run has 6 to go, then 2 more runs of 10:
        // 26 calls at (20 + 16) / 14 s each.
        let want = 26.0 * 36.0 / 14.0;
        assert!((t.eta_secs().unwrap() - want).abs() < 1e-9);
        // A run past the mean has nothing left of its own.
        for _ in 0..20 {
            t.record(1.0);
        }
        let want = 20.0 * 56.0 / 34.0;
        assert!((t.eta_secs().unwrap() - want).abs() < 1e-9);
    }

    #[test]
    fn eta_is_zero_on_the_last_run_past_its_mean() {
        let mut t = Tally::new(2);
        t.start_run();
        t.record(1.0);
        t.start_run();
        t.record(1.0);
        assert_eq!(t.eta_secs(), Some(0.0));
        assert_eq!((t.started, t.finished_calls, t.run_calls), (2, 1, 1));
    }

    #[test]
    fn clock_reads_like_a_clock() {
        assert_eq!(clock(0.0), "0s");
        assert_eq!(clock(-3.0), "0s");
        assert_eq!(clock(59.4), "59s");
        assert_eq!(clock(185.0), "3m05s");
        assert_eq!(clock(3599.0), "59m59s");
        assert_eq!(clock(7620.0), "2h07m");
    }

    #[test]
    fn the_line_names_run_calls_speed_and_time_left() {
        let mut t = Tally::new(3);
        t.start_run();
        t.record(3.0);
        t.record(5.0);
        assert_eq!(
            t.line(65.0),
            "LLM run 1/3, call 2 | 4.0 s/call | 1m05s elapsed | time left known after the first run"
        );
        t.start_run();
        assert_eq!(
            t.line(70.0),
            "LLM run 2/3, call 2 | 4.0 s/call | 1m10s elapsed | about 16s left"
        );
    }

    struct Echo;
    impl Provider for Echo {
        fn id(&self) -> String {
            "test/echo".into()
        }
        fn complete(&mut self, p: &Prompt) -> Result<Reply, LlmError> {
            Ok(Reply {
                text: p.user.clone(),
                input_tokens: 0,
                output_tokens: 0,
                cost_usd: 0.0,
                cached: false,
            })
        }
    }

    #[test]
    fn counted_providers_share_one_tally() {
        let shared: Shared = Arc::new(Mutex::new((Tally::new(2), Instant::now())));
        let prompt = Prompt {
            system: "s".into(),
            user: "7".into(),
            max_tokens: 1,
        };
        let mut a = Counted::start(Echo, shared.clone());
        assert_eq!(a.id(), "test/echo");
        assert_eq!(a.complete(&prompt).unwrap().text, "7");
        a.complete(&prompt).unwrap();
        let mut b = Counted::start(Echo, shared.clone());
        b.complete(&prompt).unwrap();
        let t = shared.lock().unwrap().0.clone();
        assert_eq!((t.started, t.finished_calls, t.run_calls), (2, 2, 1));
        assert!(t.call_secs >= 0.0);
    }
}
