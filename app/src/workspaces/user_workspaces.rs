use crate::settings::{
    AISettings, AISettingsChangedEvent, CodeSettings, CodeSettingsChangedEvent, PrivacySettings,
};
use std::collections::HashMap;

use regex::Regex;
use warp_core::features::FeatureFlag;

// LOCAL FORK: `MembershipRole` and `WorkspaceMemberUsageInfo` below are read only by
// `#[cfg(test)]` code, so the lib target reports them unused.
#[allow(unused_imports)]
use super::team::{MembershipRole, Team};
#[allow(unused_imports)]
use super::workspace::WorkspaceMemberUsageInfo;
#[allow(unused_imports)]
use crate::auth::{AuthStateProvider, UserUid};
use warp_core::settings::{ChangeEventReason, Setting};
use warpui::{
    AppContext, Entity, ModelContext, SingletonEntity, Tracked, ViewContext, WeakViewHandle,
    WindowId,
};

use super::workspace::{
    AdminEnablementSetting, BillingMetadata, CustomerType, EnterpriseSecretRegex,
    HostEnablementSetting, UgcCollectionEnablementSetting, Workspace, WorkspaceUid,
};
use crate::channel::ChannelState;
use crate::cloud_object::model::persistence::CloudModel;
use crate::cloud_object::{Owner, Space};

use crate::server::ids::ServerId;
#[cfg(test)]
#[cfg(test)]
use crate::workspaces::workspace::{AIAutonomyPolicy, WorkspaceMember, WorkspaceSettings};
use crate::workspaces::workspace::{
    AiAutonomySettings, SandboxedAgentSettings, UsageBasedPricingSettings,
};

const STRIPE_SUBSCRIPTION_INTERVAL_PAGE_PREFIX: &str = "/upgrade";

#[derive(Debug)]
pub enum UserWorkspacesEvent {
    // LOCAL FORK: the upgrade-link and Stripe billing-portal events went with billing.
    /// Fired whenever the set of teams the user is on changes.
    TeamsChanged,
    CodebaseContextEnablementChanged,
    /// Fired when a service agreement's sunsetted_to_build_ts field is updated.
    SunsettedToBuildDataUpdated,
}

/// UserWorkspaces is a singleton model that holds workspace metadata (name, members, etc).
/// It should be used for getting information about the workspaces, teams, current teams,
/// and all other things related to operating on workspace and team data.
/// TODO: move other server_api calls to update_manager to correctly update sqlite.
pub struct UserWorkspaces {
    current_workspace_uid: Tracked<Option<WorkspaceUid>>,
    workspaces: Tracked<Vec<Workspace>>,
    window_team_uids: HashMap<WindowId, Option<ServerId>>,
}

pub struct CreateTeamResponse {
    pub workspace: Workspace,
    pub team: Team,
}

impl UserWorkspaces {
    #[cfg(test)]
    pub fn mock(cached_workspaces: Vec<Workspace>, _ctx: &mut ModelContext<Self>) -> Self {
        // In tests, avoid subscribing to [`ServerExperiments`] because it
        // requires us to register that singleton along with _its_ dependencies
        // for all tests that use [`UserWorkspaces`] (a lot of them do).
        Self {
            current_workspace_uid: cached_workspaces.first().map(|w| w.uid).into(),
            workspaces: cached_workspaces.into(),
            window_team_uids: Default::default(),
        }
    }

    #[cfg(test)]
    pub fn default_mock(ctx: &mut ModelContext<Self>) -> Self {
        Self::mock(vec![], ctx)
    }

    pub fn new(
        cached_workspaces: Vec<Workspace>,
        current_workspace_uid: Option<WorkspaceUid>,
        ctx: &mut ModelContext<Self>,
    ) -> Self {
        // LOCAL FORK: this subscription re-evaluated session-sharing enablement whenever
        // the server experiment set changed.

        ctx.subscribe_to_model(
            &CodeSettings::handle(ctx),
            |_, _, code_settings_event, ctx| match code_settings_event {
                CodeSettingsChangedEvent::CodebaseContextEnabled { .. }
                | CodeSettingsChangedEvent::AutoIndexingEnabled { .. } => {
                    ctx.emit(UserWorkspacesEvent::CodebaseContextEnablementChanged);
                }
                _ => {}
            },
        );

        ctx.subscribe_to_model(&AISettings::handle(ctx), |_, _, ai_settings_event, ctx| {
            if let AISettingsChangedEvent::IsAnyAIEnabled { .. } = ai_settings_event {
                ctx.emit(UserWorkspacesEvent::CodebaseContextEnablementChanged);
            }
        });

        Self {
            current_workspace_uid: current_workspace_uid.into(),
            workspaces: cached_workspaces.into(),
            window_team_uids: Default::default(),
        }
    }

