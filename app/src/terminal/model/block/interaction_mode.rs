use warp_terminal::model::Point;
use warp_terminal::model::grid::Dimensions;

use super::{Block, SerializedAIMetadata};
use crate::terminal::event::Event;
use crate::terminal::model::RespectObfuscatedSecrets;
use crate::terminal::model::grid::RespectDisplayedOutput;
use crate::terminal::model::grid::grid_handler::GridHandler;

impl Block {
    /// `true` if the command is executing and the user has opened the agent mode input.
    ///
    /// "tagged in" means that the agent mode input should be shown, but control has yet
    /// to be passed to the agent.
    pub fn is_agent_tagged_in(&self) -> bool {
        if !self.is_active_and_long_running() {
            return false;
        }

        match &self.interaction_mode {
            InteractionMode::User(user_mode) => user_mode.did_user_tag_in_agent,
            _ => false,
        }
    }

    /// `true` if the command is eligible for tagging in the agent (e.g. showing the agent mode
    /// input and sending a query to trigger the CLI subagent).
    ///
    /// Notably, this is NOT true if the subagent had already taken control of the command, and
    /// the user took control back from the subagent.
    ///
    /// See doc comment on `InteractionMode` for explanation on the semantics of 'tagged in',
    /// 'agent in control', 'user take control, and 'agent handoff'.
    pub fn is_eligible_to_tag_in_agent(&self) -> bool {
        if !self.is_active_and_long_running()
            || self.is_in_band_command_block()
            || !self.bootstrap_stage.is_bootstrapped()
            || self.env_var_metadata().is_some()
        {
            return false;
        }

        match &self.interaction_mode {
            InteractionMode::User(user_mode) => !user_mode.did_user_tag_in_agent,
            _ => false,
        }
    }

    pub fn set_is_agent_tagged_in(&mut self, value: bool) {
        let block_id = self.id().clone();
        if let InteractionMode::User(UserMode {
            did_user_tag_in_agent,
        }) = &mut self.interaction_mode
            && *did_user_tag_in_agent != value
        {
            *did_user_tag_in_agent = value;
            self.event_proxy
                .send_terminal_event(Event::AgentTaggedInChanged {
                    block_id,
                    is_tagged_in: value,
                });
        }
    }

    /// LOCAL FORK: monitoring was tracked by `LongRunningCommandControlState`,
    /// which went with the agent. Nothing monitors a command now.
    pub fn is_agent_monitoring(&self) -> bool {
        false
    }

    /// LOCAL FORK: control was tracked by `LongRunningCommandControlState`,
    /// which went with the agent. Nothing can take control now.
    pub fn is_agent_in_control(&self) -> bool {
        false
    }

    /// Returns `true` if the agent is either in control or has been tagged in by the user.
    pub fn is_agent_in_control_or_tagged_in(&self) -> bool {
        self.is_agent_in_control() || self.is_agent_tagged_in()
    }

    /// LOCAL FORK: driving required either agent control or an agent-requested
    /// command action, both of which went with the agent.
    pub fn is_agent_driving_command(&self) -> bool {
        false
    }

    /// LOCAL FORK: there is no control state left to hand back, so teardown
    /// paths have nothing to do. Kept as a no-op for the call sites.
    pub fn set_user_control_for_teardown(&mut self) {}

    /// LOCAL FORK: response hiding was driven by the agent's control state.
    pub fn should_hide_responses(&self) -> bool {
        false
    }

    /// Returns the `agent_interaction_metadata` associated with this block, if any.
    pub fn agent_interaction_metadata(&self) -> Option<&AgentInteractionMetadata> {
        self.interaction_mode.agent_interaction_metadata()
    }


    // LOCAL FORK: fns requested_command_action_id and long_running_control_state
    // removed with the agent; both returned agent-owned types.

    /// LOCAL FORK: requested commands came from agent actions, which are gone.
    pub fn is_agent_requested_command(&self) -> bool {
        false
    }

