use anyhow::{Result, anyhow};
use async_trait::async_trait;
use cynic::{MutationBuilder, QueryBuilder};
#[cfg(test)]
use mockall::{automock, predicate::*};
use warp_graphql::queries::get_workspaces_metadata_for_user::{
    GetWorkspacesMetadataForUser, GetWorkspacesMetadataForUserVariables, PricingInfoResult,
};

use super::ServerApi;
use crate::server::graphql::get_request_context;
use crate::workspaces::user_workspaces::WorkspacesMetadataWithPricing;

#[cfg_attr(test, automock)]
#[cfg_attr(not(target_family = "wasm"), async_trait)]
#[cfg_attr(target_family = "wasm", async_trait(?Send))]
pub trait TeamClient: 'static + Send + Sync {
    async fn workspaces_metadata(&self) -> Result<WorkspacesMetadataWithPricing>;
}

#[cfg_attr(not(target_family = "wasm"), async_trait)]
#[cfg_attr(target_family = "wasm", async_trait(?Send))]
impl TeamClient for ServerApi {
    #[tracing::instrument(skip_all, err, fields(tags.cloud_agent = true))]
    async fn workspaces_metadata(&self) -> Result<WorkspacesMetadataWithPricing> {
        let variables = GetWorkspacesMetadataForUserVariables {
            request_context: get_request_context(),
        };
        let operation = GetWorkspacesMetadataForUser::build(variables);
        let response = self.send_graphql_request(operation, None).await?;

        let metadata = match response.user {
            warp_graphql::queries::get_workspaces_metadata_for_user::UserResult::UserOutput(
                user_output,
            ) => user_output.user.into(),
            warp_graphql::queries::get_workspaces_metadata_for_user::UserResult::Unknown => {
                return Err(anyhow!("Unable to fetch workspaces metadata"));
            }
        };

        let pricing_info = match response.pricing_info {
            PricingInfoResult::PricingInfoOutput(pricing_output) => {
                Some(pricing_output.pricing_info)
            }
            PricingInfoResult::Unknown => None,
        };

        Ok(WorkspacesMetadataWithPricing {
            metadata,
            pricing_info,
        })
    }
}
