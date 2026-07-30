use std::time::{SystemTime, UNIX_EPOCH};

use super::proto::{CodebaseIndexStatus, CodebaseIndexStatusState};

fn current_epoch_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

pub(super) fn queued_codebase_index_status(repo_path: String) -> CodebaseIndexStatus {
    base_codebase_index_status(repo_path, CodebaseIndexStatusState::Queued)
}

pub(super) fn not_enabled_codebase_index_status(repo_path: String) -> CodebaseIndexStatus {
    base_codebase_index_status(repo_path, CodebaseIndexStatusState::NotEnabled)
}

pub(super) fn disabled_codebase_index_status(repo_path: String) -> CodebaseIndexStatus {
    base_codebase_index_status(repo_path, CodebaseIndexStatusState::Disabled)
}
pub(super) fn unavailable_codebase_index_status(
    repo_path: String,
    failure_message: String,
) -> CodebaseIndexStatus {
    CodebaseIndexStatus {
        failure_message: Some(failure_message),
        ..base_codebase_index_status(repo_path, CodebaseIndexStatusState::Unavailable)
    }
}

fn base_codebase_index_status(
    repo_path: String,
    state: CodebaseIndexStatusState,
) -> CodebaseIndexStatus {
    CodebaseIndexStatus {
        repo_path,
        state: state.into(),
        last_updated_epoch_millis: Some(current_epoch_millis()),
        progress_completed: None,
        progress_total: None,
        failure_message: None,
        root_hash: None,
    }
}

// LOCAL FORK: `codebase_index_status_to_proto` and its helpers
// (`codebase_index_status_state`, `codebase_index_status_state_from_parts`,
// `progress_from_sync_progress`, `progress_from_codebase_index_status`,
// `failure_message_from_last_sync_result`, `failure_message_from_codebase_index_status`)
// projected a local `CodebaseIndexStatus` from the source-code embedding index manager
// onto the wire type. They went with the codebase indexing surface, together with the
// unit tests in `codebase_index_status_tests.rs` that covered
// `codebase_index_status_state_from_parts`. The constructors above stay: they build
// fixed-state responses and never touched the index manager.
