use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::json;

use crate::config::ClaudeConfig;
use crate::context::WindowContext;
use crate::dictionary::Dictionary;

use super::{build_system_prompt, Learning, ProcessResult, TextProcessor};

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
pub fn build_request_body(model: &str, system_prompt: &str, user_message: &str, tools: Option<&serde_json::Value>) -> serde_json::Value {
    let mut body = json!({
        "model": model,
        "max_tokens": 4096,
        "system": system_prompt,
        "messages": [
            {
                "role": "user",
                "content": user_message
            }
        ]
    });
    if let Some(tools) = tools {
        body["tools"] = tools.clone();
    }
    body
}

/// Define learning tools for Claude tool_use.
pub fn learning_tools() -> serde_json::Value {
    json!([
        {
            "name": "learn_term",
            "description": "Record a term that was misrecognized by speech-to-text. Use when you correct a specific word or phrase that the user likely uses regularly.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "from": { "type": "string", "description": "The misrecognized form" },
                    "to": { "type": "string", "description": "The correct form" }
                },
                "required": ["from", "to"]
            }
        },
        {
            "name": "learn_context",
            "description": "Record contextual information about the user, their work, or domain. Use when you discover something that would help process future voice inputs more accurately.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "category": {
                        "type": "string",
                        "enum": ["user_profile", "domain", "project", "workflow", "other"],
                        "description": "Category of the context information"
                    },
                    "content": {
                        "type": "string",
                        "description": "The information to remember"
                    }
                },
                "required": ["category", "content"]
            }
        }
    ])
}

