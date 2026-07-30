use itertools::Itertools;
use serde::{Deserialize, Serialize};
use warp_graphql::mutations::generate_metadata_for_command::{
    GenerateMetadataForCommandFailureType, GenerateMetadataForCommandSuccess,
};
use warpui::ViewContext;

use super::arguments::ArgumentsState;
use super::modal::{AiAssistState, WorkflowModal, WorkflowModalEvent};
use crate::workflows::workflow::Workflow;

/// Generated command metadata from server.
#[derive(Debug)]
pub struct GeneratedCommandMetadata {
    pub command: String,
    pub title: String,
    pub description: String,
    pub arguments: Vec<GeneratedArgument>,
}

/// Metadata for a parameter in the workflow.
#[derive(Debug)]
pub struct GeneratedArgument {
    pub name: String,
    pub description: String,
    pub default_value: String,
}

impl From<GenerateMetadataForCommandSuccess> for GeneratedCommandMetadata {
    fn from(value: GenerateMetadataForCommandSuccess) -> Self {
        GeneratedCommandMetadata {
            command: value.parameterized_command,
            title: value.title,
            description: value.description,
            arguments: value
                .parameters
                .into_iter()
                .map(|p| GeneratedArgument {
                    name: p.name,
                    description: p.description,
                    default_value: p.value,
                })
                .collect_vec(),
        }
    }
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
pub enum GeneratedCommandMetadataError {
    /// OpenAI failed to generate a parsable response.
    BadCommand,
    /// Request to OpenAI failed
    AiProviderError,
    /// User is over rate limit.
    RateLimited,
    Other,
}

impl GeneratedCommandMetadataError {
    pub fn user_facing_message(&self) -> String {
        match self {
            Self::BadCommand => {
                "Failed to generate metadata. Please try again with a different command."
            }
            Self::AiProviderError => "Something went wrong. Please try again.",
            Self::RateLimited => "Looks like you're out of AI credits. Please try again later.",
            Self::Other => "Something went wrong. Please try again.",
        }
        .to_string()
    }
}

impl From<GenerateMetadataForCommandFailureType> for GeneratedCommandMetadataError {
    fn from(value: GenerateMetadataForCommandFailureType) -> Self {
        match value {
            GenerateMetadataForCommandFailureType::BadCommand => Self::BadCommand,
            GenerateMetadataForCommandFailureType::AiProviderError => Self::AiProviderError,
            GenerateMetadataForCommandFailureType::RateLimited => Self::RateLimited,
            GenerateMetadataForCommandFailureType::Other => Self::Other,
        }
    }
}

impl WorkflowModal {
    /// Send request to generate metadata for the command in command editor.
    ///
    /// LOCAL FORK: the AI client that generated workflow metadata went with the agent.
    /// The entry point is kept so the modal's "AI assist" button still resolves, but the
    /// request can never be issued; report it immediately rather than leaving the modal
    /// stuck in `RequestInFlight` with its editors disabled.
    pub(super) fn issue_request(&mut self, ctx: &mut ViewContext<Self>) {
        self.ai_metadata_assist_state = AiAssistState::PreRequest;
        self.enable_editors(ctx);
        ctx.emit(WorkflowModalEvent::AiAssistError(
            "Generating workflow metadata is not available in this build.".to_string(),
        ));
        ctx.notify();
    }

    // Populate only the missing field in the workflow editor with the generated suggestion from AI.
    pub(super) fn populate_missing_field_with_suggestion(
        &mut self,
        workflow: Workflow,
        ctx: &mut ViewContext<Self>,
    ) {
        self.title_editor.update(ctx, |editor, ctx| {
            if editor.is_empty(ctx) {
                editor.set_buffer_text(workflow.name(), ctx);
            }
        });

        self.description_editor.update(ctx, |editor, ctx| {
            if editor.is_empty(ctx) {
                editor.set_buffer_text(
                    workflow
                        .description()
                        .map(String::as_str)
                        .unwrap_or_default(),
                    ctx,
                );
            }
        });

        let content_parsed = !self.arguments_state.arguments.is_empty();
        if !content_parsed {
            self.content_editor.update(ctx, |editor, ctx| {
                editor.set_buffer_text(workflow.content(), ctx);
            });

            // note: normally, we wouldn't have to do this, since editing the command
            // editor's text will trigger the event that does this automatically.
            // however, that happens in a callback, yet we need to know what the args
            // are right away to populate the description/default value editors.
            self.arguments_state = ArgumentsState::for_command_workflow(
                &self.arguments_state,
                workflow.content().to_string(),
            );
            self.update_arguments_rows(ctx);

            workflow
                .arguments()
                .iter()
                .enumerate()
                .for_each(|(index, argument)| {
                    // Since suggestion generated by AI is non-deterministic, we should make sure to handle each
                    // operation safely.
                    if index >= self.arguments_rows.len() {
                        return;
                    }

                    if let Some(description) = &argument.description {
                        self.arguments_rows[index]
                            .description_editor
                            .update(ctx, |editor, ctx| {
                                editor.set_buffer_text(description.as_str(), ctx);
                            });
                    }

                    if let Some(default_value) = &argument.default_value {
                        self.arguments_rows[index].default_value_editor.update(
                            ctx,
                            |editor, ctx| {
                                editor.set_buffer_text(default_value.as_str(), ctx);
                            },
                        );
                    }
                });
        }
    }
}
