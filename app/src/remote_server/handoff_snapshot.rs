//! Daemon-side handler for the `UploadHandoffSnapshot` RPC.
//!
//! LOCAL FORK: this used to gather git patches and orphan file contents from
//! the remote host and upload them to GCS via `ai::agent_sdk`'s
//! `upload_snapshot_for_handoff`, so a local agent session could be handed off
//! to a cloud agent. `ai/agent_sdk` has been removed, so the pipeline no longer
//! exists.
//!
//! The RPC itself is retained rather than deleted because `host_scoped_request`
//! is a protobuf enum and `server_model`'s dispatch match must stay exhaustive.
//! It now fails explicitly instead of silently returning `Ok(None)`, which the
//! caller would have read as "empty workspace, nothing to upload".

use std::sync::Arc;

use anyhow::{Result, bail};
use warp_util::standardized_path::StandardizedPath;

use crate::server::server_api::ai::{AIClient, InitialSnapshotToken};

/// Always fails: agent handoff is not supported in this build.
pub(crate) async fn gather_and_upload_handoff_snapshot(
    _paths: Vec<StandardizedPath>,
    _ai_client: Arc<dyn AIClient>,
    _http: &http_client::Client,
) -> Result<Option<InitialSnapshotToken>> {
    bail!("agent handoff is not supported in this build");
}
