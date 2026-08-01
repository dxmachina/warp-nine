// LOCAL FORK: `gql_convert` held the conversions from the `get_workspaces_metadata_for_user`
// response into the workspace, team, policy and billing models -- roughly 1,000 lines of
// `From` impls with no other caller. The query went with `TeamClient`.
pub mod team;
pub mod team_tester;
pub mod update_manager;
pub mod user_profiles;
pub mod user_workspaces;
pub mod workspace;
