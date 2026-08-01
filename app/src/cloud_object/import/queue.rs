use std::collections::{HashMap, HashSet};

use warp_errors::report_error;
use warpui::{Entity, ModelContext, SingletonEntity};

use super::nodes::{self, FileId};
use crate::cloud_object::{CloudObjectEventEntrypoint, Owner};
use crate::notebooks::CloudNotebookModel;
use crate::server::cloud_objects::update_manager::{InitiatedBy, UpdateManager};
use crate::server::ids::{ClientId, SyncId};
use crate::workflows::workflow::Workflow;
use crate::workflows::workflow_enum::WorkflowEnum;

pub(super) enum ImportQueueEvent {
    FileCompleted {
        file_id: FileId,
        server_id: Option<String>,
    },
    FolderCompleted {
        folder_id: nodes::FolderId,
        server_id: Option<String>,
    },
    FileSavedLocally(FileId),
}

#[derive(Debug)]
pub(super) enum ParentId {
    FolderToUpload(ClientId),
    InitialFolder(Option<SyncId>),
}

#[derive(Debug)]
pub(super) struct ImportQueueArgs {
    pub(super) owner: Owner,
    pub(super) parent_id: ParentId,
    pub(super) content: RequestContent,
}

#[derive(Debug)]
pub(super) enum RequestContent {
    Folder {
        name: String,
        client_id: ClientId,
        folder_id: nodes::FolderId,
    },
    Notebook {
        title: String,
        data: String,
        client_id: ClientId,
        file_id: FileId,
    },
    Workflow {
        workflows: Vec<(Workflow, ClientId)>,
        workflow_enums: HashMap<ClientId, WorkflowEnum>,
        file_id: FileId,
    },
}

#[derive(Default)]
struct FileCompletionCounter {
    client_id_to_file_id: HashMap<ClientId, FileId>,
    file_id_to_counter: HashMap<FileId, usize>,
}

impl FileCompletionCounter {
    fn request_completed(&mut self, client_id: ClientId) -> Option<FileId> {
        if let Some(file_id) = self.client_id_to_file_id.get(&client_id) {
            let completed = match self.file_id_to_counter.get_mut(file_id) {
                Some(counter) => {
                    *counter = counter.saturating_sub(1);
                    *counter == 0
                }
                None => {
                    report_error!("File completion counter should exist but it doesn't");
                    false
                }
            };

            if completed {
                return Some(*file_id);
            }
        }
        None
    }

    fn add_entry(&mut self, client_id: ClientId, file_id: FileId) {
        self.client_id_to_file_id.insert(client_id, file_id);
        *self.file_id_to_counter.entry(file_id).or_insert(0) += 1;
    }
}

pub(super) struct ImportQueue {
    queue: Vec<ImportQueueArgs>,
    /// LOCAL FORK: was `client_to_server_id: HashMap<ClientId, Option<FolderId>>`, the
    /// table that remembered which uploaded folders had come back from the server with an
    /// id yet. A folder is created the instant it is dequeued now, so all this has to
    /// record is that it happened.
    created_folders: HashSet<ClientId>,
    client_to_node_folder_id: HashMap<ClientId, nodes::FolderId>,
    file_completion: FileCompletionCounter,
}

impl ImportQueue {
    /// LOCAL FORK: no longer subscribes to the `UpdateManager`.
    ///
    /// Progress used to arrive as `ObjectOperation::Create` results, one per object, and
    /// the handler translated them into the per-file and per-folder completion events this
    /// model emits. Creation is synchronous now, so `dequeue` emits them itself.
    pub fn new(_ctx: &mut ModelContext<Self>) -> Self {
        Self {
            queue: Vec::new(),
            created_folders: HashSet::default(),
            file_completion: Default::default(),
            client_to_node_folder_id: HashMap::default(),
        }
    }

    // Whether all dependencies of an item have been created.
    fn dependency_synced(&self, item: &ImportQueueArgs) -> bool {
        match &item.parent_id {
            ParentId::FolderToUpload(id) => self.created_folders.contains(id),
            ParentId::InitialFolder(_) => true,
        }
    }

    /// Report that every object making up `file_id`'s import has been created.
    fn complete_file(&mut self, client_id: ClientId, ctx: &mut ModelContext<Self>) {
        if let Some(file_id) = self.file_completion.request_completed(client_id) {
            ctx.emit(ImportQueueEvent::FileCompleted {
                file_id,
                server_id: Some(client_id.to_string()),
            });
        }
    }

