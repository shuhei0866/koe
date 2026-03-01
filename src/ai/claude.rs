use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::json;

use crate::config::ClaudeConfig;
use crate::context::WindowContext;
use crate::dictionary::Dictionary;

use super::{build_system_prompt, TextProcessor};

pub const ANTHROPIC_API_URL: &str = "https://api.anthropic.com/v1/messages";
pub const ANTHROPIC_VERSION: &str = "2023-06-01";

pub struct ClaudeProcessor {
    api_key: String,
    model: String,
    client: reqwest::Client,
}

impl ClaudeProcessor {
    pub fn new(config: &ClaudeConfig) -> Result<Self> {
        let api_key = crate::config::resolve_api_key(&config.api_key_env)?;

        Ok(Self {
            api_key,
            model: config.model.clone(),
            client: reqwest::Client::new(),
        })
    }
}

/// Build the JSON request body for the Claude Messages API.
pub fn build_request_body(model: &str, system_prompt: &str, user_message: &str) -> serde_json::Value {
    json!({
        "model": model,
        "max_tokens": 4096,
        "system": system_prompt,
        "messages": [
            {
                "role": "user",
                "content": user_message
            }
        ]
    })
}

/// Extract text from a Claude Messages API response.
pub fn parse_response_text(resp: &serde_json::Value) -> Result<String> {
    resp["content"]
        .as_array()
        .and_then(|arr| arr.first())
        .and_then(|block| block["text"].as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow::anyhow!("unexpected Claude response format: {}", resp))
}

/// Send a minimal test request to verify API key and model.
/// Returns the model's response text on success.
pub fn test_connectivity(api_key: &str, model: &str) -> Result<String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .context("building HTTP client")?;

    let body = json!({
        "model": model,
        "max_tokens": 64,
        "messages": [
            {
                "role": "user",
                "content": "Reply with exactly: OK"
            }
        ]
    });

    let response = client
        .post(ANTHROPIC_API_URL)
        .header("x-api-key", api_key)
        .header("anthropic-version", ANTHROPIC_VERSION)
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .context("sending request to Claude API")?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().unwrap_or_default();
        anyhow::bail!("API error ({}): {}", status, body);
    }

    let resp: serde_json::Value = response.json().context("parsing Claude response")?;
    parse_response_text(&resp)
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
        let body = build_request_body(
            &self.model,
            &system_prompt,
            &format!("Clean up this speech-to-text output:\n\n{}", raw_text),
        );

        let response = self
            .client
            .post(ANTHROPIC_API_URL)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
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
        let text = parse_response_text(&resp)?;

        tracing::info!("Claude processed: {}", text);
        Ok(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_request_body_structure() {
        let body = build_request_body("claude-sonnet-4-6", "system prompt", "hello");
        assert_eq!(body["model"], "claude-sonnet-4-6");
        assert_eq!(body["max_tokens"], 4096);
        assert_eq!(body["system"], "system prompt");
        let messages = body["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(messages[0]["content"], "hello");
    }

    #[test]
    fn test_parse_response_text_success() {
        let resp: serde_json::Value = serde_json::from_str(r#"
        {
            "id": "msg_123",
            "type": "message",
            "role": "assistant",
            "content": [
                { "type": "text", "text": "OK" }
            ],
            "model": "claude-sonnet-4-6",
            "stop_reason": "end_turn"
        }
        "#).unwrap();
        assert_eq!(parse_response_text(&resp).unwrap(), "OK");
    }

    #[test]
    fn test_parse_response_text_multi_block() {
        let resp: serde_json::Value = serde_json::from_str(r#"
        {
            "content": [
                { "type": "text", "text": "First block" },
                { "type": "text", "text": "Second block" }
            ]
        }
        "#).unwrap();
        // Should return the first block
        assert_eq!(parse_response_text(&resp).unwrap(), "First block");
    }

    #[test]
    fn test_parse_response_text_empty_content() {
        let resp: serde_json::Value = serde_json::from_str(r#"
        { "content": [] }
        "#).unwrap();
        assert!(parse_response_text(&resp).is_err());
    }

    #[test]
    fn test_parse_response_text_unexpected_format() {
        let resp: serde_json::Value = serde_json::from_str(r#"
        { "error": "something went wrong" }
        "#).unwrap();
        assert!(parse_response_text(&resp).is_err());
    }

    /// Integration test: requires ANTHROPIC_API_KEY in env or GNOME Keyring.
    /// Run with: cargo test test_claude_api_connectivity -- --ignored
    #[test]
    #[ignore]
    fn test_claude_api_connectivity() {
        let api_key = crate::config::resolve_api_key("ANTHROPIC_API_KEY")
            .expect("ANTHROPIC_API_KEY must be available (env or keyring)");

        let result = test_connectivity(&api_key, "claude-sonnet-4-6");
        match &result {
            Ok(text) => println!("Claude responded: {}", text),
            Err(e) => panic!("Claude API test failed: {}", e),
        }
        assert!(result.is_ok());
    }

    /// Test claude-sonnet-4-6 model specifically.
    /// Run with: cargo test test_claude_sonnet_4_6 -- --ignored --nocapture
    #[test]
    #[ignore]
    fn test_claude_sonnet_4_6() {
        let api_key = crate::config::resolve_api_key("ANTHROPIC_API_KEY")
            .expect("ANTHROPIC_API_KEY must be available (env or keyring)");

        let result = test_connectivity(&api_key, "claude-sonnet-4-6");
        match &result {
            Ok(text) => println!("claude-sonnet-4-6 OK: {}", text),
            Err(e) => println!("claude-sonnet-4-6 FAILED: {}", e),
        }
        assert!(result.is_ok(), "claude-sonnet-4-6 should be a valid model ID");
    }
}
