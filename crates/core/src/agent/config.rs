//! One agent's full configuration: personality (01 §4.10; spec §19) plus the
//! proposed values from spec §5–§17. `Default` is spec §19's "default" column.
//! Archetypes are TOML files that override fields (build step 7).

use crate::activation::{AffectParams, CuriosityParams, GShape, PowerActivation, SignalWeights};
use crate::control::kick::KickParams;
use crate::control::oracle::{Exp3, Oracle};
use crate::control::permeability::PermeabilityParams;
use crate::control::subconscious::SubconsciousParams;
use crate::memory::MemoryParams;
use serde::{Deserialize, Serialize};

/// A change to one lens prior (spec §19 "Lens priors (vs §7)").
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LensPrior {
    /// Mode index L, E, P, O = 0..4.
    pub mode: usize,
    /// Feature index (spec §3).
    pub feature: usize,
    pub weight: f32,
}

/// Lifespan for urgency (spec §15): hidden H ~ Normal(H, 0.1H), estimate
/// noise σ = 0.15H. `None` means no horizon, so U = 0.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Horizon {
    pub ticks: f32,
}

/// k for the log form of g(C) (spec §16 sweep; proposed).
pub const G_LOG_K: f32 = 10.0;

/// Subconscious budget policy (OQ16).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetPolicy {
    /// `B = B_base·(1 + (1 − ρ_eff))·(1 + 2·[resting])` (spec §14).
    #[default]
    RestBoost,
    /// The same without the rest boost.
    Fixed,
}

impl BudgetPolicy {
    /// Whether resting boosts the budget under this policy.
    pub fn boost(self, resting: bool) -> bool {
        resting && self == BudgetPolicy::RestBoost
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BrainConfig {
    pub curiosity: CuriosityParams,
    pub k: SignalWeights,
    pub affect: AffectParams,
    pub permeability: PermeabilityParams,
    pub kick: KickParams,
    /// Kick on/off (ablation C−X, and the stuck-kick gate's control).
    pub kick_enabled: bool,
    /// Tenacity priors (a, b) per mode L, E, P, O and the fade γ.
    pub tenacity_ab: [(f32, f32); 4],
    pub tenacity_gamma: f32,
    /// Oracle bias column Θ₀ per mode (00 §5.1.1 archetype routing).
    pub oracle_bias: [f32; 4],
    /// Oracle step size η and floor (spec §8).
    pub oracle_eta: f32,
    pub oracle_floor: f32,
    pub lens_priors: Vec<LensPrior>,
    /// Learning-rate multipliers per mode.
    pub lens_lr: [f32; 4],
    /// Freeze every lens (frozen-lens ablation, spec §24).
    pub lens_frozen: [bool; 4],
    /// Thought momentum k and τ (01 §2.2).
    pub tm_k: usize,
    pub tm_tau: f32,
    pub beta_u: f32,
    pub beta_c: f32,
    /// External-drive floor ε_ext (spec §16, #41).
    pub eps_ext: f32,
    /// Danger counts as an external force: Flee candidates get the same
    /// effort floor as commanded steps, ε_ext·(1 − D) (proposed, pending
    /// Michael's call; without it a threatened agent cannot start to flee
    /// because threat lowers C).
    pub threat_drive: bool,
    /// Idle `imagine` re-proposes the player's last command as the agent's
    /// own goal while the item is not held, so its goals support the
    /// player's (spec §19 helper; proposed).
    pub goals_from_commands: bool,
    pub aging_rate: f32,
    /// age = a_t·ticks + a_e·experiences (spec §15; proposed 1 and 0).
    pub age_per_tick: f32,
    pub age_per_experience: f32,
    /// g(C) = C^α (spec §16).
    pub alpha: f32,
    /// Form of g(C) and the log form's k (OQ8 sweep; spec §16).
    pub g_shape: GShape,
    pub g_k: f32,
    /// Subconscious budget policy (OQ16; spec §14, §17).
    pub budget_policy: BudgetPolicy,
    /// Oracle learner: `None` is the contextual gradient bandit (spec §8);
    /// `Some` uses EXP3 (OQ18 alternative).
    pub oracle_exp3: Option<Exp3>,
    /// Multiplier on the dominant mode's temperature (OQ19 sweep handle).
    pub tau_scale: f32,
    /// k_g in R (spec §13).
    pub k_goal: f32,
    /// ρ_pre: 0.5 with an LLM tier attached, else 0 (spec §14).
    pub rho_pre: f32,
    pub memory: MemoryParams,
    pub subconscious: SubconsciousParams,
    pub horizon: Option<Horizon>,
    /// Success thresholds (thr0, thr1) for L, E, P (spec §14).
    pub thr_l: (f32, f32),
    pub thr_e: (f32, f32),
    pub thr_p: (f32, f32),
}

impl Default for BrainConfig {
    fn default() -> Self {
        Self {
            curiosity: CuriosityParams::default(),
            k: SignalWeights::default(),
            affect: AffectParams::default(),
            permeability: PermeabilityParams::default(),
            kick: KickParams::default(),
            kick_enabled: true,
            tenacity_ab: [(1.0, 1.0); 4],
            tenacity_gamma: 0.98,
            oracle_bias: [0.0; 4],
            oracle_eta: Oracle::default().eta,
            oracle_floor: Oracle::default().floor,
            lens_priors: Vec::new(),
            lens_lr: [1.0; 4],
            lens_frozen: [false; 4],
            tm_k: 7,
            tm_tau: 50.0,
            beta_u: 0.2,
            beta_c: 0.1,
            eps_ext: 0.20,
            threat_drive: true,
            goals_from_commands: false,
            aging_rate: 1e-6,
            age_per_tick: 1.0,
            age_per_experience: 0.0,
            alpha: PowerActivation::default().alpha,
            g_shape: GShape::default(),
            g_k: G_LOG_K,
            budget_policy: BudgetPolicy::default(),
            oracle_exp3: None,
            tau_scale: 1.0,
            k_goal: 0.5,
            rho_pre: 0.0,
            memory: MemoryParams::default(),
            subconscious: SubconsciousParams::default(),
            horizon: None,
            thr_l: (0.1, 0.4),
            thr_e: (0.1, 0.5),
            thr_p: (0.3, 0.9),
        }
    }
}
