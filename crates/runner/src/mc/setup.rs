//! Picking the arm and the LLM for a Minecraft run (spec §23, §24, §26).

use std::path::PathBuf;

use baselines::arm_b::{ArmB, DEFAULT_BETA};
use baselines::{ArmA, ArmBConfig, Policy};
use llm::anthropic::{Anthropic, AnthropicConfig};
use llm::openai::{OpenAi, OpenAiConfig};
use llm::{CacheMode, Cached, Provider, Tier};

/// The arms that run in Minecraft so far (arm C there is a later step).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McArm {
    A,
    B,
    /// B with the count bonus at β.
    BN,
}

impl McArm {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "a" => Some(Self::A),
            "b" => Some(Self::B),
            "bn" => Some(Self::BN),
            _ => None,
        }
    }

    pub fn needs_llm(self) -> bool {
        self != Self::A
    }
}

/// `local`, `cheap` or `strong`.
pub fn tier(s: &str) -> Option<Tier> {
    match s {
        "local" => Some(Tier::Local),
        "cheap" => Some(Tier::Cheap),
        "strong" => Some(Tier::Strong),
        _ => None,
    }
}

/// The provider for a tier, behind the replay cache when `cache` is given.
/// `ollama` overrides the local server's URL; `key` is the Anthropic key
/// (from the local `.env`, never committed).
pub fn provider(
    tier: Tier,
    ollama: Option<String>,
    key: Option<String>,
    cache: Option<(PathBuf, bool)>,
) -> Result<Box<dyn Provider>, String> {
    let p: Box<dyn Provider> = match tier {
        Tier::Local => {
            let mut cfg = OpenAiConfig::local();
            if let Some(url) = ollama {
                cfg.base_url = url;
            }
            Box::new(OpenAi::new(cfg))
        }
        Tier::Cheap | Tier::Strong => {
            let key = key.ok_or("ANTHROPIC_API_KEY is not set")?;
            let cfg = if tier == Tier::Cheap {
                AnthropicConfig::cheap()
            } else {
                AnthropicConfig::strong()
            };
            Box::new(Anthropic::new(cfg, key))
        }
    };
    let Some((file, replay)) = cache else {
        return Ok(p);
    };
    let mode = if replay {
        CacheMode::Replay
    } else {
        CacheMode::Record
    };
    Cached::open(p, mode, file)
        .map(|c| Box::new(c) as Box<dyn Provider>)
        .map_err(|e| format!("cannot open the cache: {e}"))
}

/// The policy for an arm. B and B+N use `llm` at `tier`; B+N uses
/// [`DEFAULT_BETA`].
pub fn arm(
    which: McArm,
    tier: Tier,
    llm: Option<Box<dyn Provider>>,
) -> Result<Box<dyn Policy>, String> {
    let novelty = (which == McArm::BN).then_some(DEFAULT_BETA);
    match (which, llm) {
        (McArm::A, _) => Ok(Box::new(ArmA::default())),
        (_, None) => Err("arms B and B+N need an LLM".into()),
        (_, Some(p)) => {
            let cfg = ArmBConfig {
                novelty,
                tier,
                ..Default::default()
            };
            Ok(Box::new(ArmB::new(cfg, p)))
        }
    }
}
