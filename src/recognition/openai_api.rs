use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::multipart;

use crate::audio::AudioData;
use crate::config::OpenAiApiConfig;

use super::SpeechRecognizer;

pub struct OpenAiRecognizer {
    api_key: String,
    language: String,
    client: reqwest::Client,
    prompt_hint: String,
}

impl OpenAiRecognizer {
    pub fn new(config: &OpenAiApiConfig) -> Result<Self> {
        let api_key = crate::config::resolve_api_key(&config.api_key_env)?;

        Ok(Self {
            api_key,
            language: config.language.clone(),
            client: reqwest::Client::new(),
            prompt_hint: String::new(),
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

        let text = response.text().await.context("reading response body")?;
        let text = text.trim().to_string();

        tracing::info!("OpenAI transcription: {}", text);
        Ok(text)
    }
}
