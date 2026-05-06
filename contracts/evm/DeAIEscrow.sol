// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

/// @title DeAI Escrow
/// @notice Escrow + reputation anchor for the DeAI decentralised inference network.
///
/// Flow (client-pays model):
///   1. Client calls lockEscrow{value: payment}(requestId, nodeAddress) → escrowId
///   2. Node completes inference, calls releaseEscrow(escrowId, proofHash) → funds sent to node
///   3. If node fails, client calls refundEscrow(escrowId) after REFUND_TIMEOUT → funds returned
///
/// For demo / standalone mode (node self-signs everything):
///   The node's wallet can act as both payer and node — lockEscrow with msg.value > 0,
///   releaseEscrow immediately after. The on-chain proof hash is what matters.
///
/// Reputation anchoring (chain-agnostic):
///   Any node can call anchorRoot(merkleRoot, label) to pin a 32-byte Merkle root
///   of its inference proofs on-chain.  No ETH required.
contract DeAIEscrow {

    // ── Constants ────────────────────────────────────────────────────────────

    uint256 public constant REFUND_TIMEOUT = 1 hours;

    // ── Storage ──────────────────────────────────────────────────────────────

    struct Escrow {
        bytes32 requestId;
        address payer;
        address node;
        uint256 amount;
        uint256 createdAt;
        bool    settled;
    }

    uint256 private _nextId = 1;

    mapping(uint256 => Escrow)  public escrows;
    /// Reverse lookup: requestId → escrowId (first lock wins)
    mapping(bytes32 => uint256) public escrowByRequest;

    // ── Events ───────────────────────────────────────────────────────────────

    event EscrowLocked(
        uint256 indexed escrowId,
        bytes32 indexed requestId,
        address indexed node,
        address         payer,
        uint256         amount
    );

    event EscrowReleased(
        uint256 indexed escrowId,
        bytes32         proofHash,
        address indexed node,
        uint256         amount
    );

    event EscrowRefunded(
        uint256 indexed escrowId,
        address indexed payer,
        uint256         amount
    );

    event RootAnchored(
        bytes32 indexed rootHash,
        bytes32         label,
        address indexed anchor
    );

    // ── Errors ───────────────────────────────────────────────────────────────

    error AlreadySettled(uint256 escrowId);
    error TimeoutNotElapsed(uint256 escrowId, uint256 unlocksAt);
    error NotAuthorized();
    error TransferFailed();
    error ZeroValue();
    error InvalidNode();

    // ── External functions ───────────────────────────────────────────────────

    /// @notice Lock ETH in escrow for an inference request.
    /// @param requestId  32-byte request ID (matches ProofOfInference.settlement_id on-chain)
    /// @param nodeAddress  The node expected to fulfil this request
    /// @return escrowId  Unique escrow counter, passed to release/refund
    function lockEscrow(bytes32 requestId, address nodeAddress)
        external
        payable
        returns (uint256 escrowId)
    {
        if (msg.value == 0)          revert ZeroValue();
        if (nodeAddress == address(0)) revert InvalidNode();

        escrowId = _nextId++;
        escrows[escrowId] = Escrow({
            requestId: requestId,
            payer:     msg.sender,
            node:      nodeAddress,
            amount:    msg.value,
            createdAt: block.timestamp,
            settled:   false
        });

        if (escrowByRequest[requestId] == 0) {
            escrowByRequest[requestId] = escrowId;
        }

        emit EscrowLocked(escrowId, requestId, nodeAddress, msg.sender, msg.value);
    }

    /// @notice Release escrowed ETH to the node after successful inference.
    /// @dev    Callable by the node address stored in the escrow, or the payer
    ///         (payer releasing early = they're satisfied).
    /// @param escrowId  From lockEscrow return value
    /// @param proofHash  SHA-256 hash of the ProofOfInference canonical bytes
    function releaseEscrow(uint256 escrowId, bytes32 proofHash) external {
        Escrow storage e = escrows[escrowId];
        if (e.settled)                                        revert AlreadySettled(escrowId);
        if (msg.sender != e.node && msg.sender != e.payer)   revert NotAuthorized();

        e.settled = true;
        uint256 amount = e.amount;
        address node   = e.node;

        (bool ok,) = node.call{value: amount}("");
        if (!ok) revert TransferFailed();

        emit EscrowReleased(escrowId, proofHash, node, amount);
    }

    /// @notice Refund escrowed ETH to the payer after the timeout window.
    /// @dev    Only callable by the payer, only after REFUND_TIMEOUT has elapsed.
    function refundEscrow(uint256 escrowId) external {
        Escrow storage e = escrows[escrowId];
        if (e.settled) revert AlreadySettled(escrowId);

        uint256 unlocksAt = e.createdAt + REFUND_TIMEOUT;
        if (block.timestamp < unlocksAt) revert TimeoutNotElapsed(escrowId, unlocksAt);
        if (msg.sender != e.payer)       revert NotAuthorized();

        e.settled = true;
        uint256 amount = e.amount;
        address payer  = e.payer;

        (bool ok,) = payer.call{value: amount}("");
        if (!ok) revert TransferFailed();

        emit EscrowRefunded(escrowId, payer, amount);
    }

    /// @notice Anchor a Merkle root of inference proofs on-chain.
    /// @dev    No ETH required. Anyone (any node) can anchor.
    ///         The Rust daemon calls this via EvmSettlement::anchor_hash().
    /// @param rootHash  32-byte Merkle root (SHA-256 tree of ProofOfInference hashes)
    /// @param label     Arbitrary label, e.g. keccak256(node_pubkey ‖ timestamp)
    function anchorRoot(bytes32 rootHash, bytes32 label) external {
        emit RootAnchored(rootHash, label, msg.sender);
    }

    // ── View helpers ─────────────────────────────────────────────────────────

    function getEscrow(uint256 escrowId) external view returns (Escrow memory) {
        return escrows[escrowId];
    }

    function isSettled(uint256 escrowId) external view returns (bool) {
        return escrows[escrowId].settled;
    }
}
