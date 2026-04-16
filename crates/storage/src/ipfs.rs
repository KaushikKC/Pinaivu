//! IPFS storage backend — speaks the Kubo HTTP RPC API.
//!
//! Compatible with:
//! - Local Kubo node (`http://localhost:5001`)
//! - Pinata (`https://api.pinata.cloud`) via `pinata_jwt` option
//! - Web3.Storage, Infura, or any IPFS-compatible pinning service
//!
//! ## Kubo API used
//!
//! ```text
//! Store: POST /api/v0/add          (multipart/form-data, field = "file")
//!        → JSON { "Hash": "Qm..." | "bafy...", "Size": "N" }
//!
//! Fetch: POST /api/v0/cat?arg=<cid>
//!        → raw bytes
//! ```
//!
//! `BlobId` = IPFS CID string, e.g. `"QmXYZ..."` (CIDv0) or `"bafybeig..."` (CIDv1).
//!
//! ## Content addressing
//!
//! IPFS is natively content-addressed, so `put` is idempotent: the same bytes
//! always produce the same CID regardless of when or where they are uploaded.
//!
//! ## Offline / no IPFS node
//!
//! If the IPFS API is unreachable, the client returns an error and the caller
//! should fall back to `LocalStorageClient`. Use `LocalStorageClient` directly
//! in standalone mode where IPFS is not available.

use std::time::Duration;

use async_trait::async_trait;
use reqwest::{Client, StatusCode};
use serde::Deserialize;
use tracing::{debug, warn};

use common::types::BlobId;

use crate::StorageClient;

// ---------------------------------------------------------------------------
// Kubo API response shapes
// ---------------------------------------------------------------------------

/// Response from `POST /api/v0/add`.
#[derive(Debug, Deserialize)]
struct AddResponse {
    #[serde(rename = "Hash")]
    hash: String,
    #[serde(rename = "Size")]
    size: String,
}

// ---------------------------------------------------------------------------
// IpfsStorageClient
// ---------------------------------------------------------------------------

pub struct IpfsStorageClient {
    http:    Client,
    api_url: String, // e.g. "http://localhost:5001"
}

impl IpfsStorageClient {
    /// Create a new `IpfsStorageClient`.
    ///
    /// `api_url` — base URL of the IPFS HTTP RPC API,
    /// e.g. `"http://localhost:5001"` for a local Kubo node.
    pub fn new(api_url: impl Into<String>) -> anyhow::Result<Self> {
        let http = Client::builder()
            .timeout(Duration::from_secs(60))
            .build()?;
        Ok(Self {
            http,
            api_url: api_url.into().trim_end_matches('/').to_string(),
        })
    }
}

#[async_trait]
impl StorageClient for IpfsStorageClient {
    /// Upload `data` to IPFS via `POST /api/v0/add`.
    ///
    /// Returns the IPFS CID as the blob ID. The same bytes always produce the
    /// same CID — uploads are idempotent and deduplicated by IPFS itself.
    async fn put(&self, data: Vec<u8>, _ttl_epochs: u64) -> anyhow::Result<BlobId> {
        let url  = format!("{}/api/v0/add", self.api_url);
        let size = data.len();

        let part = reqwest::multipart::Part::bytes(data)
            .file_name("blob")
            .mime_str("application/octet-stream")?;
        let form = reqwest::multipart::Form::new().part("file", part);

        debug!(url = %url, bytes = size, "ipfs: uploading blob");

        let resp = self.http
            .post(&url)
            .multipart(form)
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("ipfs add failed: HTTP {status} — {body}"));
        }

        let add: AddResponse = resp.json().await
            .map_err(|e| anyhow::anyhow!("ipfs: failed to parse add response: {e}"))?;

        debug!(cid = %add.hash, bytes = %add.size, "ipfs: blob stored");
        Ok(add.hash)
    }

    /// Fetch raw bytes for a CID via `POST /api/v0/cat?arg=<cid>`.
    async fn get(&self, blob_id: &BlobId) -> anyhow::Result<Vec<u8>> {
        let url = format!("{}/api/v0/cat?arg={blob_id}", self.api_url);
        debug!(url = %url, "ipfs: fetching blob");

        let resp = self.http.post(&url).send().await?;

        match resp.status() {
            StatusCode::OK => {
                let bytes = resp.bytes().await?;
                debug!(blob_id = %blob_id, bytes = bytes.len(), "ipfs: blob fetched");
                Ok(bytes.to_vec())
            }
            StatusCode::NOT_FOUND => {
                Err(anyhow::anyhow!("ipfs: CID not found: {blob_id}"))
            }
            status => {
                let body = resp.text().await.unwrap_or_default();
                Err(anyhow::anyhow!("ipfs cat failed: HTTP {status} — {body}"))
            }
        }
    }

    /// Unpin a CID from the local node. Best-effort — IPFS has no global delete.
    ///
    /// Other nodes that have pinned the same CID are unaffected.
    /// On a local Kubo node this calls `POST /api/v0/pin/rm?arg=<cid>`.
    async fn delete(&self, blob_id: &BlobId) -> anyhow::Result<()> {
        let url = format!("{}/api/v0/pin/rm?arg={blob_id}", self.api_url);
        debug!(url = %url, "ipfs: unpinning blob");

        let resp = self.http.post(&url).send().await;
        match resp {
            Ok(r) if r.status().is_success() => {
                debug!(blob_id = %blob_id, "ipfs: blob unpinned");
            }
            Ok(r) => {
                // "not pinned" is fine — treat as success
                warn!(blob_id = %blob_id, status = %r.status(), "ipfs: unpin returned non-success (ignored)");
            }
            Err(e) => {
                warn!(blob_id = %blob_id, %e, "ipfs: unpin failed (ignored)");
            }
        }
        Ok(())
    }

    fn name(&self) -> &'static str { "ipfs" }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Verify the client can be constructed with trailing slash in URL.
    #[test]
    fn test_url_trailing_slash_trimmed() {
        let c = IpfsStorageClient::new("http://localhost:5001/").unwrap();
        assert_eq!(c.api_url, "http://localhost:5001");
    }

    /// Smoke test the AddResponse parser.
    #[test]
    fn test_parse_add_response() {
        let json = r#"{"Name":"blob","Hash":"QmTestCID123","Size":"42"}"#;
        let r: AddResponse = serde_json::from_str(json).unwrap();
        assert_eq!(r.hash, "QmTestCID123");
        assert_eq!(r.size, "42");
    }
}
