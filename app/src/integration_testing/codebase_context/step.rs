//! LOCAL FORK: this module's only step, `sync_current_codebase_index`, drove
//! `CodebaseIndexManager` and went out with the embedding-based codebase index.
//! It had no callers left. The empty module is kept so `codebase_context/mod.rs`
//! still resolves; the whole `codebase_context` directory can go once
//! `integration_testing/mod.rs` drops its `pub mod codebase_context;`.
