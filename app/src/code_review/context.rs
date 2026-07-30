cfg_if::cfg_if! {
    if #[cfg(feature = "local_fs")] {
        use crate::code_review::{diff_state::DiffMode, DiffSetScope};
    }
}
// LOCAL FORK: fn convert_file_diffs_to_diffset_hunks removed with the agent.

/// Creates attachment reference and key for a set of changes based on scope and diff mode
#[cfg(feature = "local_fs")]
pub fn create_attachment_reference_and_key(
    scope: &DiffSetScope,
    diff_mode: &DiffMode,
    main_branch_name: Option<&str>,
) -> (String, String) {
    match scope {
        DiffSetScope::All => {
            let diff_set_description = match diff_mode {
                DiffMode::Head => "uncommitted changes".to_string(),
                DiffMode::MainBranch => {
                    let main_branch = main_branch_name.unwrap_or("main");
                    format!("diffset against {main_branch}")
                }
                DiffMode::OtherBranch(branch_name) => {
                    format!("diffset against {branch_name}")
                }
            };
            let key = diff_set_description.clone();
            (format!("<change:{key}>"), key)
        }
        DiffSetScope::File(repo_relative_path) => {
            debug_assert!(!std::path::Path::new(repo_relative_path).is_absolute());
            let key = repo_relative_path.clone();
            (format!("<change:{key}>"), key)
        }
    }
}

// LOCAL FORK: fn register_diffset_attachment removed with the agent.
