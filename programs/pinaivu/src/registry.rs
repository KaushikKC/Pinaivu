use anchor_lang::prelude::*;

use crate::{
    error::PeerAiError,
    state::{NodeRegistration, ProgramState, SEED_NODE, SEED_STATE},
};

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

#[event]
pub struct NodeRegistered {
    pub authority: Pubkey,
    pub node_pubkey: [u8; 32],
    pub gpu_vram_mb: u32,
    pub price_per_1k_lamports: u64,
}

#[event]
pub struct NodeUpdated {
    pub node_pubkey: [u8; 32],
    pub gpu_vram_mb: u32,
    pub price_per_1k_lamports: u64,
    pub active: bool,
}

// ---------------------------------------------------------------------------
// Instruction handlers
// ---------------------------------------------------------------------------

/// Register a GPU node. Called once per node on first deployment.
///
/// node_pubkey is the Ed25519 P2P key from the Pinaivu node binary — it is NOT
/// the Solana wallet key. It is stored so clients can match on-chain registry
/// entries to peers discovered via libp2p.
pub fn register_node(
    ctx: Context<RegisterNode>,
    node_pubkey: [u8; 32],
    model_hashes: Vec<[u8; 32]>,
    gpu_vram_mb: u32,
    price_per_1k_lamports: u64,
) -> Result<()> {
    require!(model_hashes.len() <= NodeRegistration::MAX_MODELS, PeerAiError::TooManyModels);

    let clock = Clock::get()?;
    let reg = &mut ctx.accounts.registration;
    reg.authority = ctx.accounts.authority.key();
    reg.node_pubkey = node_pubkey;
    reg.model_hashes = model_hashes;
    reg.gpu_vram_mb = gpu_vram_mb;
    reg.price_per_1k_lamports = price_per_1k_lamports;
    reg.registered_at = clock.unix_timestamp;
    reg.active = true;
    reg.bump = ctx.bumps.registration;

    ctx.accounts.program_state.total_nodes_registered = ctx
        .accounts
        .program_state
        .total_nodes_registered
        .saturating_add(1);

    emit!(NodeRegistered {
        authority: ctx.accounts.authority.key(),
        node_pubkey,
        gpu_vram_mb,
        price_per_1k_lamports,
    });

    Ok(())
}

/// Update a registered node's capabilities or toggle it active/inactive.
pub fn update_node(
    ctx: Context<UpdateNode>,
    model_hashes: Vec<[u8; 32]>,
    gpu_vram_mb: u32,
    price_per_1k_lamports: u64,
    active: bool,
) -> Result<()> {
    require!(model_hashes.len() <= NodeRegistration::MAX_MODELS, PeerAiError::TooManyModels);

    let reg = &mut ctx.accounts.registration;
    reg.model_hashes = model_hashes;
    reg.gpu_vram_mb = gpu_vram_mb;
    reg.price_per_1k_lamports = price_per_1k_lamports;
    reg.active = active;

    emit!(NodeUpdated {
        node_pubkey: reg.node_pubkey,
        gpu_vram_mb,
        price_per_1k_lamports,
        active,
    });

    Ok(())
}

// ---------------------------------------------------------------------------
// Accounts
// ---------------------------------------------------------------------------

#[derive(Accounts)]
#[instruction(node_pubkey: [u8; 32])]
pub struct RegisterNode<'info> {
    #[account(
        init,
        payer  = authority,
        space  = NodeRegistration::LEN,
        seeds  = [SEED_NODE, &node_pubkey],
        bump,
    )]
    pub registration: Account<'info, NodeRegistration>,

    #[account(
        mut,
        seeds = [SEED_STATE],
        bump  = program_state.bump,
    )]
    pub program_state: Account<'info, ProgramState>,

    #[account(mut)]
    pub authority: Signer<'info>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct UpdateNode<'info> {
    #[account(
        mut,
        seeds  = [SEED_NODE, &registration.node_pubkey],
        bump   = registration.bump,
        constraint = registration.authority == authority.key() @ PeerAiError::Unauthorized,
    )]
    pub registration: Account<'info, NodeRegistration>,

    pub authority: Signer<'info>,
}
