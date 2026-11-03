use anyhow::{Context, Result};
use async_trait::async_trait;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::audio::AudioData;
use crate::config::WhisperLocalConfig;

use super::SpeechRecognizer;

pub struct WhisperLocalRecognizer {
    ctx: std::sync::Arc<WhisperContext>,
    language: String,
    prompt_hint: std::sync::Mutex<String>,
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
            ctx: std::sync::Arc::new(ctx),
            language: config.language.clone(),
            prompt_hint: std::sync::Mutex::new(String::new()),
        })
    }
}

#[async_trait]
impl SpeechRecognizer for WhisperLocalRecognizer {
    fn set_prompt_hint(&self, hint: &str) {
        if let Ok(mut h) = self.prompt_hint.lock() {
            *h = hint.to_string();
        }
    }

    fn supports_live(&self) -> bool {
        true
    }

    async fn transcribe(&self, audio: &AudioData) -> Result<String> {
        let samples = audio.resample_to_16khz();

        if samples.is_empty() {
            return Ok(String::new());
        }

        let language = self.language.clone();
        let prompt_hint = self
            .prompt_hint
            .lock()
            .map(|h| h.clone())
            .unwrap_or_default();

        // whisper-rs is blocking, so run in a blocking thread
        // The blocking task holds its own reference, so the context stays
        // alive even if this recognizer is dropped (config reload, shutdown).
        let ctx = std::sync::Arc::clone(&self.ctx);
        let result = tokio::task::spawn_blocking(move || -> Result<String> {
            let mut state = ctx.create_state().context("creating whisper state")?;
            let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });

            params.set_language(Some(&language));
            params.set_print_special(false);
            params.set_print_progress(false);
            params.set_print_realtime(false);
            params.set_print_timestamps(false);
            params.set_suppress_blank(true);
            params.set_suppress_nst(true);

            if !prompt_hint.is_empty() {
                params.set_initial_prompt(&prompt_hint);
            }

            state
                .full(params, &samples)
                .map_err(|e| anyhow::anyhow!("whisper transcription failed: {}", e))?;

            let num_segments = state.full_n_segments();
            let mut text = String::new();
            for i in 0..num_segments {
                if let Some(segment) = state.get_segment(i) {
                    if let Ok(s) = segment.to_str() {
                        text.push_str(s);
                    }
                }
            }

            Ok(text.trim().to_string())
        })
        .await
        .context("whisper task panicked")??;

        tracing::debug!("Whisper transcription: {}", result);
        Ok(result)
    }
}
