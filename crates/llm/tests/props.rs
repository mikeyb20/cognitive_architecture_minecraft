// Invariants of the budget guard and prices (spec §23).

use llm::{BudgetGuard, Price};
use proptest::prelude::*;

proptest! {
    /// However calls arrive, no hour ever holds more than the cap, and a
    /// refusal only happens when the hour before is full.
    #[test]
    fn never_more_than_cap_in_any_hour(cap in 1u32..20, gaps in prop::collection::vec(0.0f64..900.0, 1..200)) {
        let mut g = BudgetGuard::new(Some(cap));
        let (mut now, mut spent) = (0.0, Vec::new());
        for gap in gaps {
            now += gap;
            let in_hour = spent.iter().filter(|t| now - **t < 3600.0).count();
            let ok = g.try_spend(now);
            prop_assert_eq!(ok, in_hour < cap as usize);
            if ok {
                spent.push(now);
            }
        }
    }

    /// Cost is never negative and adds up across calls.
    #[test]
    fn cost_is_additive(i in 0.0f64..100.0, o in 0.0f64..100.0, a in 0u32..100_000, b in 0u32..100_000) {
        let p = Price::new(i, o);
        prop_assert!(p.cost(a, b) >= 0.0);
        let split = p.cost(a, 0) + p.cost(0, b);
        prop_assert!((p.cost(a, b) - split).abs() < 1e-9);
    }
}
