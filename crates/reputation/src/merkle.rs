//! Simple binary Merkle tree for ProofOfInference receipts.
//!
//! Each leaf is the SHA-256 of a proof's canonical bytes (i.e. `proof.id()`).
//! Internal nodes are `SHA-256(left_hash ‖ right_hash)`.
//!
//! The tree is rebuilt from scratch on each mutation — receipt histories are
//! not expected to be extremely large (tens of thousands of entries at most),
//! and correctness is more important than incremental-update performance here.

use sha2::{Digest, Sha256};

/// A Merkle path element: the sibling hash and which side the current node is on.
#[derive(Debug, Clone)]
pub struct MerklePathStep {
    pub sibling: [u8; 32],
    pub is_left: bool, // true = current node is the left child
}

/// Minimal binary Merkle tree.
///
/// Leaves are stored in insertion order. An odd number of leaves is handled
/// by duplicating the last leaf when building the tree (standard approach).
#[derive(Debug, Clone, Default)]
pub struct MerkleTree {
    leaves: Vec<[u8; 32]>,
}

impl MerkleTree {
    pub fn new() -> Self {
        Self { leaves: Vec::new() }
    }

    /// Insert a new leaf hash (call `proof.id()` to get this).
    pub fn insert(&mut self, leaf_hash: [u8; 32]) {
        self.leaves.push(leaf_hash);
    }

    /// Number of leaves.
    pub fn len(&self) -> usize {
        self.leaves.len()
    }

    pub fn is_empty(&self) -> bool {
        self.leaves.is_empty()
    }

    /// Compute the Merkle root.
    ///
    /// Returns `[0u8; 32]` for an empty tree.
    pub fn root(&self) -> [u8; 32] {
        if self.leaves.is_empty() {
            return [0u8; 32];
        }
        build_level(&self.leaves)
    }

    /// Generate a Merkle proof (sibling path) for the leaf at `index`.
    ///
    /// Returns `None` if `index` is out of bounds.
    /// Use `verify_proof` to check the returned path against a known root.
    pub fn proof(&self, index: usize) -> Option<Vec<MerklePathStep>> {
        if index >= self.leaves.len() {
            return None;
        }

        let mut path = Vec::new();
        let mut current_level = pad_to_even(self.leaves.clone());
        let mut current_index = index;

        while current_level.len() > 1 {
            let sibling_index = if current_index % 2 == 0 {
                current_index + 1
            } else {
                current_index - 1
            };

            path.push(MerklePathStep {
                sibling: current_level[sibling_index],
                is_left: current_index % 2 == 0,
            });

            current_level = hash_pairs(&current_level);
            current_index /= 2;
        }

        Some(path)
    }
}

/// Verify a Merkle proof against a known root.
///
/// `leaf_hash`  — the hash of the leaf being proven (i.e. `proof.id()`)
/// `path`       — the sibling path returned by `MerkleTree::proof`
/// `root`       — the expected root hash
pub fn verify_proof(
    leaf_hash: [u8; 32],
    path:      &[MerklePathStep],
    root:      [u8; 32],
) -> bool {
    let mut current = leaf_hash;
    for step in path {
        current = if step.is_left {
            hash_pair(current, step.sibling)
        } else {
            hash_pair(step.sibling, current)
        };
    }
    current == root
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn hash_pair(left: [u8; 32], right: [u8; 32]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(left);
    h.update(right);
    h.finalize().into()
}

fn pad_to_even(mut level: Vec<[u8; 32]>) -> Vec<[u8; 32]> {
    if level.len() % 2 == 1 {
        let last = *level.last().unwrap();
        level.push(last);
    }
    level
}

fn hash_pairs(level: &[[u8; 32]]) -> Vec<[u8; 32]> {
    let padded = pad_to_even(level.to_vec());
    padded.chunks(2).map(|pair| hash_pair(pair[0], pair[1])).collect()
}

fn build_level(leaves: &[[u8; 32]]) -> [u8; 32] {
    let mut current = leaves.to_vec();
    while current.len() > 1 {
        current = hash_pairs(&current);
    }
    current[0]
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn leaf(n: u8) -> [u8; 32] {
        let mut h = Sha256::new();
        h.update([n]);
        h.finalize().into()
    }

    #[test]
    fn empty_tree_root_is_zero() {
        assert_eq!(MerkleTree::new().root(), [0u8; 32]);
    }

    #[test]
    fn single_leaf_root_is_leaf() {
        let mut tree = MerkleTree::new();
        tree.insert(leaf(1));
        assert_eq!(tree.root(), leaf(1));
    }

    #[test]
    fn two_leaf_root() {
        let mut tree = MerkleTree::new();
        tree.insert(leaf(1));
        tree.insert(leaf(2));
        let expected = hash_pair(leaf(1), leaf(2));
        assert_eq!(tree.root(), expected);
    }

    #[test]
    fn odd_leaves_pads_last() {
        let mut t3 = MerkleTree::new();
        t3.insert(leaf(1));
        t3.insert(leaf(2));
        t3.insert(leaf(3));
        // Should produce same root as 4-leaf tree with leaf(3) duplicated
        let mut t4 = MerkleTree::new();
        t4.insert(leaf(1));
        t4.insert(leaf(2));
        t4.insert(leaf(3));
        t4.insert(leaf(3));
        assert_eq!(t3.root(), t4.root());
    }

    #[test]
    fn proof_verification_succeeds() {
        let mut tree = MerkleTree::new();
        for i in 0..8u8 {
            tree.insert(leaf(i));
        }
        let root = tree.root();
        for i in 0..8usize {
            let path = tree.proof(i).unwrap();
            assert!(
                verify_proof(leaf(i as u8), &path, root),
                "proof for leaf {i} should verify"
            );
        }
    }

    #[test]
    fn tampered_leaf_fails_verification() {
        let mut tree = MerkleTree::new();
        tree.insert(leaf(0));
        tree.insert(leaf(1));
        let root = tree.root();
        let path = tree.proof(0).unwrap();
        assert!(!verify_proof(leaf(99), &path, root));
    }
}
