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
    model: String,
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
            model: config.model.clone(),
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
            .text("model", self.model.clone())
            .text("response_format", "json")
            .part("file", file_part);

        if !self.language.is_empty() {
            let field = language_field(&self.model);
            form = form.text(field, self.language.clone());
        }

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
            .context("sending request to OpenAI transcription API")?;

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
        let bytes = response.bytes().await.context("reading response body")?;
        if bytes.len() > self.max_response_bytes {
            anyhow::bail!(
                "OpenAI response too large ({} bytes, limit: {} bytes)",
                bytes.len(),
                self.max_response_bytes
            );
        }
        let text = parse_transcript(&bytes)?;

        tracing::info!("OpenAI transcription ({}): {}", self.model, text);
        Ok(text)
    }
}

/// Form field for the language hint. `gpt-transcribe` takes a `languages`
/// array (sent as repeated `languages[]` parts) and must not also get the
/// singular `language` that older models use.
fn language_field(model: &str) -> &'static str {
    if model.starts_with("gpt-transcribe") {
        "languages[]"
    } else {
        "language"
    }
}

/// Extract the transcript from a `response_format=json` body (`{"text": ...}`).
fn parse_transcript(body: &[u8]) -> Result<String> {
    #[derive(serde::Deserialize)]
    struct Transcription {
        text: String,
    }
    let parsed: Transcription =
        serde_json::from_slice(body).context("parsing OpenAI transcription response")?;
    Ok(parsed.text.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpt_transcribe_uses_languages_array() {
        assert_eq!(language_field("gpt-transcribe"), "languages[]");
        assert_eq!(language_field("gpt-transcribe-2026-07-28"), "languages[]");
    }

    #[test]
    fn older_models_use_singular_language() {
        assert_eq!(language_field("whisper-1"), "language");
        assert_eq!(language_field("gpt-4o-transcribe"), "language");
        assert_eq!(language_field("gpt-4o-mini-transcribe"), "language");
    }

    #[test]
    fn transcript_is_read_from_text_field() {
        let body = r#"{"text": " こんにちは。 ", "languages": [{"code": "ja"}]}"#.as_bytes();
        assert_eq!(parse_transcript(body).unwrap(), "こんにちは。");
    }

    #[test]
    fn body_without_text_is_an_error() {
        assert!(parse_transcript(br#"{"error": "x"}"#).is_err());
        assert!(parse_transcript(b"plain text").is_err());
    }
}
