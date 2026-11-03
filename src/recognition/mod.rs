pub mod openai_api;
pub mod whisper_local;

use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;

use crate::audio::AudioData;
use crate::config::{RecognitionConfig, RecognitionEngine};

/// Trait for speech recognition engines.
// `double_must_use` fires inside async-trait's generated code (fixed in async-trait 0.1.92).
#[allow(clippy::double_must_use)]
#[async_trait]
pub trait SpeechRecognizer: Send + Sync {
    async fn transcribe(&self, audio: &AudioData) -> Result<String>;

    /// Set a prompt hint to improve recognition of specific terms.
    /// The hint is a comma-separated list of terms the model should recognize.
    fn set_prompt_hint(&self, hint: &str);

    /// Whether repeated transcription while recording (live transcript) is
    /// cheap enough. False for paid cloud APIs.
    fn supports_live(&self) -> bool {
        false
    }
}

/// Create a recognizer based on config.
pub fn create_recognizer(config: &RecognitionConfig) -> Result<Arc<dyn SpeechRecognizer>> {
    match config.engine {
        RecognitionEngine::WhisperLocal => {
            let whisper_config = config
                .whisper_local
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("whisper_local config missing"))?;
            Ok(Arc::new(whisper_local::WhisperLocalRecognizer::new(
                whisper_config,
            )?))
        }
        RecognitionEngine::OpenaiApi => {
            let api_config = config
                .openai_api
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("openai_api config missing"))?;
            Ok(Arc::new(openai_api::OpenAiRecognizer::new(api_config)?))
        }
    }
}
