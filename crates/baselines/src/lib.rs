//! Baseline arms (spec §24): A is a behaviour tree, B is our own minimal LLM
//! loop (#30), B+N adds a count-based novelty bonus. All share skills,
//! perception and candidates with arm C; only the decision layer differs.

pub use host_api;
