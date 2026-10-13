//! A terminal pane restored from the previous session whose terminal hasn't been built yet.
//!
//! Restoring a window used to build every tab's terminals before the first frame: each one
//! parsed its saved blocks and started a shell, so a window with many tabs took seconds to
//! appear and its shells all started at once. Restored tabs now start out with these
//! placeholders, and [`PaneGroup::materialize_deferred_panes`] swaps in the real terminals
//! when the tab is first activated (or soon after startup, in the background).
//!
//! A placeholder keeps the snapshot it was restored from, so a session saved before its
//! terminal is built restores exactly as it was.
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::SyncSender;

use pathfinder_geometry::vector::Vector2F;
use warp_core::execution_mode::AppExecutionMode;
use warp_errors::report_error;
use warp_util::path::user_friendly_path;
use warpui::{AppContext, ModelHandle, SingletonEntity, ViewContext};

use super::{
    DetachType, PaneConfiguration, PaneContent, PaneGroup, PaneId, ShareableLink,
    ShareableLinkError,
};
use crate::app_state::{LeafContents, PaneUuid, TerminalPaneSnapshot};
use crate::pane_group::focus_state::PaneFocusHandle;
use crate::persistence::ModelEvent;
use crate::terminal::model::block::SerializedBlock;

pub struct DeferredTerminalPane {
    pane_id: PaneId,
    snapshot: TerminalPaneSnapshot,
    /// The saved blocks of every pane restored with this one. This pane's are looked up by
    /// its uuid when its terminal is built.
    block_lists: Arc<HashMap<PaneUuid, Vec<SerializedBlock>>>,
    /// The size estimated for the pane at restore, which its terminal starts out with.
    view_size: Vector2F,
    pane_configuration: ModelHandle<PaneConfiguration>,
    model_event_sender: Option<SyncSender<ModelEvent>>,
}

impl DeferredTerminalPane {
    pub(in crate::pane_group) fn new(
        snapshot: TerminalPaneSnapshot,
        block_lists: Arc<HashMap<PaneUuid, Vec<SerializedBlock>>>,
        view_size: Vector2F,
        model_event_sender: Option<SyncSender<ModelEvent>>,
        ctx: &mut ViewContext<PaneGroup>,
    ) -> Self {
        // Shown until the terminal is built: where the session was.
        let title = snapshot
            .cwd
            .as_deref()
            .map(|cwd| {
                let home = dirs::home_dir().map(|home| home.display().to_string());
                user_friendly_path(cwd, home.as_deref()).into_owned()
            })
            .unwrap_or_default();
        Self {
            pane_id: PaneId::deferred_placeholder_pane_id(),
            snapshot,
            block_lists,
            view_size,
            pane_configuration: ctx.add_model(|_| PaneConfiguration::new(title)),
            model_event_sender,
        }
    }

    pub(in crate::pane_group) fn snapshot(&self) -> &TerminalPaneSnapshot {
        &self.snapshot
    }

    /// This pane's saved blocks, if it had any.
    pub(in crate::pane_group) fn restored_blocks(&self) -> Option<&[SerializedBlock]> {
        self.block_lists
            .get(&PaneUuid(self.snapshot.uuid.clone()))
            .map(Vec::as_slice)
    }

    pub(in crate::pane_group) fn view_size(&self) -> Vector2F {
        self.view_size
    }
}

impl PaneContent for DeferredTerminalPane {
    fn id(&self) -> PaneId {
        self.pane_id
    }

    fn attach(
        &self,
        _group: &PaneGroup,
        _focus_handle: PaneFocusHandle,
        _ctx: &mut ViewContext<PaneGroup>,
    ) {
    }

    fn detach(
        &self,
        _group: &PaneGroup,
        detach_type: DetachType,
        ctx: &mut ViewContext<PaneGroup>,
    ) {
        // Closed for good before it was ever built: its saved blocks go, as a terminal
        // pane's would.
        if !matches!(detach_type, DetachType::Closed)
            || !AppExecutionMode::as_ref(ctx).can_save_session()
        {
            return;
        }
        if let Some(sender) = &self.model_event_sender
            && let Err(err) = sender.send(ModelEvent::DeleteBlocks(self.snapshot.uuid.clone()))
        {
            report_error!(anyhow::Error::new(err).context("Error sending blocks deleted event"));
        }
    }

    fn snapshot(&self, _app: &AppContext) -> LeafContents {
        LeafContents::Terminal(self.snapshot.clone())
    }

    fn has_application_focus(&self, _ctx: &mut ViewContext<PaneGroup>) -> bool {
        false
    }

    fn focus(&self, _ctx: &mut ViewContext<PaneGroup>) {}

    fn shareable_link(
        &self,
        _ctx: &mut ViewContext<PaneGroup>,
    ) -> Result<ShareableLink, ShareableLinkError> {
        Ok(ShareableLink::Base)
    }

    fn pane_configuration(&self) -> ModelHandle<PaneConfiguration> {
        self.pane_configuration.clone()
    }

    fn is_pane_being_dragged(&self, _app: &AppContext) -> bool {
        false
    }
}
