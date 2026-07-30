use std::sync::Arc;

use async_trait::async_trait;
use warpui::{Entity, SingletonEntity};

use super::server_api::{ServerApi, TranscribeError};
use crate::voice::transcriber::Transcriber;

pub struct ServerVoiceTranscriber {
    // LOCAL FORK: kept so the constructor signature is unchanged, but there is no
    // transcription endpoint left to call. See `transcribe` below.
    #[allow(dead_code)]
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
        // LOCAL FORK: both the request payload type and `ServerApi::transcribe` lived
        // with the agent, so there is no endpoint left to send the audio to. Fail
        // rather than silently returning empty text, so the caller can surface it.
        Err(TranscribeError::Other(anyhow::anyhow!(
            "voice transcription is not available in this build"
        )))
    }
}

impl Entity for ServerVoiceTranscriber {
    type Event = ();
}

impl SingletonEntity for ServerVoiceTranscriber {}
