use std::collections::HashMap;
use std::collections::hash_map::Entry;

use warpui::{Entity, EntityId, ModelContext, SingletonEntity};

use super::CloudWorkflowModel;
use super::workflow::Workflow;
use crate::cloud_object::OpenWarpDriveObjectSettings;
use crate::cloud_object::model::persistence::CloudModel;
use crate::cloud_object::{GenericCloudObject, Owner};
use crate::pane_group::{PaneContent, WorkflowPane};
use crate::server::ids::{ClientId, SyncId};
use crate::workflows::WorkflowViewMode;
use crate::workflows::workflow_view::WorkflowView;
use crate::{PaneViewLocator, WindowId, safe_warn};

pub struct WorkflowManager {
    panes_by_hashed_id: HashMap<String, WorkflowPaneData>,
}

#[derive(Debug, Clone)]
pub enum WorkflowOpenSource {
    Existing(SyncId),
    New {
        title: Option<String>,

        /// The "content" of the workflow.
        /// For `Command` workflows, this is the command.
        /// For `AgentMode` workflows, this is the AI query.
        content: Option<String>,

        owner: Owner,
        initial_folder_id: Option<SyncId>,
        is_for_agent_mode: bool,
    },
    NewFromWorkflow {
        workflow: Box<Workflow>,
        owner: Owner,
        initial_folder_id: Option<SyncId>,
    },
}

impl WorkflowManager {
    pub fn new(_ctx: &mut ModelContext<Self>) -> Self {
        // LOCAL FORK: this model no longer listens to the UpdateManager. Its handler
        // existed for one thing: when the server answered a create with the id it had
        // minted, the pane keyed under the object's client id had to be rekeyed under the
        // new server id. Objects keep their client id for life now, so there is nothing to
        // rekey and no `ObjectOperation::Create` result to hear.

        WorkflowManager {
            panes_by_hashed_id: HashMap::new(),
        }
    }

    pub fn find_pane(&self, source: &WorkflowOpenSource) -> Option<(WindowId, PaneViewLocator)> {
        match source {
            WorkflowOpenSource::Existing(workflow_id) => {
                let pane_data = self.panes_by_hashed_id.get(&workflow_id.uid())?;
                Some((pane_data.window_id, pane_data.locator))
            }
            WorkflowOpenSource::New { .. } | WorkflowOpenSource::NewFromWorkflow { .. } => None,
        }
    }

    pub fn create_pane(
        &mut self,
        source: &WorkflowOpenSource,
        settings: &OpenWarpDriveObjectSettings,
        mode: WorkflowViewMode,
        window_id: WindowId,
        ctx: &mut ModelContext<Self>,
    ) -> WorkflowPane {
        let view = ctx.add_typed_action_view(window_id, WorkflowView::new_in_pane);

        match source {
            WorkflowOpenSource::Existing(workflow_id) => {
                let workflow = CloudModel::as_ref(ctx).get_workflow(workflow_id).cloned();
                if let Some(workflow) = workflow {
                    view.update(ctx, |view, ctx| view.load(workflow, settings, mode, ctx));
                } else {
                    // LOCAL FORK: the fallback waited for the initial cloud load and then
                    // fetched the object from the server. A locally persisted object is
                    // already in `CloudModel` above; anything missing here does not exist.
                }
            }
            WorkflowOpenSource::New {
                title,
                content,
                owner,
                initial_folder_id,
                is_for_agent_mode,
            } => view.update(ctx, |view, ctx| {
                view.open_new_workflow(
                    title.clone(),
                    content.clone(),
                    *owner,
                    *initial_folder_id,
                    *is_for_agent_mode,
                    SyncId::ClientId(ClientId::default()),
                    ctx,
                )
            }),
            WorkflowOpenSource::NewFromWorkflow {
                workflow,
                owner,
                initial_folder_id,
            } => {
                view.update(ctx, |view, ctx| {
                    view.load(
                        GenericCloudObject::new_local(
                            CloudWorkflowModel::new(*workflow.clone()),
                            *owner,
                            *initial_folder_id,
                            ClientId::default(),
                        ),
                        &OpenWarpDriveObjectSettings::default(),
                        mode,
                        ctx,
                    );
                });
            }
        }

        WorkflowPane::new(view, ctx)
    }

    pub fn register_pane(
        &mut self,
        pane: &WorkflowPane,
        pane_group_id: EntityId,
        window_id: WindowId,
        ctx: &mut ModelContext<Self>,
    ) {
        let workflow_id = pane.get_view(ctx).as_ref(ctx).workflow_id();
        let entry = self.panes_by_hashed_id.entry(workflow_id.uid());
        if let Entry::Vacant(entry) = entry {
            entry.insert(WorkflowPaneData {
                workflow_id,
                window_id,
                locator: PaneViewLocator {
                    pane_group_id,
                    pane_id: pane.id(),
                },
            });
        } else {
            safe_warn!(
                safe: ("Ignoring duplicate Workflow pane registration"),
                full: ("Ignoring duplicate Workflow pane registration for {workflow_id}")
            );
        }
    }

    pub fn deregister_pane(&mut self, pane: &WorkflowPane, ctx: &mut ModelContext<Self>) {
        let workflow_id = pane.get_view(ctx).as_ref(ctx).workflow_id();

        // If a workflow pane is restored, the workflow may have been reopened in the meantime. In
        // that case, don't let closing the original pane clear out the new pane.
        if let Entry::Occupied(entry) = self.panes_by_hashed_id.entry(workflow_id.uid()) {
            if entry.get().locator.pane_id == pane.id() {
                entry.remove();
            } else {
                log::warn!(
                    "Ignoring duplicate registration of panes for {}",
                    workflow_id.uid()
                );
            }
        }
    }

    pub fn reset(&mut self) {
        self.panes_by_hashed_id.clear();
    }
}

struct WorkflowPaneData {
    workflow_id: SyncId,
    window_id: WindowId,
    locator: PaneViewLocator,
}

impl Entity for WorkflowManager {
    type Event = ();
}

impl SingletonEntity for WorkflowManager {}