    pub fn team_from_uid(&self, team_uid: ServerId) -> Option<&Team> {
        self.current_workspace()
            .and_then(|w| w.teams.iter().find(|t| t.uid == team_uid))
    }

    pub fn register_window(
        &mut self,
        window_id: WindowId,
        team_uid: Option<ServerId>,
        ctx: &mut ModelContext<Self>,
    ) {
        self.window_team_uids.entry(window_id).or_insert(team_uid);
        ctx.notify();
    }
    pub fn inherited_or_default_team_uid(
        &self,
        source_window_id: Option<WindowId>,
    ) -> Option<ServerId> {
        source_window_id
            .and_then(|source_window_id| self.team_uid_for_window(source_window_id))
            .or_else(|| {
                self.current_workspace()
                    .and_then(|workspace| workspace.teams.first())
                    .map(|team| team.uid)
            })
    }

    pub fn set_team_for_window(
        &mut self,
        window_id: WindowId,
        team_uid: ServerId,
        ctx: &mut ModelContext<Self>,
    ) {
        let window_team_uid = self.window_team_uids.entry(window_id).or_default();
        if window_team_uid.is_none() {
            *window_team_uid = Some(team_uid);
            ctx.notify();
        }
    }

    pub fn team_uid_for_window(&self, window_id: WindowId) -> Option<ServerId> {
        self.window_team_uids.get(&window_id).copied().flatten()
    }

    pub fn team_for_window(&self, window_id: WindowId) -> Option<&Team> {
        self.team_uid_for_window(window_id)
            .and_then(|team_uid| self.team_from_uid(team_uid))
    }
    pub fn team_for_view<T: Entity>(&self, ctx: &ViewContext<T>) -> Option<&Team> {
        self.team_for_window(ctx.window_id())
    }

    pub fn team_for_view_handle<T: Entity>(
        &self,
        view_handle: &WeakViewHandle<T>,
        ctx: &AppContext,
    ) -> Option<&Team> {
        view_handle
            .window_id(ctx)
            .and_then(|window_id| self.team_for_window(window_id))
    }