    pub fn has_agent_written_to_block(&self) -> bool {
        self.interaction_mode
            .agent_interaction_metadata()
            .is_some_and(|metadata| metadata.has_agent_written_to_block())
    }

    pub fn mark_agent_written_to_block(&mut self) {
        if let InteractionMode::Agent(metadata) = &mut self.interaction_mode {
            metadata.has_agent_written_to_block = true;
        }
    }

    pub fn set_should_hide(&mut self, value: bool) {
        self.interaction_mode.set_should_hide_block(value);
    }



    pub fn set_agent_interaction_mode(
        &mut self,
        agent_interaction_metadata: AgentInteractionMetadata,
    ) {
        self.interaction_mode = InteractionMode::new_agent(agent_interaction_metadata);
    }

    pub fn set_interaction_mode_from_serialized_ai_metadata(
        &mut self,
        serialized_metadata: SerializedAIMetadata,
    ) {
        self.interaction_mode = InteractionMode::from_serialized_ai_metadata(serialized_metadata);
    }

    // LOCAL FORK: fns take_over_control_for_user and handoff_control_to_agent
    // removed with the agent; there is no control to transfer.
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum UpdateInteractionModeError {
    #[error(
        "Attempted to update interaction mode from agent with requested command to agent-monitored for mismatched conversation IDs."
    )]
    UnexpectedConversationId,
    #[error("Attempted to take over control for user when block was not already agent controlled")]
    InvalidTakeOver,
    #[error("Attempted to handoff control to agent when block was not already user controlled")]
    InvalidHandOff,
}

#[derive(Debug, Clone)]
pub struct UserMode {
    // `true` if the user executed the command themself and the agent mode input should be shown.
    //
    // This does _not_ mean an agent is in control of the command. This merely means the user has
    // opted to show the agent input, indicated intent to send a query to give control.
    //
    // If the user executes a command, shows the input, then hides the input, this reverts to
    // `false`.
    did_user_tag_in_agent: bool,
}

/// Represents the 'interaction mode' for a command block with respect to the agent.
///
/// There are 4 user-perceived states:
///
/// 1) The command was executed by the user; if long-running, they are in control and the input is hidden.
/// 2) The command was executed by the user, but is long-running and the user has toggled on the
///    agent input and may send a query to trigger the Agent (the CLI subagent). We refer to this
///    state, where the user executed the command and deliberately opened the agent input, as the
///    agent being 'tagged in'. Note that this is distinct from the agent actually having control.
///    "Tagged in" merely means the command is running and the agent mode input is visible
/// 3) The command was executed by the agent (is a requested command) and is not long running. No CLI subagent was triggered.
/// 4) The command was executed by the agent and is long running, and thus the CLI subagent was triggered.
///   a) The agent is in control of the command (actively reading the command's output or writing input to the command)
///   b) The agent was in the control of the command, but the user took over.
///
/// The `User` variant represents modes where the user executed the original command and the agent has yet to take control.
/// The `Agent` variant represents modes where the agent either ran the command itself, or the user tagged in the agent and
/// passed control to the CLI subagent by sending a query during its execution
#[derive(Debug, Clone)]
pub enum InteractionMode {
    User(UserMode),
    Agent(AgentInteractionMetadata),
}

impl InteractionMode {

    fn new_agent(metadata: AgentInteractionMetadata) -> Self {
        Self::Agent(metadata)
    }

    fn from_serialized_ai_metadata(serialized_metadata: SerializedAIMetadata) -> Self {
        Self::Agent(serialized_metadata.into())
    }

    fn agent_interaction_metadata(&self) -> Option<&AgentInteractionMetadata> {
        match self {
            Self::Agent(agent_interaction_metadata) => Some(agent_interaction_metadata),
            Self::User(_) => None,
        }
    }

    pub fn should_hide_block(&self) -> bool {
        match self {
            Self::Agent(metadata) => metadata.should_hide_block,
            _ => false,
        }
    }

