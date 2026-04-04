use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::multipart;

use crate::audio::AudioData;
use crate::config::OpenAiApiConfig;

use super::SpeechRecognizer;

/// Default max response size for transcription API (10 MiB).
const DEFAULT_MAX_RESPONSE_BYTES: usize = 10 * 1024 * 1024;

pub struct OpenAiRecognizer {
    api_key: String,
    language: String,
    client: reqwest::Client,
    prompt_hint: String,
    max_response_bytes: usize,
}

impl OpenAiRecognizer {
    pub fn new(config: &OpenAiApiConfig) -> Result<Self> {
        let api_key = crate::config::resolve_api_key(&config.api_key_env)?;

        Ok(Self {
            api_key,
            language: config.language.clone(),
            client: reqwest::Client::new(),
            prompt_hint: String::new(),
            max_response_bytes: DEFAULT_MAX_RESPONSE_BYTES,
        })
    }
}

#[async_trait]
impl SpeechRecognizer for OpenAiRecognizer {
    fn set_prompt_hint(&mut self, hint: &str) {
        self.prompt_hint = hint.to_string();
    }

    async fn transcribe(&self, audio: &AudioData) -> Result<String> {
        let wav_bytes = audio.to_wav_bytes().context("encoding audio as WAV")?;

        let file_part = multipart::Part::bytes(wav_bytes)
            .file_name("audio.wav")
            .mime_str("audio/wav")?;

        let mut form = multipart::Form::new()
            .text("model", "whisper-1")
            .text("language", self.language.clone())
            .text("response_format", "text")
            .part("file", file_part);

        // Add prompt hint if available
        if !self.prompt_hint.is_empty() {
            form = form.text("prompt", self.prompt_hint.clone());
        }

        let response = self
            .client
            .post("https://api.openai.com/v1/audio/transcriptions")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .multipart(form)
            .send()
            .await
            .context("sending request to OpenAI Whisper API")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("OpenAI API error ({}): {}", status, body);
        }

        if let Some(len) = response.content_length() {
            if len > self.max_response_bytes as u64 {
                anyhow::bail!(
                    "OpenAI response too large (Content-Length: {} bytes, limit: {} bytes)",
                    len,
                    self.max_response_bytes
                );
            }
        }
        let bytes = response
            .bytes()
            .await
            .context("reading response body")?;
        if bytes.len() > self.max_response_bytes {
            anyhow::bail!(
                "OpenAI response too large ({} bytes, limit: {} bytes)",
                bytes.len(),
                self.max_response_bytes
            );
        }
        let text = String::from_utf8_lossy(&bytes).trim().to_string();

        tracing::info!("OpenAI transcription: {}", text);
        Ok(text)
    }
}
