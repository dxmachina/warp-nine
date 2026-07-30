#[cfg(not(target_family = "wasm"))]
pub mod persistence;

use cloud_objects::cloud_object::{
    GenericCloudObject, GenericServerObject, ObjectType, ServerObjectModel,
};
use cloud_objects::ids::{ServerId, SyncId};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Serialized representation of a notebook for sync queue
/// The AIDocumentID and ConversationID are stored here to avoid polluting the
/// generic CreateObjectRequest type.
#[derive(Serialize, Deserialize)]
pub struct SerializedNotebook {
    pub data: String,
    pub ai_document_id: Option<String>,
    pub conversation_id: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CloudNotebookModel {
    pub title: String,
    pub data: String,
    /// LOCAL FORK: this was `ai::document::AIDocumentId`, a newtype over `Uuid` whose only
    /// behavior was rejecting non-UUID strings on parse and printing the inner `Uuid`. The
    /// stored/serialized form is unchanged.
    pub ai_document_id: Option<Uuid>,
    /// This is the server-generated conversation token, not the client-side AIConversationId.
    pub conversation_id: Option<String>,
}

impl ServerObjectModel for CloudNotebookModel {
    fn object_type(&self) -> ObjectType {
        ObjectType::Notebook
    }
}

/// This is the notebook_id in the database associated with this notebook.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub struct NotebookId(ServerId);
cloud_objects::server_id_traits! { NotebookId, "Notebook" }

impl From<NotebookId> for SyncId {
    fn from(id: NotebookId) -> Self {
        Self::ServerId(id.into())
    }
}

/// `CloudNotebook` is a notebook retrieved from the server.
pub type CloudNotebook = GenericCloudObject<NotebookId, CloudNotebookModel>;
pub type ServerNotebook = GenericServerObject<NotebookId, CloudNotebookModel>;