    fn reconcile_window_team_assignments(&mut self) {
        let team_uids = self
            .current_workspace()
            .map(|workspace| {
                workspace
                    .teams
                    .iter()
                    .map(|team| team.uid)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let fallback_team_uid = team_uids.first().copied();

        for window_team_uid in self.window_team_uids.values_mut() {
            if window_team_uid.is_none_or(|team_uid| !team_uids.contains(&team_uid)) {
                *window_team_uid = fallback_team_uid;
            }
        }
    }

    pub fn team_from_uid_across_all_workspaces(&self, team_uid: ServerId) -> Option<&Team> {
        self.workspaces
            .iter()
            .flat_map(|w| w.teams.iter())
            .find(|t| t.uid == team_uid)
    }

    pub fn workspace_from_uid(&self, workspace_uid: WorkspaceUid) -> Option<&Workspace> {
        self.workspaces.iter().find(|w| w.uid == workspace_uid)
    }

    pub fn workspace_from_uid_mut(
        &mut self,
        workspace_uid: WorkspaceUid,
    ) -> Option<&mut Workspace> {
        self.workspaces.iter_mut().find(|w| w.uid == workspace_uid)
    }

    pub fn sole_team(&self) -> Option<&Team> {
        let [team] = self.current_workspace()?.teams.as_slice() else {
            return None;
        };
        Some(team)
    }

    pub fn sole_team_uid(&self) -> Option<ServerId> {
        self.sole_team().map(|team| team.uid)
    }

    /// Note that the workspace is populated with dummy data until the initial fetch
    /// completes (only workspace name/ID and workspace team's name/ID are cached in
    /// sqlite locally).
    /// Consider whether you need to wait for the results of the fetch before checking the
    /// values of other fields.
    pub fn current_workspace(&self) -> Option<&Workspace> {
        self.current_workspace_uid
            .and_then(|workspace_uid| self.workspace_from_uid(workspace_uid))
    }
    pub fn current_workspace_billing_metadata(&self) -> Option<&BillingMetadata> {
        self.current_workspace()
            .map(|workspace| &workspace.billing_metadata)
    }

    pub fn current_workspace_mut(&mut self) -> Option<&mut Workspace> {
        self.current_workspace_uid
            .and_then(|workspace_uid| self.workspace_from_uid_mut(workspace_uid))
    }

    pub fn workspaces(&self) -> &Vec<Workspace> {
        &self.workspaces
    }

    pub fn set_current_workspace_uid(
        &mut self,
        workspace_uid: WorkspaceUid,
        ctx: &mut ModelContext<Self>,
    ) {
        *self.current_workspace_uid = Some(workspace_uid);
        self.reconcile_window_team_assignments();
        self.notify_and_emit_teams_changed(ctx);
    }

    /// Returns `true` if active AI is allowed for the current workspace, based on billing config.
    ///
    /// In the future, we should store active AI enablement on the policy directly. For now, we
    /// proxy whether active AI by checking whether any active AI feature is enabled.
    pub fn is_active_ai_allowed(&self) -> bool {
        self.current_workspace().is_none_or(|workspace| {
            workspace
                .billing_metadata
                .tier
                .warp_ai_policy
                .is_none_or(|policy| {
                    policy.is_prompt_suggestions_toggleable
                        || policy.is_next_command_enabled
                        || policy.is_code_suggestions_toggleable
                        || policy.is_git_operations_ai_enabled
                })
        })
    }

    pub fn ai_allowed_for_team(team: Option<&Team>) -> bool {
        !team.is_some_and(|team| team.billing_metadata.customer_type == CustomerType::Enterprise)
            || team.is_some_and(|team| team.billing_metadata.is_warp_plan())
            || ChannelState::channel().is_dogfood()
    }

    /// Whether Prompt Suggestions should be toggleable for the current user, based on the active policies.
    /// Note that the value may be incorrect if called before the team's billing metadata has been fetched.
    pub fn is_prompt_suggestions_toggleable(&self) -> bool {
        self.current_workspace()
            // If the user has no team, they can toggle prompt suggestions (no restrictions).
            .is_none_or(|workspace| {
                workspace
                    .billing_metadata
                    .tier
                    .warp_ai_policy
                    .is_some_and(|policy| policy.is_prompt_suggestions_toggleable)
            })
    }

    /// Whether Code Suggestions should be toggleable for the current user, based on the active policies.
    /// Note that the value may be incorrect if called before the team's billing metadata has been fetched.
    pub fn is_code_suggestions_toggleable(&self) -> bool {
        self.current_workspace()
            // If the user has no team, they can toggle code suggestions (no restrictions).
            .is_none_or(|workspace| {
                workspace
                    .billing_metadata
                    .tier
                    .warp_ai_policy
                    .is_some_and(|policy| policy.is_code_suggestions_toggleable)
            })
    }

    /// Whether Next Command should be toggleable for the current user, based on the active policies.
    /// Note that the value may be incorrect if called before the team's billing metadata has been fetched.
    pub fn is_next_command_enabled(&self) -> bool {
        self.current_workspace()
            // If the user has no team, they can toggle Next Command (no restrictions).
            .is_none_or(|workspace| {
                workspace
                    .billing_metadata
                    .tier
                    .warp_ai_policy
                    .is_some_and(|policy| policy.is_next_command_enabled)
            })
    }

    /// Whether Git Operations AI is enabled for the current user, based on the active policies.
    /// Note that the value may be incorrect if called before the team's billing metadata has been fetched.
    pub fn is_git_operations_ai_enabled(&self) -> bool {
        self.current_workspace()
            // If the user has no team, they can toggle Git Operations AI (no restrictions).
            .is_none_or(|workspace| {
                workspace
                    .billing_metadata
                    .tier
                    .warp_ai_policy
                    .is_some_and(|policy| policy.is_git_operations_ai_enabled)
            })
    }

    /// Whether voice input should be toggleable for the current user, based on the active policies.
    /// Note that the value may be incorrect if called before the team's billing metadata has been fetched.
    /// If voice input support is not compiled into this build, always returns `false`.
    pub fn is_voice_enabled(&self) -> bool {
        cfg!(feature = "voice_input")
            && self
                .current_workspace()
                // If the user has no team, they can toggle Voice (no restrictions).
                .is_none_or(|workspace| {
                    workspace
                        .billing_metadata
                        .tier
                        .warp_ai_policy
                        .is_some_and(|policy| policy.is_voice_enabled)
                })
    }

    /// Whether BYO API key is enabled for the current user, based on the active policies.
    /// Note that the value may be incorrect if called before the team's billing metadata has been fetched.
    /// For solo users (no workspace), this is controlled by the `SoloUserByok` feature flag.
    /// Anonymous or logged-out users are not allowed to use BYO API keys.
    pub fn is_byo_api_key_enabled(&self, app: &AppContext) -> bool {
        if AuthStateProvider::as_ref(app)
            .get()
            .is_anonymous_or_logged_out()
        {
            return false;
        }
        self.current_workspace()
            .map(|workspace| workspace.is_byo_api_key_enabled())
            .unwrap_or(FeatureFlag::SoloUserByok.is_enabled())
    }

    /// Whether the current workspace's managed BYOK/BYOE policy allows members
    /// to use their own provider API keys. Users with no workspace, or
    /// workspaces without the managed BYOK/BYOE policy, have no team-level
    /// restriction, so this returns true and the normal BYO entitlement applies.
    pub fn are_member_byo_keys_allowed(&self) -> bool {
        self.current_workspace().is_none_or(|workspace| {
            !workspace.billing_metadata.is_managed_byok_byoe_enabled()
                || workspace
                    .settings
                    .team_byo
                    .as_ref()
                    .is_some_and(|team_byo| {
                        team_byo.first_party_enabled && team_byo.allow_user_keys
                    })
        })
    }
    /// Whether custom inference endpoints are enabled for the current user.
    /// Anonymous or logged-out users are not allowed to use custom inference.
    /// Controlled by the BYO_ENDPOINT billing policy.
    pub fn is_custom_inference_enabled(&self, app: &AppContext) -> bool {
        if AuthStateProvider::as_ref(app)
            .get()
            .is_anonymous_or_logged_out()
        {
            return false;
        }

        self.current_workspace()
            .map(|workspace| workspace.billing_metadata.is_byo_endpoint_enabled())
            .unwrap_or(true)
    }

    /// Whether the current workspace's managed BYOK/BYOE policy allows members
    /// to use their own custom endpoints. Users with no workspace, or
    /// workspaces without the managed BYOK/BYOE policy, have no team-level
    /// restriction, so this returns true and the normal BYO entitlement applies.
    pub fn are_member_byo_endpoints_allowed(&self) -> bool {
        self.current_workspace().is_none_or(|workspace| {
            !workspace.billing_metadata.is_managed_byok_byoe_enabled()
                || workspace
                    .settings
                    .team_byo
                    .as_ref()
                    .is_some_and(|team_byo| {
                        team_byo.endpoints_enabled && team_byo.allow_user_endpoints
                    })
        })
    }

    /// LOCAL FORK: per-host LLM configuration was keyed by the agent's
    /// `LLMModelHost` and is no longer carried on the workspace, so no host is
    /// ever configured.
    pub fn aws_bedrock_host_settings(&self) -> Option<&super::workspace::LlmHostSettings> {
        None
    }

    /// Did the admin enable AWS Bedrock for the current workspace?
    pub fn is_aws_bedrock_available_from_workspace(&self) -> bool {
        self.current_workspace().is_some_and(|workspace| {
            workspace.settings.llm_settings.enabled
                && self
                    .aws_bedrock_host_settings()
                    .is_some_and(|settings| settings.enabled)
        })
    }
    pub fn aws_bedrock_host_enablement_setting(&self) -> HostEnablementSetting {
        self.aws_bedrock_host_settings()
            .map(|settings| settings.enablement_setting.clone())
            .unwrap_or_default()
    }

    pub fn is_aws_bedrock_credentials_toggleable(&self) -> bool {
        matches!(
            self.aws_bedrock_host_enablement_setting(),
            HostEnablementSetting::RespectUserSetting
        )
    }

    pub fn is_aws_bedrock_credentials_enabled(&self, app: &AppContext) -> bool {
        // i.e. did the admin go and toggle on aws bedrock in the admin panel?
        if !self.is_aws_bedrock_available_from_workspace() {
            return false;
        }

        match self.aws_bedrock_host_enablement_setting() {
            HostEnablementSetting::Enforce => true,
            HostEnablementSetting::RespectUserSetting => *AISettings::as_ref(app)
                .aws_bedrock_credentials_enabled
                .value(),
        }
    }

    /// LOCAL FORK: see [`Self::aws_bedrock_host_settings`].
    pub fn gemini_enterprise_host_settings(&self) -> Option<&super::workspace::LlmHostSettings> {
        None
    }

    /// Did the admin enable Gemini Enterprise (GEAP) for the current workspace?
    pub fn is_gemini_enterprise_available_from_workspace(&self) -> bool {
        self.current_workspace().is_some_and(|workspace| {
            workspace.settings.llm_settings.enabled
                && self
                    .gemini_enterprise_host_settings()
                    .is_some_and(|settings| settings.enabled)
        })
    }

    pub fn gemini_enterprise_host_enablement_setting(&self) -> HostEnablementSetting {
        self.gemini_enterprise_host_settings()
            .map(|settings| settings.enablement_setting.clone())
            .unwrap_or_default()
    }

    pub fn is_gemini_enterprise_credentials_toggleable(&self) -> bool {
        matches!(
            self.gemini_enterprise_host_enablement_setting(),
            HostEnablementSetting::RespectUserSetting
        )
    }

    /// Whether Gemini Enterprise (GEAP) credentials should be minted and attached for the
    /// current user. Anonymous/logged-out guard from [`Self::is_byo_api_key_enabled`]:
    /// a GEAP credential mint is rooted in the user's Warp session, so without one
    /// there is nothing to mint from.
    pub fn is_gemini_enterprise_credentials_enabled(&self, app: &AppContext) -> bool {
        if !FeatureFlag::GeminiEnterprise.is_enabled() {
            return false;
        }
        if AuthStateProvider::as_ref(app)
            .get()
            .is_anonymous_or_logged_out()
        {
            return false;
        }
        // i.e. did the admin toggle on Gemini Enterprise in the admin panel?
        if !self.is_gemini_enterprise_available_from_workspace() {
            return false;
        }

        match self.gemini_enterprise_host_enablement_setting() {
            HostEnablementSetting::Enforce => true,
            HostEnablementSetting::RespectUserSetting => *AISettings::as_ref(app)
                .gemini_enterprise_credentials_enabled
                .value(),
        }
    }

    /// Returns the AI autonomy settings that are enforced by the workspace for all its members.
    /// If a setting is `None`, the workspace doesn't enforce a particular setting.
    pub fn ai_autonomy_settings(&self) -> AiAutonomySettings {
        self.current_workspace()
            .map(|workspace| workspace.settings.ai_autonomy_settings.clone())
            .unwrap_or_default()
    }

    /// Returns the sandboxed agent settings enforced by the workspace, if any.
    pub fn sandboxed_agent_settings(&self) -> Option<SandboxedAgentSettings> {
        self.current_workspace()
            .and_then(|workspace| workspace.settings.sandboxed_agent_settings.clone())
    }

    /// Returns true iff AI autonomy features are allowed for this client.
    /// TODO: This should be deleted soon. AI autonomy settings have been moved into organization
    /// settings (see `ai_autonomy_settings` above), but there could be an interim time where we
    /// have not set up the org settings yet for an enterprise that previously had the entire
    /// feature set disabled. To capture that case, we'll see if all the settings are `None`;
    /// if so, we'll fall back to their billing metadata's value. Once we've migrated everyone
    /// into org settings, we should remove `is_enabled` from the policy and delete this function.
    pub fn is_ai_autonomy_allowed(&self) -> bool {
        self.current_workspace().is_none_or(|workspace| {
            let settings = &workspace.settings.ai_autonomy_settings;
            // LOCAL FORK: the per-capability autonomy settings (apply code diffs,
            // read files, execute commands) went with the agent; only the
            // allow/deny lists remain on `AiAutonomySettings`.
            let all_settings_none = settings.read_files_allowlist.is_none()
                && settings.execute_commands_allowlist.is_none()
                && settings.execute_commands_denylist.is_none();

            if all_settings_none {
                workspace
                    .billing_metadata
                    .tier
                    .ai_autonomy_policy
                    .is_some_and(|policy| policy.is_enabled)
            } else {
                true
            }
        })
    }

    // Returns a Vec of the user's active spaces, based on their
    // team membership.
    pub fn team_spaces(&self) -> Vec<Space> {
        if let Some(workspace) = self.current_workspace() {
            workspace
                .teams
                .iter()
                .map(|team| Space::Team { team_uid: team.uid })
                .collect()
        } else {
            // If the user has no workspace, they have no team spaces.
            vec![]
        }
    }

    pub fn spaces_for_window(&self, window_id: WindowId, ctx: &AppContext) -> Vec<Space> {
        if AuthStateProvider::as_ref(ctx)
            .get()
            .is_user_web_anonymous_user()
            .unwrap_or_default()
        {
            return vec![Space::Shared];
        }
        let mut spaces = vec![];
        if let Some(team) = self.team_for_window(window_id) {
            spaces.push(Space::Team { team_uid: team.uid });
        }

        if FeatureFlag::SharedWithMe.is_enabled()
            && CloudModel::as_ref(ctx).has_directly_shared_objects(self, ctx)
        {
            spaces.push(Space::Shared);
        }
        spaces.push(Space::Personal);

        spaces
    }

    // Returns the [`Owner`] for the user's personal drive. If the user is not authenticated, this
    // returns `None`.
    pub fn personal_drive(&self, ctx: &AppContext) -> Option<Owner> {
        AuthStateProvider::as_ref(ctx)
            .get()
            .user_id()
            .map(|user_uid| Owner::User { user_uid })
    }

    // Maps a [`Space`] into an [`Owner`], based on the user's team memberships. If the space
    // does not directly identify an owner (it's the space for shared objects), returns `None`.
    pub fn space_to_owner(&self, space: Space, ctx: &AppContext) -> Option<Owner> {
        match space {
            Space::Team { team_uid } => Some(Owner::Team { team_uid }),
            Space::Personal => self.personal_drive(ctx),
            Space::Shared => None,
        }
    }

    // Maps an [`Owner`] into a [`Space`], based on the user's team memberships.
    // This is always possible, as unknown owners imply the shared space.
    pub fn owner_to_space(&self, owner: Owner, ctx: &AppContext) -> Space {
        match owner {
            Owner::User { user_uid } => {
                if !FeatureFlag::SharedWithMe.is_enabled() {
                    return Space::Personal;
                }

                let current_user = AuthStateProvider::as_ref(ctx).get().user_id();
                if Some(user_uid) == current_user {
                    Space::Personal
                } else {
                    Space::Shared
                }
            }
            Owner::Team { team_uid } => {
                if !FeatureFlag::SharedWithMe.is_enabled()
                    || self.team_from_uid_across_all_workspaces(team_uid).is_some()
                {
                    Space::Team { team_uid }
                } else {
                    Space::Shared
                }
            }
        }
    }

    pub fn has_teams(&self) -> bool {
        if let Some(workspace) = self.current_workspace() {
            !workspace.teams.is_empty()
        } else {
            false
        }
    }

    pub fn has_workspaces(&self) -> bool {
        !self.workspaces.is_empty()
    }

    pub fn update_workspaces(&mut self, workspaces: Vec<Workspace>, ctx: &mut ModelContext<Self>) {
        // Check if sunsetted_to_build_ts changed for any workspace
        let sunsetted_to_build_changed = self.has_sunsetted_to_build_data_changed(&workspaces);

        *self.workspaces = workspaces;
        self.reconcile_window_team_assignments();
        self.notify_and_emit_teams_changed(ctx);

        if sunsetted_to_build_changed {
            ctx.emit(UserWorkspacesEvent::SunsettedToBuildDataUpdated);
        }
    }

    /// Checks if any workspace's service agreement sunsetted_to_build_ts field has changed.
    fn has_sunsetted_to_build_data_changed(&self, new_workspaces: &[Workspace]) -> bool {
        for new_workspace in new_workspaces {
            // Find the corresponding old workspace
            let old_workspace = self.workspaces.iter().find(|w| w.uid == new_workspace.uid);

            if let Some(old_workspace) = old_workspace {
                // Check if any team's service agreement sunsetted_to_build_ts changed
                for new_team in &new_workspace.teams {
                    let old_team = old_workspace.teams.iter().find(|t| t.uid == new_team.uid);

                    if let Some(old_team) = old_team {
                        let old_sunsetted = old_team
                            .billing_metadata
                            .service_agreements
                            .first()
                            .and_then(|sa| sa.sunsetted_to_build_ts);

                        let new_sunsetted = new_team
                            .billing_metadata
                            .service_agreements
                            .first()
                            .and_then(|sa| sa.sunsetted_to_build_ts);

                        // Detect if it changed from None to Some or changed value
                        if old_sunsetted != new_sunsetted {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }

    fn notify_and_emit_teams_changed(&self, ctx: &mut ModelContext<Self>) {
        // PrivacySettings can't observe UserWorkspaces for updates, as it's initialized too early in
        // the app initialization flow. So, we update it manually whenever teams data changes.
        PrivacySettings::handle(ctx).update(ctx, |settings, ctx| {
            settings.set_is_telemetry_force_enabled(self.is_telemetry_force_enabled());
            settings.set_enterprise_secret_redaction_settings(
                self.is_enterprise_secret_redaction_enabled(),
                self.get_enterprise_secret_redaction_regex_list(),
                ChangeEventReason::CloudSync,
                ctx,
            );
        });

        ctx.emit(UserWorkspacesEvent::TeamsChanged);
        ctx.emit(UserWorkspacesEvent::CodebaseContextEnablementChanged);
        ctx.notify();
    }

    pub fn team_created(
        &mut self,
        create_team_response: &CreateTeamResponse,
        ctx: &mut ModelContext<Self>,
    ) {
        self.workspaces.push(create_team_response.workspace.clone());
        self.set_current_workspace_uid(create_team_response.workspace.uid, ctx);
        self.notify_and_emit_teams_changed(ctx);
    }

    pub fn usage_based_pricing_settings(&self) -> UsageBasedPricingSettings {
        self.current_workspace()
            .map(|workspace| workspace.settings.usage_based_pricing_settings.clone())
            .unwrap_or_default()
    }

    pub fn is_telemetry_force_enabled(&self) -> bool {
        self.current_workspace()
            .map(|workspace| workspace.settings.telemetry_settings.force_enabled)
            .unwrap_or(false)
    }

    pub fn is_enterprise_secret_redaction_enabled(&self) -> bool {
        self.current_workspace()
            .map(|workspace| workspace.settings.secret_redaction_settings.enabled)
            .unwrap_or(false)
    }

    pub fn get_enterprise_secret_redaction_regex_list(&self) -> Vec<EnterpriseSecretRegex> {
        self.current_workspace()
            .map(|workspace| workspace.settings.secret_redaction_settings.regexes.clone())
            .unwrap_or_default()
    }

    pub fn get_ugc_collection_enablement_setting(&self) -> UgcCollectionEnablementSetting {
        self.current_workspace()
            .map(|workspace| workspace.settings.ugc_collection_settings.setting.clone())
            .unwrap_or_default()
    }

    pub fn get_cloud_conversation_storage_enablement_setting(&self) -> AdminEnablementSetting {
        self.current_workspace()
            .map(|workspace| {
                workspace
                    .settings
                    .cloud_conversation_storage_settings
                    .setting
                    .clone()
            })
            .unwrap_or_default()
    }

    pub fn is_ai_allowed_in_remote_sessions(&self) -> bool {
        self.current_workspace()
            .map(|workspace| {
                workspace
                    .settings
                    .ai_permissions_settings
                    .allow_ai_in_remote_sessions
            })
            .unwrap_or(true)
    }

    pub fn get_remote_session_regex_list(&self) -> Vec<Regex> {
        self.current_workspace()
            .map(|workspace| {
                workspace
                    .settings
                    .ai_permissions_settings
                    .remote_session_regex_list
                    .clone()
            })
            .unwrap_or_default()
    }

    pub fn is_anyone_with_link_sharing_enabled(&self) -> bool {
        self.current_workspace()
            .map(|workspace| {
                workspace
                    .settings
                    .link_sharing_settings
                    .anyone_with_link_sharing_enabled
            })
            .unwrap_or(true)
    }

    pub fn is_direct_link_sharing_enabled(&self) -> bool {
        self.current_workspace()
            .map(|workspace| {
                workspace
                    .settings
                    .link_sharing_settings
                    .direct_link_sharing_enabled
            })
            .unwrap_or(true)
    }

    /// Returns the codebase context settings, taking into account the organization,
    /// global AI settings, and codebase-specific settings.
    /// Prefer this function to determine whether to show indexing-related functionality.
    pub fn is_codebase_context_enabled(&self, app: &AppContext) -> bool {
        // If the organization has an explicit setting, respect it and make user toggle irrelevant.
        // - Enable: forced ON by org, regardless of user preference.
        // - Disable: forced OFF by org.
        // - RespectUserSetting: respect the user setting.
        let org_setting = self.team_allows_codebase_context();
        let ai_globally_enabled = AISettings::as_ref(app).is_any_ai_enabled(app);

        match org_setting {
            AdminEnablementSetting::Enable => ai_globally_enabled,
            AdminEnablementSetting::Disable => false,
            AdminEnablementSetting::RespectUserSetting => {
                ai_globally_enabled && *CodeSettings::as_ref(app).codebase_context_enabled.value()
            }
        }
    }

    pub fn default_host_slug(&self) -> Option<&str> {
        self.current_workspace()
            .and_then(|workspace| workspace.settings.default_host_slug.as_deref())
    }

    /// Returns the team-level agent attribution setting.
    ///
    /// Use this to decide whether the user's attribution toggle should be locked
    /// (`Enable`/`Disable`) or editable (`RespectUserSetting`).
    pub fn get_agent_attribution_setting(&self) -> AdminEnablementSetting {
        self.current_workspace()
            .map(|workspace| workspace.settings.enable_warp_attribution.clone())
            .unwrap_or_default()
    }

    /// Returns only the organization-specific codebase context enablement setting.
    /// Do not use this function to determine whether codebase context is generally enabled --
    /// use `is_codebase_context_enabled` instead.
    pub fn team_allows_codebase_context(&self) -> AdminEnablementSetting {
        self.current_workspace()
            .map(|workspace| workspace.settings.codebase_context_settings.setting.clone())
            .unwrap_or_default()
    }
}

#[cfg(test)]
impl UserWorkspaces {
    /// Creates a test workspace with a team and sets it as the current workspace.
    /// Returns the workspace UID and admin UID for use in tests.
    pub fn setup_test_workspace(&mut self, ctx: &mut ModelContext<Self>) {
        let workspace_uid = WorkspaceUid::from(ServerId::from(1));
        let owner_uid = UserUid::new("test_owner");

        let workspace_settings = WorkspaceSettings::default();

        let workspace = Workspace {
            uid: workspace_uid,
            name: "Test Workspace".to_string(),
            stripe_customer_id: None,
            teams: vec![Team {
                uid: ServerId::from(2),
                name: "Test Team".to_string(),
                organization_settings: workspace_settings.clone(),
                billing_metadata: BillingMetadata::default(),
                members: vec![],
                invite_code: None,
                pending_email_invites: vec![],
                invite_link_domain_restrictions: vec![],
                stripe_customer_id: None,
                is_eligible_for_discovery: false,
                has_billing_history: false,
            }],
            members: vec![WorkspaceMember {
                uid: owner_uid,
                email: "test@example.com".to_string(),
                role: MembershipRole::Owner,
                usage_info: WorkspaceMemberUsageInfo {
                    requests_used_since_last_refresh: 0,
                    request_limit: 1000,
                    is_unlimited: false,
                    is_request_limit_prorated: false,
                },
            }],
            billing_metadata: BillingMetadata::default(),
            bonus_grants_purchased_this_month: Default::default(),
            billing_cycle_usage: None,
            has_billing_history: false,
            settings: workspace_settings,
            invite_code: None,
            invite_link_domain_restrictions: vec![],
            pending_email_invites: vec![],
            is_eligible_for_discovery: false,
            total_requests_used_since_last_refresh: 0,
        };

        self.update_workspaces(vec![workspace], ctx);
        self.set_current_workspace_uid(workspace_uid, ctx);
    }

    /// Updates the current workspace by applying a mutation function.
    pub fn update_current_workspace<F>(&mut self, f: F, ctx: &mut ModelContext<Self>)
    where
        F: FnOnce(&mut Workspace),
    {
        if let Some(workspace) = self.current_workspace() {
            if workspace.teams.is_empty() {
                panic!("No team found in current workspace. Did you call setup_test_workspace()?");
            }

            let mut new_workspace = workspace.clone();
            f(&mut new_workspace);

            self.update_workspaces(vec![new_workspace], ctx);
        } else {
            panic!("No workspace found. Did you call setup_test_workspace()?");
        }
    }

    pub fn update_sandboxed_agent_settings<F>(&mut self, f: F, ctx: &mut ModelContext<Self>)
    where
        F: FnOnce(&mut Option<SandboxedAgentSettings>),
    {
        self.update_current_workspace(
            |workspace| {
                f(&mut workspace.settings.sandboxed_agent_settings);
            },
            ctx,
        );
    }

    pub fn update_ai_autonomy_settings<F>(&mut self, f: F, ctx: &mut ModelContext<Self>)
    where
        F: FnOnce(&mut AiAutonomySettings),
    {
        self.update_current_workspace(
            |workspace| {
                f(&mut workspace.settings.ai_autonomy_settings);
            },
            ctx,
        );
    }

    pub fn update_ai_autonomy_policy_flag(&mut self, enabled: bool, ctx: &mut ModelContext<Self>) {
        self.update_current_workspace(
            |workspace| {
                if let Some(team) = workspace.teams.first_mut() {
                    team.billing_metadata.tier.ai_autonomy_policy = Some(AIAutonomyPolicy {
                        is_enabled: enabled,
                        toggleable: true,
                    });
                } else {
                    panic!(
                        "No team found in current workspace. Did you call setup_test_workspace()?"
                    );
                }
            },
            ctx,
        );
    }
}

impl Entity for UserWorkspaces {
    type Event = UserWorkspacesEvent;
}

/// Mark UserWorkspaces as global application state.
impl SingletonEntity for UserWorkspaces {}

#[cfg(test)]
#[path = "user_workspaces_tests.rs"]
mod user_workspaces_tests;
