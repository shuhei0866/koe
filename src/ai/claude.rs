use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::json;

use crate::config::ClaudeConfig;
use crate::context::WindowContext;
use crate::dictionary::Dictionary;

use super::{build_system_prompt, TextProcessor};

pub struct ClaudeProcessor {
    api_key: String,
    model: String,
    client: reqwest::Client,
}

impl ClaudeProcessor {
    pub fn new(config: &ClaudeConfig) -> Result<Self> {
        let api_key = std::env::var(&config.api_key_env).with_context(|| {
            format!(
                "Environment variable {} not set for Claude API key",
                config.api_key_env
            )
        })?;

        Ok(Self {
            api_key,
            model: config.model.clone(),
            client: reqwest::Client::new(),
        })
    }
}

#[async_trait]
impl TextProcessor for ClaudeProcessor {
    async fn process(
        &self,
        raw_text: &str,
        context: &WindowContext,
        dictionary: &Dictionary,
    ) -> Result<String> {
        let system_prompt = build_system_prompt(context, dictionary);

        let body = json!({
            "model": self.model,
            "max_tokens": 4096,
            "system": system_prompt,
            "messages": [
                {
                    "role": "user",
                    "content": format!("Clean up this speech-to-text output:\n\n{}", raw_text)
                }
            ]
        });

        let response = self
            .client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .context("sending request to Claude API")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Claude API error ({}): {}", status, body);
        }

        let resp: serde_json::Value = response.json().await.context("parsing Claude response")?;

        let text = resp["content"]
            .as_array()
            .and_then(|arr| arr.first())
            .and_then(|block| block["text"].as_str())
            .ok_or_else(|| anyhow::anyhow!("unexpected Claude response format: {}", resp))?
            .to_string();

        tracing::info!("Claude processed: {}", text);
        Ok(text)
    }
}
