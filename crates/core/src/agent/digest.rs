//! Portable fingerprints of a run, for golden-trace tests (spec §27, "golden
//! trace stable"). The discrete fields are hashed exactly. Floats are summed
//! and compared to a tolerance, because `exp`, `ln` and `cos` can differ in
//! the last bit between machines.

use super::log::TickLog;

/// FNV-1a over the discrete fields of each row: tick, choice, goal, plan,
/// memory counts, credited mode and so on (spec §27).
pub fn trace_digest(rows: &[TickLog]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for r in rows {
        let line = format!(
            "{}|{}|{}|{:?}|{:?}|{}|{}|{}|{}|{}|{}|{:?}|{}|{}|{}|{}|{}|{}|{}",
            r.tick,
            r.dt,
            r.chosen,
            r.goal,
            r.plan_step,
            r.n_choices,
            r.kicks,
            r.stuck_kick,
            r.mem_items,
            r.formed,
            r.pruned,
            r.tm,
            r.prev_action,
            r.credited,
            r.external_drive,
            r.retrieved,
            r.stage,
            r.creative,
            r.new_items,
        );
        for b in line.bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    h
}

/// Σ C, Σ D, Σ V, Σ R and Σ w_P over a trace (spec §27).
pub fn float_sums(rows: &[TickLog]) -> [f64; 5] {
    let mut s = [0.0; 5];
    for r in rows {
        for (acc, v) in s.iter_mut().zip([r.c, r.d, r.v, r.r, r.w[2]]) {
            *acc += f64::from(v);
        }
    }
    s
}

/// True when every sum is within `rel` of the pinned value, relative to
/// max(|pinned|, 1) (spec §27).
pub fn sums_close(got: [f64; 5], want: [f64; 5], rel: f64) -> bool {
    got.iter()
        .zip(want)
        .all(|(g, w)| (g - w).abs() <= rel * w.abs().max(1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(tick: u64, c: f32) -> TickLog {
        TickLog {
            tick,
            c,
            chosen: "explore".into(),
            ..TickLog::default()
        }
    }

    #[test]
    fn digest_ignores_floats_but_not_choices() {
        let a = [row(1, 0.1), row(2, 0.2)];
        let b = [row(1, 0.9), row(2, 0.0)];
        assert_eq!(trace_digest(&a), trace_digest(&b));
        let mut c = a.clone();
        c[1].chosen = "idle".into();
        assert_ne!(trace_digest(&a), trace_digest(&c));
        assert_ne!(trace_digest(&a), trace_digest(&a[..1]));
        assert_eq!(trace_digest(&[]), 0xcbf2_9ce4_8422_2325);
    }

    #[test]
    fn digest_exact_value() {
        // FNV-1a of "0|0||None|None|0|0|false|0|0|0|[]|||false|0|0|false|0"
        // with the reference offset and prime.
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in "0|0||None|None|0|0|false|0|0|0|[]|||false|0|0|false|0".bytes() {
            h = (h ^ u64::from(b)).wrapping_mul(1_099_511_628_211);
        }
        assert_eq!(trace_digest(&[TickLog::default()]), h);
    }

    #[test]
    fn sums_add_each_field() {
        let mut r = row(0, 0.5);
        (r.d, r.v, r.r, r.w) = (0.25, -0.5, 1.0, [0.0, 0.0, 0.75, 0.0]);
        assert_eq!(float_sums(&[r.clone(), r]), [1.0, 0.5, -1.0, 2.0, 1.5]);
    }

    #[test]
    fn close_is_relative_with_a_floor_of_one() {
        let want = [100.0, 0.0, 1.0, -50.0, 10.0];
        assert!(sums_close(want, want, 0.0));
        assert!(sums_close([100.09, 0.0009, 1.0, -50.04, 10.0], want, 1e-3));
        assert!(!sums_close([100.2, 0.0, 1.0, -50.0, 10.0], want, 1e-3));
        assert!(!sums_close([100.0, 0.002, 1.0, -50.0, 10.0], want, 1e-3));
    }
}
