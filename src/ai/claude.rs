use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::json;

use crate::config::ClaudeConfig;
use crate::context::WindowContext;
use crate::dictionary::Dictionary;

use super::{build_system_prompt, ConsolidationResult, Learning, ProcessResult, TextProcessor};

pub const ANTHROPIC_API_URL: &str = "https://api.anthropic.com/v1/messages";
pub const ANTHROPIC_VERSION: &str = "2023-06-01";

/// Resolve the Messages API endpoint, honoring a custom base URL.
pub fn messages_url(config: &ClaudeConfig) -> Result<String> {
    match &config.base_url {
        Some(base_url) => anthropic_messages_url(base_url),
        None => Ok(ANTHROPIC_API_URL.to_string()),
    }
}

/// Append the Messages API path to a custom base URL, as the Anthropic SDKs do.
fn anthropic_messages_url(base_url: &str) -> Result<String> {
    let url = reqwest::Url::parse(base_url)
        .with_context(|| format!("invalid ai.claude.base_url: {:?}", base_url))?;
    // The API key travels with every request; allow plain HTTP only to this machine.
    let loopback = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    if !(url.scheme() == "https" || url.scheme() == "http" && loopback) {
        anyhow::bail!(
            "ai.claude.base_url must use https (http is allowed only for localhost): {:?}",
            base_url
        );
    }
    if url.query().is_some() || url.fragment().is_some() {
        anyhow::bail!(
            "ai.claude.base_url must not have a query or fragment: {:?}",
            base_url
        );
    }
    Ok(format!(
        "{}/v1/messages",
        url.as_str().trim_end_matches('/')
    ))
}

/// Read the response body as JSON, enforcing a maximum size limit.
///
/// Uses chunk-based streaming so that a response without Content-Length
/// cannot cause unbounded memory allocation before the size check.
pub(crate) async fn read_response_json(
    mut response: reqwest::Response,
    max_bytes: usize,
) -> Result<serde_json::Value> {
    // Fast-reject when Content-Length is available.
    if let Some(len) = response.content_length() {
        if len > max_bytes as u64 {
            anyhow::bail!(
                "API response too large (Content-Length: {} bytes, limit: {} bytes)",
                len,
                max_bytes
            );
        }
    }

    // Stream chunks, enforcing the limit incrementally.
    let mut buf = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .context("reading API response chunk")?
    {
        if buf.len() + chunk.len() > max_bytes {
            anyhow::bail!(
                "API response too large (exceeded limit of {} bytes)",
                max_bytes
            );
        }
        buf.extend_from_slice(&chunk);
    }

    serde_json::from_slice(&buf).context("parsing API response JSON")
}

fn is_loopback_http(endpoint: &str) -> bool {
    reqwest::Url::parse(endpoint).is_ok_and(|url| {
        url.scheme() == "http"
            && matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))
    })
}

/// Never follow redirects: reqwest keeps `x-api-key` on a redirect to another
/// host, so following one could send the key past the `base_url` checks.
fn http_client_builder(endpoint: &str) -> reqwest::ClientBuilder {
    let builder = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none());
    // Plain HTTP is permitted only for local traffic. An environment proxy
    // would send the API key and transcript outside that boundary.
    if is_loopback_http(endpoint) {
        builder.no_proxy()
    } else {
        builder
    }
}

pub struct ClaudeProcessor {
    endpoint: String,
    api_key: String,
    model: String,
    client: reqwest::Client,
    max_response_bytes: usize,
}

impl ClaudeProcessor {
    pub fn new(config: &ClaudeConfig, max_response_bytes: usize) -> Result<Self> {
        let endpoint = messages_url(config)?;
        let api_key = crate::config::resolve_api_key(&config.api_key_env)?;

        let client = http_client_builder(&endpoint)
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .context("building Claude HTTP client")?;
        Ok(Self {
            endpoint,
            api_key,
            model: config.model.clone(),
            client,
            max_response_bytes,
        })
    }

    async fn send_message(
        &self,
        endpoint: &str,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        let response = self
            .client
            .post(endpoint)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .json(body)
            .send()
            .await
            .context("sending request to Claude API")?;
        if !response.status().is_success() {
            let status = response.status();
            let host = response.url().host_str().unwrap_or_default().to_string();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Claude API error ({}) from {}: {}", status, host, body);
        }
        read_response_json(response, self.max_response_bytes).await
    }

