use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub recognition: RecognitionConfig,
    pub ai: AiConfig,
    pub hotkey: HotkeyConfig,
    #[serde(default)]
    pub input: InputConfig,
    #[serde(default)]
    pub dictionaries: DictionaryConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct RecognitionConfig {
    pub engine: RecognitionEngine,
    pub whisper_local: Option<WhisperLocalConfig>,
    pub openai_api: Option<OpenAiApiConfig>,
}

#[derive(Debug, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RecognitionEngine {
    WhisperLocal,
    OpenaiApi,
}

#[derive(Debug, Deserialize, Clone)]
pub struct WhisperLocalConfig {
    pub model_path: String,
    #[serde(default = "default_language")]
    pub language: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct OpenAiApiConfig {
    #[serde(default = "default_openai_key_env")]
    pub api_key_env: String,
    #[serde(default = "default_language")]
    pub language: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct AiConfig {
    pub engine: AiEngine,
    pub claude: Option<ClaudeConfig>,
    pub ollama: Option<OllamaConfig>,
}

#[derive(Debug, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AiEngine {
    Claude,
    Ollama,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ClaudeConfig {
    #[serde(default = "default_anthropic_key_env")]
    pub api_key_env: String,
    #[serde(default = "default_claude_model")]
    pub model: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct OllamaConfig {
    #[serde(default = "default_ollama_host")]
    pub host: String,
    #[serde(default = "default_ollama_model")]
    pub model: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct HotkeyConfig {
    #[serde(default = "default_hotkey_mode")]
    pub mode: HotkeyMode,
    #[serde(default = "default_hotkey_key")]
    pub key: String,
}

#[derive(Debug, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum HotkeyMode {
    PushToTalk,
    Toggle,
}

#[derive(Debug, Deserialize, Clone)]
pub struct InputConfig {
    #[serde(default = "default_input_method")]
    pub method: String,
}

impl Default for InputConfig {
    fn default() -> Self {
        Self {
            method: default_input_method(),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct DictionaryConfig {
    #[serde(default)]
    pub paths: Vec<String>,
}

impl Default for DictionaryConfig {
    fn default() -> Self {
        Self { paths: vec![] }
    }
}

fn default_language() -> String {
    "ja".to_string()
}
fn default_openai_key_env() -> String {
    "OPENAI_API_KEY".to_string()
}
fn default_anthropic_key_env() -> String {
    "ANTHROPIC_API_KEY".to_string()
}
fn default_claude_model() -> String {
    "claude-sonnet-4-6-20250514".to_string()
}
fn default_ollama_host() -> String {
    "http://localhost:11434".to_string()
}
fn default_ollama_model() -> String {
    "qwen2.5:14b".to_string()
}
fn default_hotkey_mode() -> HotkeyMode {
    HotkeyMode::PushToTalk
}
fn default_hotkey_key() -> String {
    "Super_R".to_string()
}
fn default_input_method() -> String {
    "direct_type".to_string()
}

/// Expand ~ and environment variables in a path string.
fn expand_path(p: &str) -> PathBuf {
    let expanded = shellexpand::tilde(p);
    PathBuf::from(expanded.as_ref())
}

impl Config {
    /// Load config from a TOML file, trying several default locations.
    pub fn load() -> Result<Self> {
        let candidates = vec![
            PathBuf::from("config.toml"),
            dirs::config_dir()
                .map(|d| d.join("koe/config.toml"))
                .unwrap_or_default(),
            expand_path("~/.config/koe/config.toml"),
        ];

        for path in &candidates {
            if path.exists() {
                return Self::load_from(path);
            }
        }

        anyhow::bail!(
            "No config.toml found. Searched: {:?}",
            candidates
        )
    }

    pub fn load_from(path: &Path) -> Result<Self> {
        let content =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let config: Config =
            toml::from_str(&content).with_context(|| format!("parsing {}", path.display()))?;
        Ok(config)
    }

    /// Resolve the whisper model path (expand ~).
    pub fn whisper_model_path(&self) -> Option<PathBuf> {
        self.recognition
            .whisper_local
            .as_ref()
            .map(|w| expand_path(&w.model_path))
    }

    /// Resolve dictionary paths (expand ~).
    pub fn dictionary_paths(&self) -> Vec<PathBuf> {
        self.dictionaries
            .paths
            .iter()
            .map(|p| expand_path(p))
            .collect()
    }
}
