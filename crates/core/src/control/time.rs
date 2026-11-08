//! Aging (01 §4.8) and mortality urgency (01 §4.9); spec §15.

/// `age = a_t·ticks + a_e·experiences` (01 §4.8).
pub fn age(a_t: f32, ticks: u64, a_e: f32, experiences: u64) -> f32 {
    a_t * ticks as f32 + a_e * experiences as f32
}

/// `plasticity = base_lr·e^(−aging_rate_eff·age)`, with
/// `aging_rate_eff = aging_rate·(1 − 0.5·C̄_long)`: high long-run C slows the
/// decline. Aging acts on learning rates only (01 §4.8).
pub fn plasticity(base_lr: f32, aging_rate: f32, c_long: f32, age: f32) -> f32 {
    let rate = aging_rate * (1.0 - 0.5 * c_long.clamp(0.0, 1.0));
    base_lr * (-rate * age).exp()
}

/// `U = clamp(1 − T_rem_est/H, 0, 1)²` (spec §15; form proposed in 01 §4.9).
pub fn urgency(t_remaining_est: f32, horizon: f32) -> f32 {
    if horizon <= 0.0 {
        return 1.0;
    }
    (1.0 - t_remaining_est / horizon).clamp(0.0, 1.0).powi(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plasticity_declines_slower_with_high_c() {
        let a = age(1.0, 1_000_000, 0.0, 0);
        let curious = plasticity(0.05, 1e-6, 0.9, a);
        let dull = plasticity(0.05, 1e-6, 0.1, a);
        assert!(curious > dull && dull < 0.05);
    }

    #[test]
    fn urgency_rises_as_time_runs_out() {
        assert_eq!(urgency(100.0, 100.0), 0.0);
        assert!(urgency(50.0, 100.0) < urgency(10.0, 100.0));
        assert_eq!(urgency(0.0, 100.0), 1.0);
    }
}
