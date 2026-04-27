use anchor_lang::prelude::*;

use crate::{
    error::PeerAiError,
    state::{NodeScore, SEED_SCORE},
};

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

#[event]
pub struct ScoreInitialized {
    pub node_pubkey: [u8; 32],
    pub authority: Pubkey,
}

#[event]
pub struct ProofSubmitted {
    pub node_pubkey: [u8; 32],
    /// SHA-256(ProofOfInference.canonical_bytes()) — verified off-chain with
    /// only the node's Ed25519 public key; no chain call needed.
    pub proof_hash: [u8; 32],
    pub output_tokens: u32,
    pub latency_ms: u32,
    pub lamports_earned: u64,
    pub new_score: u64,
    pub total_jobs: u64,
}

#[event]
pub struct MerkleRootAnchored {
    pub node_pubkey: [u8; 32],
    pub merkle_root: [u8; 32],
    pub label: [u8; 32],
    pub timestamp: i64,
}

// ---------------------------------------------------------------------------
// Instruction handlers
// ---------------------------------------------------------------------------

/// Create the score account for a node. Called once, typically alongside
/// register_node, so both PDAs exist before the node accepts jobs.
pub fn initialize_score(ctx: Context<InitializeScore>, node_pubkey: [u8; 32]) -> Result<()> {
    let score = &mut ctx.accounts.score;
    score.node_pubkey = node_pubkey;
    score.authority = ctx.accounts.authority.key();
    score.merkle_root = [0u8; 32];
    score.merkle_root_label = [0u8; 32];
    score.total_jobs = 0;
    score.total_tokens_earned = 0;
    score.total_lamports_earned = 0;
    score.success_rate_bps = 10_000; // 100% until explicit failures are tracked
    score.avg_latency_ms = 0;
    score.score = 0;
    score.last_updated = Clock::get()?.unix_timestamp;
    score.bump = ctx.bumps.score;

    emit!(ScoreInitialized {
        node_pubkey,
        authority: ctx.accounts.authority.key(),
    });

    Ok(())
}

/// Record a completed inference job and recompute the on-chain score.
///
/// Called by the node after release_escrow succeeds. proof_hash ties this
/// on-chain record to the off-chain ProofOfInference that the client received,
/// enabling anyone to verify the full chain of trust without a chain call.
pub fn submit_proof(
    ctx: Context<SubmitProof>,
    proof_hash: [u8; 32],
    output_tokens: u32,
    latency_ms: u32,
    lamports_earned: u64,
) -> Result<()> {
    require!(proof_hash != [0u8; 32], PeerAiError::InvalidProofHash);

    let score = &mut ctx.accounts.score;
    score.total_jobs = score.total_jobs.saturating_add(1);
    score.total_tokens_earned = score.total_tokens_earned.saturating_add(output_tokens as u64);
    score.total_lamports_earned = score.total_lamports_earned.saturating_add(lamports_earned);

    // Exponential moving average: avg_new = 0.9 * avg_old + 0.1 * sample.
    // Integer arithmetic: (9 * old + 1 * new) / 10.
    score.avg_latency_ms = ((score.avg_latency_ms as u64 * 9 + latency_ms as u64) / 10) as u32;

    score.recompute_score();
    score.last_updated = Clock::get()?.unix_timestamp;

    emit!(ProofSubmitted {
        node_pubkey: score.node_pubkey,
        proof_hash,
        output_tokens,
        latency_ms,
        lamports_earned,
        new_score: score.score,
        total_jobs: score.total_jobs,
    });

    Ok(())
}

/// Anchor the node's gossip Merkle root on-chain.
///
/// The Merkle root is computed off-chain by the node daemon over its full
/// ProofOfInference history. Any third party can verify any individual proof
/// using the root + a Merkle path from the P2P layer, with no chain call.
/// Anchoring it here makes it publicly observable and tamper-evident.
pub fn anchor_merkle_root(
    ctx: Context<AnchorMerkleRoot>,
    merkle_root: [u8; 32],
    label: [u8; 32],
) -> Result<()> {
    let clock = Clock::get()?;
    let score = &mut ctx.accounts.score;
    score.merkle_root = merkle_root;
    score.merkle_root_label = label;
    score.last_updated = clock.unix_timestamp;

    emit!(MerkleRootAnchored {
        node_pubkey: score.node_pubkey,
        merkle_root,
        label,
        timestamp: clock.unix_timestamp,
    });

    Ok(())
}

// ---------------------------------------------------------------------------
// Accounts
// ---------------------------------------------------------------------------

#[derive(Accounts)]
#[instruction(node_pubkey: [u8; 32])]
pub struct InitializeScore<'info> {
    #[account(
        init,
        payer  = authority,
        space  = NodeScore::LEN,
        seeds  = [SEED_SCORE, &node_pubkey],
        bump,
    )]
    pub score: Account<'info, NodeScore>,

    #[account(mut)]
    pub authority: Signer<'info>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct SubmitProof<'info> {
    #[account(
        mut,
        seeds  = [SEED_SCORE, &score.node_pubkey],
        bump   = score.bump,
        constraint = score.authority == authority.key() @ PeerAiError::Unauthorized,
    )]
    pub score: Account<'info, NodeScore>,

    pub authority: Signer<'info>,
}

#[derive(Accounts)]
pub struct AnchorMerkleRoot<'info> {
    #[account(
        mut,
        seeds  = [SEED_SCORE, &score.node_pubkey],
        bump   = score.bump,
        constraint = score.authority == authority.key() @ PeerAiError::Unauthorized,
    )]
    pub score: Account<'info, NodeScore>,

    pub authority: Signer<'info>,
}