/// Parse a Claude response into a ProcessResult, extracting text and tool_use learnings.
pub fn parse_process_result(resp: &serde_json::Value) -> Result<ProcessResult> {
    let content = resp["content"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("missing content array"))?;

    let mut text = String::new();
    let mut learnings = Vec::new();

    for block in content {
        match block["type"].as_str() {
            Some("text") => {
                if let Some(t) = block["text"].as_str() {
                    if !text.is_empty() {
                        text.push(' ');
                    }
                    text.push_str(t);
                }
            }
            Some("tool_use") => {
                let name = block["name"].as_str().unwrap_or("");
                let input = &block["input"];
                match name {
                    "learn_term" => {
                        if let (Some(from), Some(to)) =
                            (input["from"].as_str(), input["to"].as_str())
                        {
                            learnings.push(Learning::Term {
                                from: from.to_string(),
                                to: to.to_string(),
                            });
                        }
                    }
                    "learn_context" => {
                        if let (Some(cat), Some(cont)) =
                            (input["category"].as_str(), input["content"].as_str())
                        {
                            learnings.push(Learning::Context {
                                category: cat.to_string(),
                                content: cont.to_string(),
                            });
                        }
                    }
                    _ => {
                        tracing::warn!("Unknown tool_use: {}", name);
                    }
                }
            }
            _ => {}
        }
    }

    if text.is_empty() {
        anyhow::bail!("No text content in Claude response");
    }

    Ok(ProcessResult {
        text: text.trim().to_string(),
        learnings,
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
        memory_context: &str,
    ) -> Result<ProcessResult> {
        let system_prompt = build_system_prompt(context, dictionary, memory_context);
        let tools = learning_tools();
        let body = build_request_body(
            &self.model,
            &system_prompt,
            &format!("Clean up this speech-to-text output:\n\n{}", raw_text),
            Some(&tools),
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
        let result = parse_process_result(&resp)?;

        tracing::info!(
            "Claude processed: {} (learnings: {})",
            result.text,
            result.learnings.len()
        );
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_request_body_structure() {
        let body = build_request_body("claude-sonnet-4-6", "system prompt", "hello", None);
        assert_eq!(body["model"], "claude-sonnet-4-6");
        assert_eq!(body["max_tokens"], 4096);
        assert_eq!(body["system"], "system prompt");
        let messages = body["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(messages[0]["content"], "hello");
        assert!(body.get("tools").is_none() || body["tools"].is_null());
    }

    #[test]
    fn test_build_request_body_with_tools() {
        let tools = learning_tools();
        let body = build_request_body("claude-sonnet-4-6", "system", "hello", Some(&tools));
        let tools_arr = body["tools"].as_array().unwrap();
        assert_eq!(tools_arr.len(), 2);
        assert_eq!(tools_arr[0]["name"], "learn_term");
        assert_eq!(tools_arr[1]["name"], "learn_context");
    }

    #[test]
    fn test_build_request_body_without_tools() {
        let body = build_request_body("claude-sonnet-4-6", "system", "hello", None);
        // tools key should not exist or be null
        assert!(body.get("tools").is_none() || body["tools"].is_null());
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

    #[test]
    fn test_parse_process_result_text_only() {
        let resp: serde_json::Value = serde_json::from_str(r#"
        {
            "content": [
                { "type": "text", "text": "Hello, world!" }
            ],
            "stop_reason": "end_turn"
        }
        "#).unwrap();
        let result = parse_process_result(&resp).unwrap();
        assert_eq!(result.text, "Hello, world!");
        assert!(result.learnings.is_empty());
    }

    #[test]
    fn test_parse_process_result_with_learn_term() {
        let resp: serde_json::Value = serde_json::from_str(r#"
        {
            "content": [
                { "type": "text", "text": "Rustで書かれたコード" },
                {
                    "type": "tool_use",
                    "id": "toolu_123",
                    "name": "learn_term",
                    "input": { "from": "ラスト", "to": "Rust" }
                }
            ],
            "stop_reason": "end_turn"
        }
        "#).unwrap();
        let result = parse_process_result(&resp).unwrap();
        assert_eq!(result.text, "Rustで書かれたコード");
        assert_eq!(result.learnings.len(), 1);
        match &result.learnings[0] {
            super::Learning::Term { from, to } => {
                assert_eq!(from, "ラスト");
                assert_eq!(to, "Rust");
            }
            _ => panic!("Expected Learning::Term"),
        }
    }

    #[test]
    fn test_parse_process_result_with_learn_context() {
        let resp: serde_json::Value = serde_json::from_str(r#"
        {
            "content": [
                { "type": "text", "text": "処理済みテキスト" },
                {
                    "type": "tool_use",
                    "id": "toolu_456",
                    "name": "learn_context",
                    "input": {
                        "category": "project",
                        "content": "User works on a Rust project called koe"
                    }
                }
            ],
            "stop_reason": "end_turn"
        }
        "#).unwrap();
        let result = parse_process_result(&resp).unwrap();
        assert_eq!(result.text, "処理済みテキスト");
        assert_eq!(result.learnings.len(), 1);
        match &result.learnings[0] {
            super::Learning::Context { category, content } => {
                assert_eq!(category, "project");
                assert_eq!(content, "User works on a Rust project called koe");
            }
            _ => panic!("Expected Learning::Context"),
        }
    }

    #[test]
    fn test_parse_process_result_multiple_learnings() {
        let resp: serde_json::Value = serde_json::from_str(r#"
        {
            "content": [
                { "type": "text", "text": "koeプロジェクトのRustコード" },
                {
                    "type": "tool_use",
                    "id": "toolu_1",
                    "name": "learn_term",
                    "input": { "from": "こえ", "to": "koe" }
                },
                {
                    "type": "tool_use",
                    "id": "toolu_2",
                    "name": "learn_term",
                    "input": { "from": "ラスト", "to": "Rust" }
                },
                {
                    "type": "tool_use",
                    "id": "toolu_3",
                    "name": "learn_context",
                    "input": {
                        "category": "domain",
                        "content": "User frequently discusses voice recognition technology"
                    }
                }
            ],
            "stop_reason": "end_turn"
        }
        "#).unwrap();
        let result = parse_process_result(&resp).unwrap();
        assert_eq!(result.text, "koeプロジェクトのRustコード");
        assert_eq!(result.learnings.len(), 3);
    }

    #[test]
    fn test_parse_process_result_no_text() {
        let resp: serde_json::Value = serde_json::from_str(r#"
        {
            "content": [
                {
                    "type": "tool_use",
                    "id": "toolu_1",
                    "name": "learn_term",
                    "input": { "from": "foo", "to": "bar" }
                }
            ],
            "stop_reason": "end_turn"
        }
        "#).unwrap();
        let result = parse_process_result(&resp);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No text content"));
    }

    #[test]
    fn test_build_system_prompt_with_memory() {
        use crate::context::WindowContext;
        use crate::dictionary::Dictionary;

        let ctx = WindowContext {
            window_title: String::new(),
            app_name: String::new(),
            window_class: String::new(),
        };
        let dict = Dictionary::default();
        let memory = "Term: ラスト -> Rust\nContext: User works on koe project";

        let prompt = super::build_system_prompt(&ctx, &dict, memory);
        assert!(prompt.contains("Learned context from previous interactions:"));
        assert!(prompt.contains("ラスト -> Rust"));
        assert!(prompt.contains("User works on koe project"));
    }

    #[test]
    fn test_build_system_prompt_empty_memory() {
        use crate::context::WindowContext;
        use crate::dictionary::Dictionary;

        let ctx = WindowContext {
            window_title: String::new(),
            app_name: String::new(),
            window_class: String::new(),
        };
        let dict = Dictionary::default();

        let prompt = super::build_system_prompt(&ctx, &dict, "");
        assert!(!prompt.contains("Learned context from previous interactions:"));
        // But should still contain learning instructions
        assert!(prompt.contains("You have access to learning tools"));
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
