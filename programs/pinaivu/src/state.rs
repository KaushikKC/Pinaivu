use anchor_lang::prelude::*;

// PDA seeds — kept in one place so adapters and tests can import them.
pub const SEED_STATE:  &[u8] = b"state";
pub const SEED_ESCROW: &[u8] = b"escrow";
pub const SEED_NODE:   &[u8] = b"node";
pub const SEED_SCORE:  &[u8] = b"score";

// ---------------------------------------------------------------------------
// ProgramState — global stats and config, one per deployment
// ---------------------------------------------------------------------------

#[account]
pub struct ProgramState {
    pub admin: Pubkey,
    pub total_nodes_registered: u64,
    pub total_jobs_completed: u64,
    pub total_volume_lamports: u64,
    /// Default escrow lifetime in seconds; clients can override per-job.
    pub escrow_timeout_secs: i64,
    pub bump: u8,
}

impl ProgramState {
    // 8 discriminator + 32 admin + 8*4 counters/timeout + 1 bump
    pub const LEN: usize = 8 + 32 + 8 + 8 + 8 + 8 + 1;
}

// ---------------------------------------------------------------------------
// EscrowAccount — one per inference job
// ---------------------------------------------------------------------------

#[derive(AnchorSerialize, AnchorDeserialize, Clone, PartialEq, Eq)]
pub enum EscrowState {
    Locked,
    Released,
    Refunded,
}

#[account]
pub struct EscrowAccount {
    pub client: Pubkey,
    pub node: Pubkey,
    pub amount_lamports: u64,
    /// UUID bytes of the Pinaivu AI RequestId.
    pub request_id: [u8; 16],
    pub state: EscrowState,
    pub created_at: i64,
    pub expires_at: i64,
    /// SHA-256 of ProofOfInference.canonical_bytes() — set on release.
    /// Allows off-chain auditors to verify the job without a chain call.
    pub proof_hash: [u8; 32],
    pub bump: u8,
}

impl EscrowAccount {
    // 8 + 32 + 32 + 8 + 16 + 1(enum) + 8 + 8 + 32 + 1
    pub const LEN: usize = 8 + 32 + 32 + 8 + 16 + 1 + 8 + 8 + 32 + 1;
}

// ---------------------------------------------------------------------------
// NodeRegistration — one per GPU node
// ---------------------------------------------------------------------------

#[account]
pub struct NodeRegistration {
    /// Solana wallet that controls this registration.
    pub authority: Pubkey,
    /// Ed25519 P2P keypair from the Pinaivu node binary (not the wallet key).
    pub node_pubkey: [u8; 32],
    /// SHA-256 of each model name, max 8 models.
    pub model_hashes: Vec<[u8; 32]>,
    pub gpu_vram_mb: u32,
    /// Price per 1 000 output tokens in lamports.
    pub price_per_1k_lamports: u64,
    pub registered_at: i64,
    pub active: bool,
    pub bump: u8,
}

impl NodeRegistration {
    pub const MAX_MODELS: usize = 8;
    // 8 + 32 + 32 + (4 + 8*32) + 4 + 8 + 8 + 1 + 1
    pub const LEN: usize = 8 + 32 + 32 + (4 + Self::MAX_MODELS * 32) + 4 + 8 + 8 + 1 + 1;
}

// ---------------------------------------------------------------------------
// NodeScore — on-chain reputation / leaderboard entry, one per node
// ---------------------------------------------------------------------------

#[account]
pub struct NodeScore {
    /// Matches NodeRegistration.node_pubkey — Ed25519 P2P key.
    pub node_pubkey: [u8; 32],
    /// Solana wallet that may update this account.
    pub authority: Pubkey,
    /// Latest gossip Merkle root of the node's ProofOfInference history.
    /// Any third party can use this to verify individual proofs off-chain
    /// using only the node's Ed25519 public key.
    pub merkle_root: [u8; 32],
    /// SHA-256 of the human-readable label string (e.g. "v1", epoch number).
    pub merkle_root_label: [u8; 32],
    pub total_jobs: u64,
    pub total_tokens_earned: u64,
    pub total_lamports_earned: u64,
    /// Success rate in basis points (0–10_000 = 0%–100%).
    /// Starts at 10_000; explicit failure tracking can decrease it.
    pub success_rate_bps: u16,
    /// Exponential moving average latency in milliseconds (α = 0.1).
    pub avg_latency_ms: u32,
    /// Composite score 0–1_000_000_000.
    /// Weights: success_rate 40%, job_volume 40%, latency 20%.
    pub score: u64,
    pub last_updated: i64,
    pub bump: u8,
}

impl NodeScore {
    // 8 + 32 + 32 + 32 + 32 + 8 + 8 + 8 + 2 + 4 + 8 + 8 + 1
    pub const LEN: usize = 8 + 32 + 32 + 32 + 32 + 8 + 8 + 8 + 2 + 4 + 8 + 8 + 1;

    /// Recompute the composite score from stored components.
    ///
    /// All arithmetic is saturating to prevent overflow in adversarial inputs.
    /// The result is in [0, 1_000_000_000] where 1_000_000_000 = perfect.
    pub fn recompute_score(&mut self) {
        // success_rate_bps: 0–10_000 → weight 40%
        let success = (self.success_rate_bps as u64)
            .saturating_mul(400_000_000)
            / 10_000;

        // job volume: saturates at 10_000 jobs → weight 40%
        let jobs_capped = self.total_jobs.min(10_000);
        let jobs = jobs_capped.saturating_mul(400_000_000) / 10_000;

        // latency: lower is better, saturates at 5 000 ms → weight 20%
        let latency_capped = (self.avg_latency_ms as u64).min(5_000);
        let latency = (5_000u64.saturating_sub(latency_capped))
            .saturating_mul(200_000_000)
            / 5_000;

        self.score = success + jobs + latency;
    }
}
