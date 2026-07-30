use std::sync::Arc;

use async_trait::async_trait;
use warpui::{Entity, SingletonEntity};

use super::server_api::{ServerApi, TranscribeError};
use crate::voice::transcriber::Transcriber;

pub struct ServerVoiceTranscriber {
    server_api: Arc<ServerApi>,
}

impl ServerVoiceTranscriber {
    pub fn new(server_api: Arc<ServerApi>) -> Self {
        Self { server_api }
    }
}

#[cfg_attr(not(target_family = "wasm"), async_trait)]
#[cfg_attr(target_family = "wasm", async_trait(?Send))]
impl Transcriber for ServerVoiceTranscriber {
    async fn transcribe(
        &self,
        _wav_base64: String,
        _language: Option<String>,
    ) -> Result<String, TranscribeError> {
        // LOCAL FORK: the request payload type lived with the agent, so the audio and
        // language are no longer carried to the endpoint.
        let response = self.server_api.transcribe().await;
        match response {
            Ok(response) => Ok(response.text),
            Err(e) => Err(e),
        }
    }
}

impl Entity for ServerVoiceTranscriber {
    type Event = ();
}

impl SingletonEntity for ServerVoiceTranscriber {}
