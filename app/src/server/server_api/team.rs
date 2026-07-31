use anyhow::{Result, anyhow};
use async_trait::async_trait;
use cynic::{MutationBuilder, QueryBuilder};
#[cfg(test)]
use mockall::{automock, predicate::*};
use warp_graphql::mutations::add_invite_link_domain_restriction::{
    AddInviteLinkDomainRestriction, AddInviteLinkDomainRestrictionInput,
    AddInviteLinkDomainRestrictionResult, AddInviteLinkDomainRestrictionVariables,
};
use warp_graphql::mutations::create_team::{
    CreateTeam, CreateTeamInput, CreateTeamResult, CreateTeamVariables,
};
use warp_graphql::mutations::delete_invite_link_domain_restriction::{
    DeleteInviteLinkDomainRestriction, DeleteInviteLinkDomainRestrictionInput,
    DeleteInviteLinkDomainRestrictionResult, DeleteInviteLinkDomainRestrictionVariables,
};
use warp_graphql::mutations::delete_team_invite::{
    DeleteTeamInvite, DeleteTeamInviteInput, DeleteTeamInviteResult, DeleteTeamInviteVariables,
};
use warp_graphql::mutations::join_team_with_team_discovery::{
    JoinTeamWithTeamDiscovery, JoinTeamWithTeamDiscoveryInput, JoinTeamWithTeamDiscoveryResult,
    JoinTeamWithTeamDiscoveryVariables, TeamDiscoveryEntrypoint,
};
use warp_graphql::mutations::remove_user_from_team::{
    RemoveUserFromTeam, RemoveUserFromTeamInput, RemoveUserFromTeamResult,
    RemoveUserFromTeamVariables,
};
use warp_graphql::mutations::rename_team::{
    RenameTeam, RenameTeamInput, RenameTeamResult, RenameTeamVariables,
};
use warp_graphql::mutations::reset_invite_links::{
    ResetInviteLinks, ResetInviteLinksInput, ResetInviteLinksResult, ResetInviteLinksVariables,
};
use warp_graphql::mutations::send_team_invite_email::{
    SendTeamInviteEmail, SendTeamInviteEmailInput, SendTeamInviteEmailResult,
    SendTeamInviteEmailVariables,
};
use warp_graphql::mutations::set_is_invite_link_enabled::{
    SetIsInviteLinkEnabled, SetIsInviteLinkEnabledInput, SetIsInviteLinkEnabledResult,
    SetIsInviteLinkEnabledVariables,
};
use warp_graphql::mutations::set_team_discoverability::{
    SetTeamDiscoverability, SetTeamDiscoverabilityInput, SetTeamDiscoverabilityResult,
    SetTeamDiscoverabilityVariables,
};
use warp_graphql::mutations::set_team_member_role::{
    SetTeamMemberRole, SetTeamMemberRoleInput, SetTeamMemberRoleResult, SetTeamMemberRoleVariables,
};
use warp_graphql::mutations::transfer_team_ownership::{
    TransferTeamOwnership, TransferTeamOwnershipInput, TransferTeamOwnershipResult,
    TransferTeamOwnershipVariables,
};
use warp_graphql::queries::get_discoverable_teams::{
    GetDiscoverableTeams, GetDiscoverableTeamsVariables,
};
use warp_graphql::queries::get_workspaces_metadata_for_user::{
    GetWorkspacesMetadataForUser, GetWorkspacesMetadataForUserVariables, PricingInfoResult,
};

use super::ServerApi;
use crate::auth::UserUid;
use crate::cloud_object::CloudObjectEventEntrypoint;
use crate::server::graphql::{get_request_context, get_user_facing_error_message};
use crate::server::ids::ServerId;
use crate::workspaces::team::{DiscoverableTeam, MembershipRole};
use crate::workspaces::user_workspaces::{CreateTeamResponse, WorkspacesMetadataWithPricing};
use crate::workspaces::workspace::Workspace;

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
