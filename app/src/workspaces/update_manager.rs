use std::sync::mpsc::SyncSender;

use anyhow::Context;
use warp_errors::report_if_error;
use warpui::{Entity, ModelContext, SingletonEntity};

use super::user_workspaces::UserWorkspaces;
use super::workspace::WorkspaceUid;
use crate::persistence::ModelEvent;

pub enum TeamUpdateManagerEvent {
    // LOCAL FORK: the leave-team and rename-team events went with team administration.
}

/// TeamUpdateManager is a singleton model responsible for keeping the local database in
/// step with the workspace metadata models.
///
/// LOCAL FORK: it no longer communicates with a server, and no longer holds a `TeamClient`.
///
/// It used to poll `get_workspaces_metadata_for_user` on a timer for the user's teams,
/// their policies and the active experiment arms, restarting the poll whenever the network
/// came back or the team-tester flag changed, with an out-of-band refresh the settings
/// pages triggered on open. Both entry points already returned early on `!is_logged_in()`,
/// so no request ever went out and no response handler ever ran.
///
/// `UserWorkspaces` and `Workspace` stay. They load from sqlite at startup, so a user who
/// belonged to a team before this fork still has that workspace and its settings; removing
/// the models would take away data they already have. Only the fetch that could change
/// them is gone.
pub struct TeamUpdateManager {
    model_event_sender: Option<SyncSender<ModelEvent>>,
}

impl TeamUpdateManager {
    pub fn new(
        model_event_sender: Option<SyncSender<ModelEvent>>,
        _ctx: &mut ModelContext<Self>,
    ) -> Self {
        Self { model_event_sender }
    }

    #[cfg(test)]
    pub fn mock(ctx: &mut ModelContext<Self>) -> Self {
        Self::new(Default::default(), ctx)
    }

    pub fn set_current_workspace_uid(
        &mut self,
        workspace_uid: WorkspaceUid,
        ctx: &mut ModelContext<Self>,
    ) {
        UserWorkspaces::handle(ctx).update(ctx, |user_workspaces, ctx| {
            user_workspaces.set_current_workspace_uid(workspace_uid, ctx);
        });

        // Update sqlite
        self.save_to_db([ModelEvent::SetCurrentWorkspace { workspace_uid }]);
    }

    fn save_to_db(&self, events: impl IntoIterator<Item = ModelEvent>) {
        let model_event_sender = self.model_event_sender.clone();
        if let Some(model_event_sender) = &model_event_sender {
            for event in events {
                report_if_error!(
                    model_event_sender
                        .send(event)
                        .context("Error saving to database")
                );
            }
        }
    }
}

impl Entity for TeamUpdateManager {
    type Event = TeamUpdateManagerEvent;
}

impl SingletonEntity for TeamUpdateManager {}

// LOCAL FORK: the test module went with the fetch. Its one test,
// `test_leaving_team_removes_objects`, had already been removed with team administration,
// leaving a file of helpers that built a `MockTeamClient` for a manager that no longer
// takes one.
