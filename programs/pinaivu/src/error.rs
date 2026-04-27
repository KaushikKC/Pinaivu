use anchor_lang::prelude::*;

#[error_code]
pub enum PeerAiError {
    #[msg("Escrow is not in Locked state")]
    EscrowNotLocked,
    #[msg("Escrow has expired — client may now reclaim funds")]
    EscrowExpired,
    #[msg("Escrow timeout has not elapsed yet")]
    EscrowNotExpired,
    #[msg("Caller is not authorized for this operation")]
    Unauthorized,
    #[msg("proof_hash must be 32 non-zero bytes")]
    InvalidProofHash,
    #[msg("Too many models — maximum 8")]
    TooManyModels,
}
