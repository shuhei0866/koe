pub mod claude;
pub mod ollama;

use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;

use crate::config::{AiConfig, AiEngine};
use crate::context::WindowContext;
use crate::dictionary::Dictionary;

/// AI processing result (text + learnings).
#[derive(Debug, Clone)]
pub struct ProcessResult {
    pub text: String,
    pub learnings: Vec<Learning>,
}

/// Information learned by the LLM during processing.
#[derive(Debug, Clone)]
pub enum Learning {
    Term { from: String, to: String },
    Context { category: String, content: String },
}

/// Result of memory consolidation.
#[derive(Debug, Clone)]
pub struct ConsolidationResult {
    pub terms: HashMap<String, String>,
    pub context_markdown: String,
}

/// Trait for AI text post-processing.
#[async_trait]
pub trait TextProcessor: Send + Sync {
    async fn process(
        &self,
        raw_text: &str,
        context: &WindowContext,
        dictionary: &Dictionary,
        memory_context: &str,
    ) -> Result<ProcessResult>;

    /// Consolidate memory data by summarizing and deduplicating.
    /// Returns None if the engine does not support consolidation.
    async fn consolidate_memory(
        &self,
        memory_content: &str,
    ) -> Result<Option<ConsolidationResult>>;
}

/// Build the system prompt for AI post-processing.
pub fn build_system_prompt(context: &WindowContext, dictionary: &Dictionary, memory_context: &str) -> String {
    let mut prompt = String::from(
        "You are a voice input post-processor. Your job is to clean up and format speech-to-text output.\n\n\
         Rules:\n\
         - Fix obvious speech recognition errors\n\
         - Apply proper punctuation and formatting\n\
         - Preserve the speaker's intent and meaning\n\
         - If the context suggests code or technical content, format appropriately\n\
         - Apply any term corrections from the dictionary\n\
         - Output ONLY the corrected text, no explanations\n",
    );

    // Add window context
    if !context.window_title.is_empty() || !context.app_name.is_empty() {
        prompt.push_str(&format!(
            "\nCurrent context:\n  Window: {}\n  Application: {}\n",
            context.window_title, context.app_name
        ));
    }

    // Add dictionary info
    let dict_info = dictionary.format_for_prompt();
    if !dict_info.is_empty() {
        prompt.push_str(&format!("\nDictionary:\n{}\n", dict_info));
    }

    // Add memory context
    if !memory_context.is_empty() {
        prompt.push_str(&format!("\nLearned context from previous interactions:\n{}\n", memory_context));
    }

    // Add learning instructions
    prompt.push_str(
        "\nYou have access to learning tools. When you notice information worth remembering \
         for future voice inputs (new terms, user context, domain knowledge), use the \
         appropriate tool. Only learn genuinely useful information — do not learn from \
         every input.\n"
    );

    prompt
}

/// Build the prompt for memory consolidation.
pub fn build_consolidation_prompt(memory_content: &str) -> String {
    format!(
        "以下は音声入力ツール koe が自動学習したメモリデータです。\n\
         重複を排除し、関連する情報を統合して簡潔にまとめてください。\n\n\
         出力形式:\n\
         1. まず用語辞書を以下の JSON 形式で出力してください:\n\
         ```json\n\
         {{\"terms\": {{\"誤認識\": \"正しい表記\", ...}}}}\n\
         ```\n\n\
         2. 次にコンテキスト情報を以下の Markdown 形式で出力してください:\n\
         ```markdown\n\
         ## category_name\n\
         - 内容\n\
         ```\n\n\
         ## 現在のメモリ:\n\
         {}",
        memory_content
    )
}

/// Create a text processor based on config.
pub fn create_processor(config: &AiConfig) -> Result<Box<dyn TextProcessor>> {
    match config.engine {
        AiEngine::Claude => {
            let claude_config = config
                .claude
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("claude config missing"))?;
            Ok(Box::new(claude::ClaudeProcessor::new(claude_config)?))
        }
        AiEngine::Ollama => {
            let ollama_config = config
                .ollama
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("ollama config missing"))?;
            Ok(Box::new(ollama::OllamaProcessor::new(ollama_config)?))
        }
    }
}
