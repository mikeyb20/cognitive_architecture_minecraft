//! Run independent seeded jobs on threads, results in job order (spec §26:
//! a table replays exactly whatever the thread count).

/// `f` over every job, `threads` at a time; output order is job order.
pub fn par_map<J: Sync, T: Clone + Default + Send>(
    jobs: &[J],
    threads: usize,
    f: impl Fn(&J) -> T + Sync,
) -> Vec<T> {
    let mut out = vec![T::default(); jobs.len()];
    let chunk = jobs.len().div_ceil(threads.max(1)).max(1);
    std::thread::scope(|scope| {
        for (js, os) in jobs.chunks(chunk).zip(out.chunks_mut(chunk)) {
            let f = &f;
            scope.spawn(move || {
                for (j, o) in js.iter().zip(os) {
                    *o = f(j);
                }
            });
        }
    });
    out
}

#[cfg(test)]
mod tests {
    use super::par_map;

    #[test]
    fn keeps_job_order_for_any_thread_count() {
        let jobs: Vec<u32> = (0..10).collect();
        for t in [0, 1, 3, 16] {
            assert_eq!(
                par_map(&jobs, t, |j| j * 2),
                (0..10).map(|j| j * 2).collect::<Vec<_>>()
            );
        }
        assert!(par_map(&[] as &[u32], 4, |j| *j).is_empty());
    }
}
