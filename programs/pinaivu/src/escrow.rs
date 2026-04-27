use anchor_lang::prelude::*;

use crate::{
    error::PeerAiError,
    state::{EscrowAccount, EscrowState, ProgramState, SEED_ESCROW, SEED_STATE},
};

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

#[event]
pub struct EscrowLocked {
    pub request_id: [u8; 16],
    pub client: Pubkey,
    pub node: Pubkey,
    pub amount_lamports: u64,
    pub expires_at: i64,
}

#[event]
pub struct EscrowReleased {
    pub request_id: [u8; 16],
    pub node: Pubkey,
    pub amount_lamports: u64,
    /// SHA-256 of ProofOfInference.canonical_bytes().
    pub proof_hash: [u8; 32],
}

#[event]
pub struct EscrowRefunded {
    pub request_id: [u8; 16],
    pub client: Pubkey,
    pub amount_lamports: u64,
}

// ---------------------------------------------------------------------------
// Instruction handlers
// ---------------------------------------------------------------------------

/// Lock SOL in escrow before sending an inference job.
///
/// The escrow PDA is keyed on request_id so the node can locate it after the
/// job completes. If timeout_secs is 0 the program default is used.
pub fn lock_escrow(
    ctx: Context<LockEscrow>,
    request_id: [u8; 16],
    amount_lamports: u64,
    timeout_secs: i64,
) -> Result<()> {
    let clock = Clock::get()?;
    let timeout = if timeout_secs > 0 {
        timeout_secs
    } else {
        ctx.accounts.program_state.escrow_timeout_secs
    };

    let escrow = &mut ctx.accounts.escrow;
    escrow.client = ctx.accounts.client.key();
    escrow.node = ctx.accounts.node_wallet.key();
    escrow.amount_lamports = amount_lamports;
    escrow.request_id = request_id;
    escrow.state = EscrowState::Locked;
    escrow.created_at = clock.unix_timestamp;
    escrow.expires_at = clock.unix_timestamp + timeout;
    escrow.proof_hash = [0u8; 32];
    escrow.bump = ctx.bumps.escrow;

    // Transfer the payment on top of the rent already deposited by `init`.
    anchor_lang::system_program::transfer(
        CpiContext::new(
            ctx.accounts.system_program.to_account_info(),
            anchor_lang::system_program::Transfer {
                from: ctx.accounts.client.to_account_info(),
                to:   ctx.accounts.escrow.to_account_info(),
            },
        ),
        amount_lamports,
    )?;

    emit!(EscrowLocked {
        request_id,
        client: ctx.accounts.client.key(),
        node: ctx.accounts.node_wallet.key(),
        amount_lamports,
        expires_at: escrow.expires_at,
    });

    Ok(())
}

/// Release escrowed SOL to the node after successful inference.
///
/// Must be called by the node (signer = node_wallet stored in the escrow).
/// proof_hash is SHA-256(ProofOfInference.canonical_bytes()); it is stored
/// on-chain so auditors can verify the job off-chain without a chain call.
pub fn release_escrow(ctx: Context<ReleaseEscrow>, proof_hash: [u8; 32]) -> Result<()> {
    let clock = Clock::get()?;
    let escrow = &mut ctx.accounts.escrow;

    require!(escrow.state == EscrowState::Locked, PeerAiError::EscrowNotLocked);
    require!(clock.unix_timestamp <= escrow.expires_at, PeerAiError::EscrowExpired);

    let amount = escrow.amount_lamports;
    escrow.state = EscrowState::Released;
    escrow.proof_hash = proof_hash;

    // Direct lamport transfer: escrow PDA → node wallet.
    // We only move the payment amount; rent stays in the escrow for the audit log.
    **escrow.to_account_info().try_borrow_mut_lamports()? -= amount;
    **ctx.accounts.node_wallet.to_account_info().try_borrow_mut_lamports()? += amount;

    let state = &mut ctx.accounts.program_state;
    state.total_jobs_completed = state.total_jobs_completed.saturating_add(1);
    state.total_volume_lamports = state.total_volume_lamports.saturating_add(amount);

    emit!(EscrowReleased {
        request_id: escrow.request_id,
        node: ctx.accounts.node_wallet.key(),
        amount_lamports: amount,
        proof_hash,
    });

    Ok(())
}

/// Refund escrowed SOL back to the client.
///
/// Only callable after expires_at has passed, protecting nodes from clients
/// who disappear mid-job.
pub fn refund_escrow(ctx: Context<RefundEscrow>) -> Result<()> {
    let clock = Clock::get()?;
    let escrow = &mut ctx.accounts.escrow;

    require!(escrow.state == EscrowState::Locked, PeerAiError::EscrowNotLocked);
    require!(clock.unix_timestamp > escrow.expires_at, PeerAiError::EscrowNotExpired);

    let amount = escrow.amount_lamports;
    escrow.state = EscrowState::Refunded;

    **escrow.to_account_info().try_borrow_mut_lamports()? -= amount;
    **ctx.accounts.client.to_account_info().try_borrow_mut_lamports()? += amount;

    emit!(EscrowRefunded {
        request_id: escrow.request_id,
        client: ctx.accounts.client.key(),
        amount_lamports: amount,
    });

    Ok(())
}

// ---------------------------------------------------------------------------
// Accounts
// ---------------------------------------------------------------------------

#[derive(Accounts)]
#[instruction(request_id: [u8; 16])]
pub struct LockEscrow<'info> {
    #[account(
        init,
        payer  = client,
        space  = EscrowAccount::LEN,
        seeds  = [SEED_ESCROW, &request_id],
        bump,
    )]
    pub escrow: Account<'info, EscrowAccount>,

    #[account(
        mut,
        seeds = [SEED_STATE],
        bump  = program_state.bump,
    )]
    pub program_state: Account<'info, ProgramState>,

    #[account(mut)]
    pub client: Signer<'info>,

    /// CHECK: pubkey is stored in the escrow for routing; the node must sign
    /// release_escrow using this same wallet to claim funds.
    pub node_wallet: UncheckedAccount<'info>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct ReleaseEscrow<'info> {
    #[account(
        mut,
        seeds  = [SEED_ESCROW, &escrow.request_id],
        bump   = escrow.bump,
        constraint = escrow.node == node_wallet.key() @ PeerAiError::Unauthorized,
    )]
    pub escrow: Account<'info, EscrowAccount>,

    #[account(
        mut,
        seeds = [SEED_STATE],
        bump  = program_state.bump,
    )]
    pub program_state: Account<'info, ProgramState>,

    #[account(mut)]
    pub node_wallet: Signer<'info>,
}

#[derive(Accounts)]
pub struct RefundEscrow<'info> {
    #[account(
        mut,
        seeds  = [SEED_ESCROW, &escrow.request_id],
        bump   = escrow.bump,
        constraint = escrow.client == client.key() @ PeerAiError::Unauthorized,
    )]
    pub escrow: Account<'info, EscrowAccount>,

    #[account(mut)]
    pub client: Signer<'info>,
}
