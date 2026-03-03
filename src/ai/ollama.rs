use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::json;

use crate::config::OllamaConfig;
use crate::context::WindowContext;
use crate::dictionary::Dictionary;

use super::{build_system_prompt, ProcessResult, TextProcessor};

pub struct OllamaProcessor {
    host: String,
    model: String,
    client: reqwest::Client,
}

impl OllamaProcessor {
    pub fn new(config: &OllamaConfig) -> Result<Self> {
        Ok(Self {
            host: config.host.trim_end_matches('/').to_string(),
            model: config.model.clone(),
            client: reqwest::Client::new(),
        })
    }
}

#[async_trait]
impl TextProcessor for OllamaProcessor {
    async fn process(
        &self,
        raw_text: &str,
        context: &WindowContext,
        dictionary: &Dictionary,
        memory_context: &str,
    ) -> Result<ProcessResult> {
        let system_prompt = build_system_prompt(context, dictionary, memory_context);
        let user_prompt = format!("Clean up this speech-to-text output:\n\n{}", raw_text);

        let body = json!({
            "model": self.model,
            "system": system_prompt,
            "prompt": user_prompt,
            "stream": false,
        });

        let url = format!("{}/api/generate", self.host);

        let response = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .context("sending request to Ollama")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Ollama API error ({}): {}", status, body);
        }

        let resp: serde_json::Value = response.json().await.context("parsing Ollama response")?;

        let text = resp["response"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("unexpected Ollama response format: {}", resp))?
            .trim()
            .to_string();

        tracing::info!("Ollama processed: {}", text);
        Ok(ProcessResult {
            text,
            learnings: vec![],
        })
    }
}