    // Enqueue a new request to the import queue.
    pub fn enqueue(&mut self, arg: ImportQueueArgs, ctx: &mut ModelContext<Self>) {
        // Update internal tracker of the object.
        match &arg.content {
            RequestContent::Folder {
                client_id,
                folder_id,
                ..
            } => {
                self.client_to_node_folder_id.insert(*client_id, *folder_id);
            }
            RequestContent::Notebook {
                client_id, file_id, ..
            } => self.file_completion.add_entry(*client_id, *file_id),
            RequestContent::Workflow {
                workflows, file_id, ..
            } => {
                for (_, client_id) in workflows {
                    self.file_completion.add_entry(*client_id, *file_id);
                }
            }
        }

        self.queue.push(arg);
        self.dequeue(ctx);
    }

    // Dequeue a new request from the import queue.
    pub fn dequeue(&mut self, ctx: &mut ModelContext<Self>) {
        if self.queue.is_empty() {
            return;
        }

        if let Some(idx) = self
            .queue
            .iter()
            .position(|item| self.dependency_synced(item))
        {
            let dequeued_item = self.queue.remove(idx);
            let parent_id = match dequeued_item.parent_id {
                // The parent keeps its client id for life, so the child can point at it
                // directly instead of waiting for a server id to be minted.
                ParentId::FolderToUpload(client_id) => Some(SyncId::ClientId(client_id)),
                ParentId::InitialFolder(folder_id) => folder_id,
            };

            match dequeued_item.content {
                RequestContent::Folder {
                    name, client_id, ..
                } => {
                    UpdateManager::handle(ctx).update(ctx, |update_manager, ctx| {
                        update_manager.create_folder(
                            name,
                            dequeued_item.owner,
                            client_id,
                            parent_id,
                            false,
                            InitiatedBy::User,
                            ctx,
                        );
                    });

                    self.created_folders.insert(client_id);
                    if let Some(node_id) = self.client_to_node_folder_id.get(&client_id) {
                        ctx.emit(ImportQueueEvent::FolderCompleted {
                            folder_id: *node_id,
                            server_id: Some(client_id.to_string()),
                        });
                    }
                }
                RequestContent::Notebook {
                    title,
                    data,
                    client_id,
                    file_id,
                } => {
                    UpdateManager::handle(ctx).update(ctx, |update_manager, ctx| {
                        update_manager.create_notebook(
                            client_id,
                            dequeued_item.owner,
                            parent_id,
                            CloudNotebookModel {
                                title,
                                data,
                                ai_document_id: None,
                                conversation_id: None,
                            },
                            CloudObjectEventEntrypoint::ImportModal,
                            false,
                            ctx,
                        );
                    });
                    ctx.emit(ImportQueueEvent::FileSavedLocally(file_id));
                    self.complete_file(client_id, ctx);
                }
                RequestContent::Workflow {
                    workflows,
                    workflow_enums,
                    file_id,
                } => {
                    let workflow_client_ids: Vec<ClientId> =
                        workflows.iter().map(|(_, client_id)| *client_id).collect();
                    UpdateManager::handle(ctx).update(ctx, |update_manager, ctx| {
                        // Create any new workflow enums
                        for (client_id, workflow_enum) in workflow_enums {
                            update_manager.create_workflow_enum(
                                workflow_enum,
                                dequeued_item.owner,
                                client_id,
                                CloudObjectEventEntrypoint::ImportModal,
                                false,
                                ctx,
                            );
                        }

                        // Create the workflow
                        for (workflow, client_id) in workflows {
                            update_manager.create_workflow(
                                workflow,
                                dequeued_item.owner,
                                parent_id,
                                client_id,
                                CloudObjectEventEntrypoint::ImportModal,
                                false,
                                ctx,
                            );
                        }
                    });
                    ctx.emit(ImportQueueEvent::FileSavedLocally(file_id));
                    for client_id in workflow_client_ids {
                        self.complete_file(client_id, ctx);
                    }
                }
            }
            self.dequeue(ctx);
        }
    }
}

impl Entity for ImportQueue {
    type Event = ImportQueueEvent;
}
