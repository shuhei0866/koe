pub mod openai_api;
pub mod whisper_local;

use anyhow::Result;
use async_trait::async_trait;

use crate::audio::AudioData;
use crate::config::{RecognitionConfig, RecognitionEngine};

/// Trait for speech recognition engines.
#[async_trait]
pub trait SpeechRecognizer: Send + Sync {
    async fn transcribe(&self, audio: &AudioData) -> Result<String>;
}

/// Create a recognizer based on config.
pub fn create_recognizer(config: &RecognitionConfig) -> Result<Box<dyn SpeechRecognizer>> {
    match config.engine {
        RecognitionEngine::WhisperLocal => {
            let whisper_config = config
                .whisper_local
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("whisper_local config missing"))?;
            Ok(Box::new(whisper_local::WhisperLocalRecognizer::new(
                whisper_config,
            )?))
        }
        RecognitionEngine::OpenaiApi => {
            let api_config = config
                .openai_api
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("openai_api config missing"))?;
            Ok(Box::new(openai_api::OpenAiRecognizer::new(api_config)?))
        }
    }
}
