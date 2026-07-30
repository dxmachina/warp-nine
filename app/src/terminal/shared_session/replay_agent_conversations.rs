use std::collections::HashMap;

use api::response_event::stream_finished as stream_finished_event;
use api::{client_action as api_client_action, response_event as api_response_event};
use warp_multi_agent_api::{self as api, ResponseEvent};



/// Wrap a ClientAction in a ResponseEvent.
fn wrap_action_in_event(action: api_client_action::Action) -> ResponseEvent {
    ResponseEvent {
        r#type: Some(api_response_event::Type::ClientActions(
            api_response_event::ClientActions {
                actions: vec![api::ClientAction {
                    action: Some(action),
                }],
            },
        )),
    }
}

