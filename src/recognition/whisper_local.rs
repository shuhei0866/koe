use anyhow::{Context, Result};
use async_trait::async_trait;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::audio::AudioData;
use crate::config::WhisperLocalConfig;

use super::SpeechRecognizer;

pub struct WhisperLocalRecognizer {
    ctx: WhisperContext,
    language: String,
}

impl WhisperLocalRecognizer {
    pub fn new(config: &WhisperLocalConfig) -> Result<Self> {
        let model_path = shellexpand::tilde(&config.model_path);
        let path = std::path::Path::new(model_path.as_ref());

        if !path.exists() {
            anyhow::bail!(
                "Whisper model not found: {}. Download from https://huggingface.co/ggerganov/whisper.cpp",
                path.display()
            );
        }

        tracing::info!("Loading whisper model: {}", path.display());
        let ctx = WhisperContext::new_with_params(
            path.to_str().unwrap(),
            WhisperContextParameters::default(),
        )
        .context("failed to load whisper model")?;

        Ok(Self {
            ctx,
            language: config.language.clone(),
        })
    }
}

#[async_trait]
impl SpeechRecognizer for WhisperLocalRecognizer {
    async fn transcribe(&self, audio: &AudioData) -> Result<String> {
        let samples = audio.resample_to_16khz();

        if samples.is_empty() {
            return Ok(String::new());
        }

        let language = self.language.clone();

        // whisper-rs is blocking, so run in a blocking thread
        let ctx_ptr = &self.ctx as *const WhisperContext as usize;
        let result = tokio::task::spawn_blocking(move || -> Result<String> {
            // SAFETY: WhisperContext is Send+Sync, and we hold it for the lifetime of the app
            let ctx = unsafe { &*(ctx_ptr as *const WhisperContext) };

            let mut state = ctx.create_state().context("creating whisper state")?;
            let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });

            params.set_language(Some(&language));
            params.set_print_special(false);
            params.set_print_progress(false);
            params.set_print_realtime(false);
            params.set_print_timestamps(false);
            params.set_suppress_blank(true);
            params.set_suppress_non_speech_tokens(true);

            state
                .full(params, &samples)
                .context("whisper transcription failed")?;

            let num_segments = state.full_n_segments().context("getting segment count")?;
            let mut text = String::new();
            for i in 0..num_segments {
                if let Ok(segment) = state.full_get_segment_text(i) {
                    text.push_str(&segment);
                }
            }

            Ok(text.trim().to_string())
        })
        .await
        .context("whisper task panicked")??;

        tracing::info!("Whisper transcription: {}", result);
        Ok(result)
    }
}
