use anchor_lang::prelude::*;

pub mod error;
pub mod escrow;
pub mod registry;
pub mod score;
pub mod state;

use escrow::*;
use registry::*;
use score::*;
use state::{ProgramState, SEED_STATE};

// Replace with `anchor keys list` output after `anchor build`.
declare_id!("PiNaivuXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX");

#[program]
pub mod pinaivu {
    use super::*;

    // ── One-time setup ────────────────────────────────────────────────────────

    /// Deploy the global program state. Called once by the deployer.
    pub fn initialize(ctx: Context<Initialize>, escrow_timeout_secs: i64) -> Result<()> {
        let state = &mut ctx.accounts.program_state;
        state.admin = ctx.accounts.admin.key();
        state.total_nodes_registered = 0;
        state.total_jobs_completed = 0;
        state.total_volume_lamports = 0;
        state.escrow_timeout_secs = escrow_timeout_secs;
        state.bump = ctx.bumps.program_state;
        Ok(())
    }

    // ── Escrow ────────────────────────────────────────────────────────────────

    pub fn lock_escrow(
        ctx: Context<LockEscrow>,
        request_id: [u8; 16],
        amount_lamports: u64,
        timeout_secs: i64,
    ) -> Result<()> {
        escrow::lock_escrow(ctx, request_id, amount_lamports, timeout_secs)
    }

    pub fn release_escrow(ctx: Context<ReleaseEscrow>, proof_hash: [u8; 32]) -> Result<()> {
        escrow::release_escrow(ctx, proof_hash)
    }

    pub fn refund_escrow(ctx: Context<RefundEscrow>) -> Result<()> {
        escrow::refund_escrow(ctx)
    }

    // ── Registry ──────────────────────────────────────────────────────────────

    pub fn register_node(
        ctx: Context<RegisterNode>,
        node_pubkey: [u8; 32],
        model_hashes: Vec<[u8; 32]>,
        gpu_vram_mb: u32,
        price_per_1k_lamports: u64,
    ) -> Result<()> {
        registry::register_node(ctx, node_pubkey, model_hashes, gpu_vram_mb, price_per_1k_lamports)
    }

    pub fn update_node(
        ctx: Context<UpdateNode>,
        model_hashes: Vec<[u8; 32]>,
        gpu_vram_mb: u32,
        price_per_1k_lamports: u64,
        active: bool,
    ) -> Result<()> {
        registry::update_node(ctx, model_hashes, gpu_vram_mb, price_per_1k_lamports, active)
    }

    // ── Score / Reputation ────────────────────────────────────────────────────

    pub fn initialize_score(ctx: Context<InitializeScore>, node_pubkey: [u8; 32]) -> Result<()> {
        score::initialize_score(ctx, node_pubkey)
    }

    pub fn submit_proof(
        ctx: Context<SubmitProof>,
        proof_hash: [u8; 32],
        output_tokens: u32,
        latency_ms: u32,
        lamports_earned: u64,
    ) -> Result<()> {
        score::submit_proof(ctx, proof_hash, output_tokens, latency_ms, lamports_earned)
    }

    pub fn anchor_merkle_root(
        ctx: Context<AnchorMerkleRoot>,
        merkle_root: [u8; 32],
        label: [u8; 32],
    ) -> Result<()> {
        score::anchor_merkle_root(ctx, merkle_root, label)
    }
}

// ---------------------------------------------------------------------------
// Initialize accounts
// ---------------------------------------------------------------------------

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(
        init,
        payer  = admin,
        space  = ProgramState::LEN,
        seeds  = [SEED_STATE],
        bump,
    )]
    pub program_state: Account<'info, ProgramState>,

    #[account(mut)]
    pub admin: Signer<'info>,

    pub system_program: Program<'info, System>,
}