    // The production entry point always supplies self.endpoint. Keeping the
    // transport boundary here lets regression tests exercise the entire exchange.
    async fn process_at_endpoint(
        &self,
        raw_text: &str,
        context: &WindowContext,
        dictionary: &Dictionary,
        memory_context: &str,
        endpoint: &str,
    ) -> Result<ProcessResult> {
        let body = build_request_body(
            &self.model,
            &build_system_prompt(context, dictionary, memory_context),
            &format!("Clean up this speech-to-text output:\n\n{}", raw_text),
            Some(&learning_tools()),
        );
        let response = self.send_message(endpoint, &body).await?;
        let mut result = parse_content_blocks(&response)?;
        if result.text.is_empty() {
            let followup = build_learning_followup(&body, &response)?;
            let response = self.send_message(endpoint, &followup).await?;
            // One follow-up only; tool_choice=none requests the final text and
            // bounds both latency and cost. Do not expose pending learnings on failure.
            if response["content"]
                .as_array()
                .is_some_and(|blocks| blocks.iter().any(|b| b["type"] == "tool_use"))
            {
                anyhow::bail!("Claude requested another tool after learning follow-up");
            }
            result.text = parse_process_result(&response)?.text;
        }
        Ok(result)
    }
}

fn apply_model_settings(body: &mut serde_json::Value) {
    if body["model"] == "claude-sonnet-5-5" {
        // Voice formatting is latency-sensitive. Sonnet 5.5 otherwise enables
        // up-front adaptive thinking by default, unlike Sonnet 4.6.
        body["thinking"] = json!({"type": "between_tools"});
        body["output_config"] = json!({"effort": "low"});
    }
}

/// Build the JSON request body for the Claude Messages API.
pub fn build_request_body(
    model: &str,
    system_prompt: &str,
    user_message: &str,
    tools: Option<&serde_json::Value>,
) -> serde_json::Value {
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
    apply_model_settings(&mut body);
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

fn parse_learning(block: &serde_json::Value) -> Option<Learning> {
    let input = &block["input"];
    match block["name"].as_str()? {
        "learn_term" => {
            let from = input["from"].as_str()?;
            let to = input["to"].as_str()?;
            if from.trim().is_empty() || to.trim().is_empty() {
                return None;
            }
            Some(Learning::Term {
                from: from.into(),
                to: to.into(),
            })
        }
        "learn_context" => {
            let category = input["category"].as_str()?;
            let content = input["content"].as_str()?;
            if !matches!(
                category,
                "user_profile" | "domain" | "project" | "workflow" | "other"
            ) || content.trim().is_empty()
            {
                return None;
            }
            Some(Learning::Context {
                category: category.into(),
                content: content.into(),
            })
        }
        _ => None,
    }
}

fn build_learning_followup(
    original: &serde_json::Value,
    response: &serde_json::Value,
) -> Result<serde_json::Value> {
    let content = response["content"]
        .as_array()
        .context("missing content array")?;
    let mut results = Vec::new();
    let mut ids = std::collections::HashSet::new();
    for block in content.iter().filter(|b| b["type"] == "tool_use") {
        let id = block["id"]
            .as_str()
            .filter(|id| !id.is_empty())
            .context("missing learning tool_use id")?;
        if !ids.insert(id) {
            anyhow::bail!("duplicate learning tool_use id");
        }
        let accepted = parse_learning(block).is_some();
        results.push(json!({
            "type": "tool_result",
            "tool_use_id": id,
            "is_error": !accepted,
            "content": if accepted {
                "Learning accepted for saving after the final text is returned. Now output only the corrected speech-to-text text."
            } else {
                "Unsupported or invalid learning request. Output the corrected speech-to-text text without more tool calls."
            }
        }));
    }
    if results.is_empty() {
        anyhow::bail!("No text content in Claude response");
    }
    let mut followup = original.clone();
    let messages = followup["messages"]
        .as_array_mut()
        .context("missing messages array")?;
    // Preserve the full content verbatim, including signed thinking blocks.
    messages.push(json!({"role": "assistant", "content": content}));
    messages.push(json!({"role": "user", "content": results}));
    followup["tool_choice"] = json!({"type": "none"});
    Ok(followup)
}

/// Parse content without requiring text yet: a tool-only turn must be resumed.
fn parse_content_blocks(resp: &serde_json::Value) -> Result<ProcessResult> {
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
                if let Some(learning) = parse_learning(block) {
                    learnings.push(learning);
                }
            }
            _ => {}
        }
    }

    Ok(ProcessResult {
        text: text.trim().to_string(),
        learnings,
    })
}

