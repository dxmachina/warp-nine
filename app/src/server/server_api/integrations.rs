// LOCAL FORK: check_user_repo_auth_status, create_or_update_simple_integration,
// list_simple_integrations, poll_oauth_connect_status and get_integrations_using_environment
// went with the cloud-agent integrations UI. Only the two calls the environment settings form
// still makes are left.

use anyhow::{Result, anyhow};
use async_trait::async_trait;
use cynic::QueryBuilder;
#[cfg(test)]
use mockall::automock;
use warp_graphql::queries::suggest_cloud_environment_image::{
    RepoInput as SuggestCloudEnvironmentImageRepoInput, SuggestCloudEnvironmentImage,
    SuggestCloudEnvironmentImageInput, SuggestCloudEnvironmentImageResult,
    SuggestCloudEnvironmentImageVariables,
};
use warp_graphql::queries::user_github_info::{
    GithubAuthRequiredOutput, UserGithubInfo, UserGithubInfoResult, UserGithubInfoVariables,
};

use super::ServerApi;
use crate::channel::ChannelState;
use crate::features::FeatureFlag;
use crate::server::graphql::{get_request_context, get_user_facing_error_message};

#[cfg(not(target_family = "wasm"))]
pub trait IntegrationsClientBounds: Send + Sync {}

#[cfg(not(target_family = "wasm"))]
impl<T: 'static + Send + Sync> IntegrationsClientBounds for T {}

#[cfg(target_family = "wasm")]
pub trait IntegrationsClientBounds {}

#[cfg(target_family = "wasm")]
impl<T: 'static> IntegrationsClientBounds for T {}

#[cfg_attr(test, automock)]
#[cfg_attr(target_family = "wasm", allow(dead_code))]
#[cfg_attr(target_family = "wasm", async_trait(?Send))]
#[cfg_attr(not(target_family = "wasm"), async_trait)]
pub trait IntegrationsClient: 'static + IntegrationsClientBounds {
    /// Gets the user's GitHub connection info, including accessible repos.
    ///
    /// # Returns
    /// * `Ok(UserGithubInfoResult)` - Either connected with repos, or auth required
    /// * `Err` - If the query fails
    async fn get_user_github_info(&self) -> Result<UserGithubInfoResult>;

    /// Suggests a Docker image for a cloud environment based on the provided repos.
    async fn suggest_cloud_environment_image(
        &self,
        repos: Vec<(String, String)>,
    ) -> Result<SuggestCloudEnvironmentImageResult>;
}

#[cfg_attr(target_family = "wasm", async_trait(?Send))]
#[cfg_attr(not(target_family = "wasm"), async_trait)]
impl IntegrationsClient for ServerApi {
    async fn get_user_github_info(&self) -> Result<UserGithubInfoResult> {
        let variables = UserGithubInfoVariables {
            request_context: get_request_context(),
        };

        let operation = UserGithubInfo::build(variables);
        let response = self.send_graphql_request(operation, None).await?;

        let result = response.user_github_info;

        // Dev-only helper for testing GitHub-unauthed flows.
        //
        // Important: this runs after the network request completes so the UI can still
        // show the loading state.
        if FeatureFlag::SimulateGithubUnauthed.is_enabled()
            && let UserGithubInfoResult::GithubConnectedOutput(connected) = &result
        {
            let auth_url = format!("{}/oauth/connect/github", ChannelState::server_root_url());
            return Ok(UserGithubInfoResult::GithubAuthRequiredOutput(
                GithubAuthRequiredOutput {
                    auth_url,
                    // This value is unused by the app UI; it exists in the schema for
                    // tx-bound flows. We intentionally omit txId from the auth URL so
                    // the web flow can proceed without a server-created tx.
                    tx_id: cynic::Id::new("simulated"),
                    app_install_link: connected.app_install_link.clone(),
                },
            ));
        }

        Ok(result)
    }

    async fn suggest_cloud_environment_image(
        &self,
        repos: Vec<(String, String)>,
    ) -> Result<SuggestCloudEnvironmentImageResult> {
        let repo_inputs: Vec<SuggestCloudEnvironmentImageRepoInput> = repos
            .into_iter()
            .map(|(owner, repo)| SuggestCloudEnvironmentImageRepoInput { owner, repo })
            .collect();

        let variables = SuggestCloudEnvironmentImageVariables {
            request_context: get_request_context(),
            input: SuggestCloudEnvironmentImageInput { repos: repo_inputs },
        };

        let operation = SuggestCloudEnvironmentImage::build(variables);
        let response = self.send_graphql_request(operation, None).await?;

        match response.suggest_cloud_environment_image {
            SuggestCloudEnvironmentImageResult::SuggestCloudEnvironmentImageAuthRequiredOutput(
                output,
            ) => Ok(
                SuggestCloudEnvironmentImageResult::SuggestCloudEnvironmentImageAuthRequiredOutput(
                    output,
                ),
            ),
            SuggestCloudEnvironmentImageResult::SuggestCloudEnvironmentImageOutput(output) => {
                Ok(SuggestCloudEnvironmentImageResult::SuggestCloudEnvironmentImageOutput(output))
            }
            SuggestCloudEnvironmentImageResult::UserFacingError(error) => {
                Err(anyhow!(get_user_facing_error_message(error)))
            }
            SuggestCloudEnvironmentImageResult::Unknown => Err(anyhow!(
                "Unknown response from suggestCloudEnvironmentImage query"
            )),
        }
    }
}