    pub fn is_agent_tagged_in(&self) -> bool {
        matches!(
            self,
            Self::User(UserMode {
                did_user_tag_in_agent: true
            })
        )
    }

    fn set_should_hide_block(&mut self, value: bool) {
        if let Self::Agent(metadata) = self {
            metadata.should_hide_block = value;
        }
    }

    // LOCAL FORK: fn take_over_for_user removed with the agent.
}

impl Default for InteractionMode {
    fn default() -> Self {
        Self::User(UserMode {
            did_user_tag_in_agent: false,
        })
    }
}

/// Blocklist AI metadata associated with this block.
///
/// LOCAL FORK: the action, conversation, subagent-task and long-running-control
/// fields went with the agent; only the two block-local flags are left.
#[derive(Debug, Clone)]
pub struct AgentInteractionMetadata {
    /// `true` if the agent has previously written to this block.
    has_agent_written_to_block: bool,

    /// `true` if this block should be hidden from the user (as is the case with AI-requested
    /// commands, for example).
    should_hide_block: bool,
}

impl AgentInteractionMetadata {
    /// Creates a new metadata instance with fully specified fields.
    pub fn new(
        has_agent_written_to_block: bool,
        should_hide_block: bool,
    ) -> Self {
        AgentInteractionMetadata {
            has_agent_written_to_block,
            should_hide_block,
        }
    }

    /// Convenience constructor for the common "hidden by default" case used for requested commands.
    pub fn new_hidden() -> Self {
        Self::new(false, true)
    }

    /// LOCAL FORK: control was tracked by `LongRunningCommandControlState`,
    /// which went with the agent.
    pub fn is_agent_in_control(&self) -> bool {
        false
    }

    pub fn has_agent_written_to_block(&self) -> bool {
        self.has_agent_written_to_block
    }

    pub fn should_hide_block(&self) -> bool {
        self.should_hide_block
    }
}

/// String representation of the cursor to interpolate in the terminal contents string.
pub const CURSOR_MARKER: &str = "<|cursor|>";

/// Returns a string representation of the terminal contents (represented by the `grid_handler`),
/// limited to `max_row_count` rows in the grid.
///
/// This function returns a string representation of the terminal contents, with a cursor "marker" substring
/// interpolated at the same position in the string as it appears in the grid.
pub fn formatted_terminal_contents_for_input(
    grid_handler: &GridHandler,
    max_row_count: Option<usize>,
    cursor_pattern: &'static str,
) -> String {
    let cursor_point = grid_handler.cursor_point();

    let max_column_index = grid_handler.columns().saturating_sub(1);
    let (context_start_point, context_end_point) = match max_row_count {
        Some(max_count) => {
            // Return start and end points such that the range is of size max_count, bounded to the
            // max row value of the grid.
            let end_point = Point::new(grid_handler.max_content_row(), max_column_index).min(
                Point::new(cursor_point.row + max_count / 2, max_column_index),
            );
            let start_point = Point::new(end_point.row.saturating_sub(max_count), 0);
            (start_point, end_point)
        }
        None => (
            Point::new(0, 0),
            Point::new(
                grid_handler.total_rows().saturating_sub(1),
                grid_handler.columns().saturating_sub(1),
            ),
        ),
    };

    format!(
        "{}{}{cursor_pattern}{}",
        grid_handler.bounds_to_string(
            context_start_point,
            if cursor_point.col == 0 {
                Point::new(
                    cursor_point.row.saturating_sub(1),
                    grid_handler.columns().saturating_sub(1),
                )
            } else {
                Point::new(cursor_point.row, cursor_point.col.saturating_sub(1))
            },
            false,
            RespectObfuscatedSecrets::Yes,
            true,
            RespectDisplayedOutput::No,
        ),
        if cursor_point.col == 0 { "\n" } else { "" },
        grid_handler.bounds_to_string(
            cursor_point,
            context_end_point,
            false,
            RespectObfuscatedSecrets::Yes,
            true,
            RespectDisplayedOutput::No,
        )
    )
}