/// Parse a completed response; callers must never type an empty result.
pub fn parse_process_result(resp: &serde_json::Value) -> Result<ProcessResult> {
    let result = parse_content_blocks(resp)?;
    if result.text.is_empty() {
        anyhow::bail!("No text content in Claude response");
    }
    Ok(result)
}

/// Extract text from a Claude Messages API response.
pub fn parse_response_text(resp: &serde_json::Value) -> Result<String> {
    let text = resp["content"]
        .as_array()
        .context("missing content array")?
        .iter()
        .filter(|block| block["type"] == "text")
        .filter_map(|block| block["text"].as_str())
        .collect::<Vec<_>>()
        .join(" ");
    if text.trim().is_empty() {
        anyhow::bail!("No text content in Claude response");
    }
    Ok(text)
}

/// Parse the consolidation response containing JSON terms and Markdown context.
pub fn parse_consolidation_response(text: &str) -> Result<ConsolidationResult> {
    use std::collections::HashMap;

    let mut terms: HashMap<String, String> = HashMap::new();
    let mut context_markdown = String::new();

    // Extract JSON block for terms
    if let Some(json_start) = text.find("```json") {
        let json_content = &text[json_start + 7..];
        if let Some(json_end) = json_content.find("```") {
            let json_str = json_content[..json_end].trim();
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(json_str) {
                if let Some(terms_obj) = parsed.get("terms").and_then(|t| t.as_object()) {
                    for (k, v) in terms_obj {
                        if let Some(val) = v.as_str() {
                            terms.insert(k.clone(), val.to_string());
                        }
                    }
                }
            }
        }
    }

    // Extract Markdown block for context
    // Find the text after the JSON code block closes
    let search_text = if let Some(json_start_pos) = text.find("```json") {
        let after_json_tag = &text[json_start_pos + 7..];
        if let Some(close_pos) = after_json_tag.find("```") {
            let after_close = &after_json_tag[close_pos + 3..];
            // Skip optional newline after closing ```
            after_close.strip_prefix('\n').unwrap_or(after_close)
        } else {
            text
        }
    } else {
        text
    };

    if let Some(md_start) = search_text.find("```markdown") {
        let md_content = &search_text[md_start + 11..];
        if let Some(md_end) = md_content.find("```") {
            context_markdown = md_content[..md_end].trim().to_string();
        }
    }

    // Also handle case where context is written directly (without code block)
    // by looking for ## headers after the JSON block
    if context_markdown.is_empty() {
        let mut in_context = false;
        let mut context_lines = Vec::new();
        for line in search_text.lines() {
            if line.starts_with("## ") {
                in_context = true;
            }
            if in_context {
                context_lines.push(line);
            }
        }
        if !context_lines.is_empty() {
            context_markdown = context_lines.join("\n");
        }
    }

    Ok(ConsolidationResult {
        terms,
        context_markdown,
    })
}

/// Send a minimal test request to verify API key and model.
/// Returns the model's response text on success.
pub fn test_connectivity(endpoint: &str, api_key: &str, model: &str) -> Result<String> {
    let builder = reqwest::blocking::Client::builder()
        // See http_client_builder.
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(15));
    let builder = if is_loopback_http(endpoint) {
        builder.no_proxy()
    } else {
        builder
    };
    let client = builder.build().context("building HTTP client")?;

    let mut body = json!({
        "model": model,
        "max_tokens": 64,
        "messages": [
            {
                "role": "user",
                "content": "Reply with exactly: OK"
            }
        ]
    });
    apply_model_settings(&mut body);

    let response = client
        .post(endpoint)
        .header("x-api-key", api_key)
        .header("anthropic-version", ANTHROPIC_VERSION)
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .context("sending request to Claude API")?;

    let status = response.status();
    if !status.is_success() {
        let host = response.url().host_str().unwrap_or_default().to_string();
        let body = response.text().unwrap_or_default();
        anyhow::bail!("API error ({}) from {}: {}", status, host, body);
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
        let result = self
            .process_at_endpoint(
                raw_text,
                context,
                dictionary,
                memory_context,
                &self.endpoint,
            )
            .await?;

        tracing::info!(
            "Claude processed: {} (learnings: {})",
            result.text,
            result.learnings.len()
        );
        Ok(result)
    }

    async fn consolidate_memory(
        &self,
        memory_content: &str,
    ) -> Result<Option<ConsolidationResult>> {
        let prompt = super::build_consolidation_prompt(memory_content);

        // Use build_request_body without tools (consolidation doesn't need learning tools)
        let body = build_request_body(
            &self.model,
            "You are a data consolidation assistant. Follow the output format exactly.",
            &prompt,
            None,
        );

        let response = self
            .client
            .post(&self.endpoint)
            // Consolidation can summarize a large memory; keep a bounded but
            // longer deadline than interactive dictation requests.
            .timeout(std::time::Duration::from_secs(120))
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .context("sending consolidation request to Claude")?;

        if !response.status().is_success() {
            let status = response.status();
            let host = response.url().host_str().unwrap_or_default().to_string();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!(
                "Claude consolidation error ({}) from {}: {}",
                status,
                host,
                body
            );
        }

        let resp = read_response_json(response, self.max_response_bytes).await?;
        let text = parse_response_text(&resp)?;

        let result = parse_consolidation_response(&text)?;
        Ok(Some(result))
    }
}

#[cfg(test)]
mod exchange_tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::time::{Duration, Instant};

    fn accept(listener: &TcpListener) -> TcpStream {
        let deadline = Instant::now() + Duration::from_secs(5);
        let stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(e)
                    if e.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < deadline =>
                {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(e) => panic!("expected API request: {e}"),
            }
        };
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream
    }

    /// Read one request and return its body.
    fn read_request(stream: &mut TcpStream) -> Vec<u8> {
        let mut bytes = Vec::new();
        let (header_end, length) = loop {
            let mut buffer = [0; 4096];
            let count = stream.read(&mut buffer).unwrap();
            assert!(count > 0);
            bytes.extend_from_slice(&buffer[..count]);
            if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&bytes[..end]);
                let length = headers
                    .lines()
                    .find_map(|line| {
                        let (key, value) = line.split_once(':')?;
                        key.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().unwrap())
                    })
                    .unwrap();
                break (end + 4, length);
            }
        };
        while bytes.len() < header_end + length {
            let mut buffer = [0; 4096];
            let count = stream.read(&mut buffer).unwrap();
            assert!(count > 0);
            bytes.extend_from_slice(&buffer[..count]);
        }
        bytes[header_end..header_end + length].to_vec()
    }

    fn mock_api(
        responses: Vec<(u16, serde_json::Value)>,
    ) -> (String, std::thread::JoinHandle<Vec<serde_json::Value>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}/v1/messages", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let handle = std::thread::spawn(move || {
            let mut requests = Vec::new();
            for (status, response) in responses {
                let mut stream = accept(&listener);
                requests.push(serde_json::from_slice(&read_request(&mut stream)).unwrap());
                if status == 0 {
                    std::thread::sleep(Duration::from_millis(500));
                    continue;
                }
                let body = response.to_string();
                write!(stream, "HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
            requests
        });
        (endpoint, handle)
    }

    fn processor() -> ClaudeProcessor {
        ClaudeProcessor {
            endpoint: ANTHROPIC_API_URL.into(),
            api_key: "synthetic-test-key".into(),
            model: "claude-sonnet-5-5".into(),
            client: http_client_builder(ANTHROPIC_API_URL)
                .no_proxy()
                .build()
                .unwrap(),
            max_response_bytes: 1024 * 1024,
        }
    }

    /// Answer one request with a 307 to a second listener, and return the first
    /// endpoint and the second listener, which must never receive a connection.
    fn redirect_to_another_host() -> (String, TcpListener, std::thread::JoinHandle<()>) {
        let target = TcpListener::bind("127.0.0.1:0").unwrap();
        target.set_nonblocking(true).unwrap();
        let location = format!("http://{}/v1/messages", target.local_addr().unwrap());
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint = format!("http://{}/v1/messages", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let mut stream = accept(&listener);
            read_request(&mut stream);
            write!(stream, "HTTP/1.1 307 Temporary Redirect\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
        });
        (endpoint, target, handle)
    }

    fn assert_never_contacted(target: &TcpListener) {
        assert!(matches!(
            target.accept(),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock
        ));
    }

    #[tokio::test]
    async fn processing_does_not_follow_redirects_with_the_api_key() {
        let (endpoint, target, server) = redirect_to_another_host();
        let mut processor = processor();
        // The target never answers; time out instead of hanging if it is reached.
        processor.client = http_client_builder(&endpoint)
            .no_proxy()
            .timeout(Duration::from_secs(2))
            .build()
            .unwrap();
        let error = processor
            .process_at_endpoint(
                "合成入力",
                &WindowContext::default(),
                &Dictionary::default(),
                "",
                &endpoint,
            )
            .await
            .unwrap_err();
        server.join().unwrap();
        assert!(error.to_string().contains("307"), "{error}");
        assert_never_contacted(&target);
    }

    #[test]
    fn connection_test_does_not_follow_redirects_with_the_api_key() {
        let (endpoint, target, server) = redirect_to_another_host();
        let error =
            test_connectivity(&endpoint, "synthetic-test-key", "claude-sonnet-5-5").unwrap_err();
        server.join().unwrap();
        assert!(error.to_string().contains("307"), "{error}");
        assert_never_contacted(&target);
    }

    // Run the real entry points in child processes so proxy environment changes
    // cannot race with other tests or depend on the developer's proxy settings.
    #[test]
    fn loopback_proxy_child() {
        let Ok(mode) = std::env::var("KOE_TEST_PROXY_MODE") else {
            return;
        };
        let base_url = std::env::var("KOE_TEST_BASE_URL").unwrap();
        let config = ClaudeConfig {
            api_key_env: "KOE_TEST_SYNTHETIC_KEY".into(),
            model: "claude-sonnet-5-5".into(),
            base_url: Some(base_url),
        };
        if mode == "processing" {
            let runtime = tokio::runtime::Runtime::new().unwrap();
            let result = runtime.block_on(async {
                ClaudeProcessor::new(&config, 1024 * 1024)
                    .unwrap()
                    .process(
                        "synthetic speech",
                        &WindowContext::default(),
                        &Dictionary::default(),
                        "",
                    )
                    .await
                    .unwrap()
            });
            assert_eq!(result.text, "OK");
        } else {
            assert_eq!(
                test_connectivity(
                    &messages_url(&config).unwrap(),
                    "synthetic-proxy-test-key",
                    &config.model,
                )
                .unwrap(),
                "OK"
            );
        }
    }

    fn optional_api() -> (String, std::thread::JoinHandle<bool>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let handle = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(3);
            loop {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream
                            .set_read_timeout(Some(Duration::from_secs(3)))
                            .unwrap();
                        read_request(&mut stream);
                        let body = r#"{"content":[{"type":"text","text":"OK"}]}"#;
                        write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
                        return true;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        if Instant::now() >= deadline {
                            return false;
                        }
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("accepting synthetic request: {error}"),
                }
            }
        });
        (base_url, handle)
    }

    fn assert_loopback_bypasses_environment_proxy(mode: &str) {
        let (origin, origin_server) = optional_api();
        let (proxy, proxy_server) = optional_api();
        let mut child = std::process::Command::new(std::env::current_exe().unwrap());
        child.args([
            "--exact",
            "ai::claude::exchange_tests::loopback_proxy_child",
            "--nocapture",
        ]);
        for name in [
            "HTTP_PROXY",
            "http_proxy",
            "HTTPS_PROXY",
            "https_proxy",
            "ALL_PROXY",
            "all_proxy",
        ] {
            child.env(name, &proxy);
        }
        child.env_remove("NO_PROXY").env_remove("no_proxy");
        let output = child
            .env("KOE_TEST_PROXY_MODE", mode)
            .env("KOE_TEST_BASE_URL", origin)
            .env("KOE_TEST_SYNTHETIC_KEY", "synthetic-proxy-test-key")
            .output()
            .unwrap();
        let origin_contacted = origin_server.join().unwrap();
        let proxy_contacted = proxy_server.join().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        assert!(
            !proxy_contacted,
            "loopback HTTP sent the request through an environment proxy"
        );
        assert!(
            origin_contacted,
            "loopback HTTP did not reach the local origin"
        );
    }

    #[test]
    fn processing_loopback_bypasses_environment_proxy() {
        assert_loopback_bypasses_environment_proxy("processing");
    }

    #[test]
    fn connectivity_loopback_bypasses_environment_proxy() {
        assert_loopback_bypasses_environment_proxy("connectivity");
    }

    fn tool_turn() -> serde_json::Value {
        json!({"stop_reason":"tool_use", "content":[
            {"type":"thinking", "thinking":"", "signature":"synthetic-signature"},
            {"type":"tool_use", "id":"term-1", "name":"learn_term", "input":{"from":"ラスト", "to":"Rust"}},
            {"type":"tool_use", "id":"context-1", "name":"learn_context", "input":{"category":"project", "content":"Synthetic Rust project"}}
        ]})
    }

    #[tokio::test]
    async fn tool_only_response_resumes_and_retains_each_learning_once() {
        let initial = tool_turn();
        let (endpoint, server) = mock_api(vec![
            (200, initial.clone()),
            (
                200,
                json!({"content":[
                    {"type":"thinking", "thinking":"", "signature":"final-signature"},
                    {"type":"text", "text":"Rustで実装します。"}
                ]}),
            ),
        ]);
        let result = processor()
            .process_at_endpoint(
                "ラストで実装します",
                &WindowContext::default(),
                &Dictionary::default(),
                "",
                &endpoint,
            )
            .await
            .unwrap();
        assert_eq!(result.text, "Rustで実装します。");
        assert_eq!(result.learnings.len(), 2);
        assert!(
            matches!(&result.learnings[0], Learning::Term { from, to } if from == "ラスト" && to == "Rust")
        );
        assert!(
            matches!(&result.learnings[1], Learning::Context { category, .. } if category == "project")
        );
        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[1]["messages"][0], requests[0]["messages"][0]);
        assert_eq!(requests[1]["messages"][1]["content"], initial["content"]);
        let results = requests[1]["messages"][2]["content"].as_array().unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0]["tool_use_id"], "term-1");
        assert_eq!(results[1]["tool_use_id"], "context-1");
        assert_eq!(requests[1]["tool_choice"]["type"], "none");
        assert_eq!(requests[1]["thinking"], requests[0]["thinking"]);
        assert_eq!(requests[1]["output_config"], requests[0]["output_config"]);
    }

    #[tokio::test]
    async fn complete_text_and_learning_still_need_only_one_request() {
        let mut response = tool_turn();
        response["content"]
            .as_array_mut()
            .unwrap()
            .push(json!({"type":"text","text":"Rustで実装します。"}));
        let (endpoint, server) = mock_api(vec![(200, response)]);
        let result = processor()
            .process_at_endpoint(
                "ラストで実装します",
                &WindowContext::default(),
                &Dictionary::default(),
                "",
                &endpoint,
            )
            .await
            .unwrap();
        assert_eq!(result.text, "Rustで実装します。");
        assert_eq!(result.learnings.len(), 2);
        assert_eq!(server.join().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn failed_followup_does_not_return_pending_learnings() {
        let (endpoint, server) = mock_api(vec![
            (200, tool_turn()),
            (500, json!({"error":{"type":"api_error"}})),
        ]);
        let result = processor()
            .process_at_endpoint(
                "合成入力",
                &WindowContext::default(),
                &Dictionary::default(),
                "",
                &endpoint,
            )
            .await;
        assert!(result.is_err());
        assert_eq!(server.join().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn stalled_followup_times_out_without_returning_learnings() {
        let (endpoint, server) = mock_api(vec![(200, tool_turn()), (0, json!(null))]);
        let mut processor = processor();
        processor.client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_millis(250))
            .build()
            .unwrap();
        let error = processor
            .process_at_endpoint(
                "合成入力",
                &WindowContext::default(),
                &Dictionary::default(),
                "",
                &endpoint,
            )
            .await
            .unwrap_err();
        assert!(error.chain().any(|cause| cause
            .downcast_ref::<reqwest::Error>()
            .is_some_and(|error| error.is_timeout())));
        assert_eq!(server.join().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn another_tool_turn_is_rejected_without_a_third_request() {
        let (endpoint, server) = mock_api(vec![(200, tool_turn()), (200, tool_turn())]);
        let result = processor()
            .process_at_endpoint(
                "合成入力",
                &WindowContext::default(),
                &Dictionary::default(),
                "",
                &endpoint,
            )
            .await;
        assert!(result.unwrap_err().to_string().contains("another tool"));
        assert_eq!(server.join().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn empty_response_without_tools_is_not_retried() {
        let (endpoint, server) = mock_api(vec![(
            200,
            json!({"content":[{"type":"text","text":"  "}]}),
        )]);
        let result = processor()
            .process_at_endpoint(
                "合成入力",
                &WindowContext::default(),
                &Dictionary::default(),
                "",
                &endpoint,
            )
            .await;
        assert!(result.is_err());
        assert_eq!(server.join().unwrap().len(), 1);
    }

    #[test]
    fn text_extraction_skips_thinking_and_keeps_all_text_blocks() {
        let response = json!({"content":[
            {"type":"thinking","thinking":"","signature":"synthetic"},
            {"type":"text","text":"第一文。"},
            {"type":"text","text":"第二文。"}
        ]});
        assert_eq!(parse_response_text(&response).unwrap(), "第一文。 第二文。");
    }

    #[test]
    fn sonnet_55_uses_latency_settings_without_changing_older_models() {
        let modern = build_request_body("claude-sonnet-5-5", "system", "input", None);
        assert_eq!(modern["thinking"], json!({"type":"between_tools"}));
        assert_eq!(modern["output_config"], json!({"effort":"low"}));
        let older = build_request_body("claude-sonnet-4-6", "system", "input", None);
        assert!(older.get("thinking").is_none());
        assert!(older.get("output_config").is_none());
    }

    fn with_base_url(base_url: Option<&str>) -> ClaudeConfig {
        ClaudeConfig {
            api_key_env: "SYNTHETIC_KEY".into(),
            model: "claude-sonnet-5-5".into(),
            base_url: base_url.map(Into::into),
        }
    }

    #[test]
    fn messages_url_defaults_to_the_anthropic_api() {
        assert_eq!(
            messages_url(&with_base_url(None)).unwrap(),
            ANTHROPIC_API_URL
        );
    }

    #[test]
    fn base_url_gets_the_messages_path() {
        for (base_url, expected) in [
            (
                "https://llm.example.com",
                "https://llm.example.com/v1/messages",
            ),
            (
                "https://llm.example.com/",
                "https://llm.example.com/v1/messages",
            ),
            (
                "https://llm.example.com/anthropic/",
                "https://llm.example.com/anthropic/v1/messages",
            ),
            ("http://localhost:8080", "http://localhost:8080/v1/messages"),
        ] {
            assert_eq!(
                messages_url(&with_base_url(Some(base_url))).unwrap(),
                expected
            );
        }
    }

    #[test]
    fn base_url_rejects_unsafe_or_ambiguous_urls() {
        for base_url in [
            "http://llm.example.com",
            "llm.example.com",
            "ftp://llm.example.com",
            "https://llm.example.com/?key=x",
            "https://llm.example.com/#x",
        ] {
            assert!(
                messages_url(&with_base_url(Some(base_url))).is_err(),
                "{base_url}"
            );
        }
    }

    #[test]
    fn invalid_learning_is_reported_without_accepting_it() {
        let original = build_request_body(
            "claude-sonnet-5-5",
            "system",
            "input",
            Some(&learning_tools()),
        );
        let response = json!({"content":[{"type":"tool_use","id":"bad","name":"learn_term","input":{"from":""}}]});
        assert!(parse_content_blocks(&response)
            .unwrap()
            .learnings
            .is_empty());
        let next = build_learning_followup(&original, &response).unwrap();
        assert_eq!(next["messages"][2]["content"][0]["is_error"], true);
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
        let resp: serde_json::Value = serde_json::from_str(
            r#"
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
        "#,
        )
        .unwrap();
        assert_eq!(parse_response_text(&resp).unwrap(), "OK");
    }

    #[test]
    fn test_parse_response_text_multi_block() {
        let resp: serde_json::Value = serde_json::from_str(
            r#"
        {
            "content": [
                { "type": "text", "text": "First block" },
                { "type": "text", "text": "Second block" }
            ]
        }
        "#,
        )
        .unwrap();
        assert_eq!(
            parse_response_text(&resp).unwrap(),
            "First block Second block"
        );
    }

    #[test]
    fn test_parse_response_text_empty_content() {
        let resp: serde_json::Value = serde_json::from_str(
            r#"
        { "content": [] }
        "#,
        )
        .unwrap();
        assert!(parse_response_text(&resp).is_err());
    }

    #[test]
    fn test_parse_response_text_unexpected_format() {
        let resp: serde_json::Value = serde_json::from_str(
            r#"
        { "error": "something went wrong" }
        "#,
        )
        .unwrap();
        assert!(parse_response_text(&resp).is_err());
    }

    #[test]
    fn test_parse_process_result_text_only() {
        let resp: serde_json::Value = serde_json::from_str(
            r#"
        {
            "content": [
                { "type": "text", "text": "Hello, world!" }
            ],
            "stop_reason": "end_turn"
        }
        "#,
        )
        .unwrap();
        let result = parse_process_result(&resp).unwrap();
        assert_eq!(result.text, "Hello, world!");
        assert!(result.learnings.is_empty());
    }

    #[test]
    fn test_parse_process_result_with_learn_term() {
        let resp: serde_json::Value = serde_json::from_str(
            r#"
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
        "#,
        )
        .unwrap();
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
        let resp: serde_json::Value = serde_json::from_str(
            r#"
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
        "#,
        )
        .unwrap();
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
        let resp: serde_json::Value = serde_json::from_str(
            r#"
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
        "#,
        )
        .unwrap();
        let result = parse_process_result(&resp).unwrap();
        assert_eq!(result.text, "koeプロジェクトのRustコード");
        assert_eq!(result.learnings.len(), 3);
    }

    #[test]
    fn test_parse_process_result_no_text() {
        let resp: serde_json::Value = serde_json::from_str(
            r#"
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
        "#,
        )
        .unwrap();
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

    #[test]
    fn test_parse_consolidation_response() {
        let text = r#"Here is the consolidated memory:

```json
{"terms": {"ラスト": "Rust", "クロード": "Claude"}}
```

```markdown
## domain
- ソフトウェア開発
- Linux デスクトップ
## user_profile
- Rust エンジニア
```
"#;
        let result = parse_consolidation_response(text).unwrap();
        assert_eq!(result.terms.len(), 2);
        assert_eq!(result.terms.get("ラスト").unwrap(), "Rust");
        assert_eq!(result.terms.get("クロード").unwrap(), "Claude");
        assert!(result.context_markdown.contains("## domain"));
        assert!(result.context_markdown.contains("- ソフトウェア開発"));
        assert!(result.context_markdown.contains("## user_profile"));
    }

    #[test]
    fn test_parse_consolidation_response_no_json() {
        let text = "No JSON block here, just text.\n## domain\n- something";
        let result = parse_consolidation_response(text).unwrap();
        assert!(result.terms.is_empty());
        assert!(result.context_markdown.contains("## domain"));
        assert!(result.context_markdown.contains("- something"));
    }

    #[test]
    fn test_parse_consolidation_response_direct_markdown() {
        let text = r#"```json
{"terms": {"a": "A"}}
```

## category1
- entry1
- entry2
## category2
- entry3
"#;
        let result = parse_consolidation_response(text).unwrap();
        assert_eq!(result.terms.len(), 1);
        assert_eq!(result.terms.get("a").unwrap(), "A");
        assert!(result.context_markdown.contains("## category1"));
        assert!(result.context_markdown.contains("- entry1"));
        assert!(result.context_markdown.contains("## category2"));
        assert!(result.context_markdown.contains("- entry3"));
    }

    #[test]
    fn test_build_consolidation_prompt() {
        let memory_content = "## 用語辞書\n- ラスト → Rust\n\n## domain\n- ソフトウェア開発";
        let prompt = crate::ai::build_consolidation_prompt(memory_content);
        assert!(prompt.contains("メモリデータ"));
        assert!(prompt.contains("ラスト → Rust"));
        assert!(prompt.contains("ソフトウェア開発"));
    }

    /// Integration test: requires ANTHROPIC_API_KEY in env or GNOME Keyring.
    /// Run with: cargo test test_claude_api_connectivity -- --ignored
    #[test]
    #[ignore]
    fn test_claude_api_connectivity() {
        let api_key = crate::config::resolve_api_key("ANTHROPIC_API_KEY")
            .expect("ANTHROPIC_API_KEY must be available (env or keyring)");

        let result = test_connectivity(ANTHROPIC_API_URL, &api_key, "claude-sonnet-4-6");
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

        let result = test_connectivity(ANTHROPIC_API_URL, &api_key, "claude-sonnet-4-6");
        match &result {
            Ok(text) => println!("claude-sonnet-4-6 OK: {}", text),
            Err(e) => println!("claude-sonnet-4-6 FAILED: {}", e),
        }
        assert!(
            result.is_ok(),
            "claude-sonnet-4-6 should be a valid model ID"
        );
    }
}
