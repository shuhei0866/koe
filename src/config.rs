use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Config {
    pub recognition: RecognitionConfig,
    pub ai: AiConfig,
    pub hotkey: HotkeyConfig,
    #[serde(default)]
    pub input: InputConfig,
    #[serde(default)]
    pub dictionaries: DictionaryConfig,
    #[serde(default)]
    pub memory: MemoryConfig,
    #[serde(default)]
    pub feedback: FeedbackConfig,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct RecognitionConfig {
    pub engine: RecognitionEngine,
    pub whisper_local: Option<WhisperLocalConfig>,
    pub openai_api: Option<OpenAiApiConfig>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RecognitionEngine {
    WhisperLocal,
    OpenaiApi,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct WhisperLocalConfig {
    pub model_path: String,
    #[serde(default = "default_language")]
    pub language: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct OpenAiApiConfig {
    #[serde(default = "default_openai_key_env")]
    pub api_key_env: String,
    #[serde(default = "default_language")]
    pub language: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AiConfig {
    pub engine: AiEngine,
    pub claude: Option<ClaudeConfig>,
    pub ollama: Option<OllamaConfig>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AiEngine {
    Claude,
    Ollama,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ClaudeConfig {
    #[serde(default = "default_anthropic_key_env")]
    pub api_key_env: String,
    #[serde(default = "default_claude_model")]
    pub model: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct OllamaConfig {
    #[serde(default = "default_ollama_host")]
    pub host: String,
    #[serde(default = "default_ollama_model")]
    pub model: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct HotkeyConfig {
    #[serde(default = "default_hotkey_mode")]
    pub mode: HotkeyMode,
    #[serde(default = "default_hotkey_key")]
    pub key: String,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum HotkeyMode {
    PushToTalk,
    Toggle,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
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

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct DictionaryConfig {
    #[serde(default)]
    pub paths: Vec<String>,
}

impl Default for DictionaryConfig {
    fn default() -> Self {
        Self { paths: vec![] }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct MemoryConfig {
    #[serde(default = "default_memory_enabled")]
    pub enabled: bool,
    #[serde(default = "default_memory_dir")]
    pub dir: String,
    #[serde(default = "default_consolidation_threshold")]
    pub consolidation_threshold: usize,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            enabled: default_memory_enabled(),
            dir: default_memory_dir(),
            consolidation_threshold: default_consolidation_threshold(),
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct FeedbackConfig {
    #[serde(default = "default_feedback_sound_enabled")]
    pub sound_enabled: bool,
    #[serde(default = "default_feedback_indicator_enabled")]
    pub indicator_enabled: bool,
}

impl Default for FeedbackConfig {
    fn default() -> Self {
        Self {
            sound_enabled: default_feedback_sound_enabled(),
            indicator_enabled: default_feedback_indicator_enabled(),
        }
    }
}

fn default_feedback_sound_enabled() -> bool {
    true
}
fn default_feedback_indicator_enabled() -> bool {
    true
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
    "claude-sonnet-4-6".to_string()
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
fn default_memory_enabled() -> bool {
    true
}
fn default_memory_dir() -> String {
    "~/.local/share/koe/memory".to_string()
}
fn default_consolidation_threshold() -> usize {
    50
}

/// Expand ~ and environment variables in a path string.
fn expand_path(p: &str) -> PathBuf {
    let expanded = shellexpand::tilde(p);
    PathBuf::from(expanded.as_ref())
}

/// Resolve an API key: try environment variable first, then GNOME Keyring via secret-tool.
///
/// The keyring lookup uses attributes `service=koe key=<env_var_name in kebab-case>`.
/// For example, `ANTHROPIC_API_KEY` → `secret-tool lookup service koe key anthropic-api-key`.
pub fn resolve_api_key(env_var: &str) -> Result<String> {
    // 1. Try environment variable
    if let Ok(val) = std::env::var(env_var) {
        if !val.is_empty() {
            return Ok(val);
        }
    }

    // 2. Try GNOME Keyring via secret-tool
    let keyring_key = env_var.to_lowercase().replace('_', "-");
    match std::process::Command::new("secret-tool")
        .args(["lookup", "service", "koe", "key", &keyring_key])
        .output()
    {
        Ok(output) if output.status.success() => {
            let secret = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !secret.is_empty() {
                return Ok(secret);
            }
        }
        _ => {}
    }

    anyhow::bail!(
        "{} is not set (checked env var and GNOME Keyring `secret-tool lookup service koe key {}`)",
        env_var,
        keyring_key
    )
}

/// Store an API key in the GNOME Keyring via secret-tool.
///
/// Uses attributes `service=koe key=<env_var_name in kebab-case>`.
pub fn store_api_key_in_keyring(env_var: &str, secret: &str) -> Result<()> {
    let keyring_key = env_var.to_lowercase().replace('_', "-");
    let mut child = std::process::Command::new("secret-tool")
        .args(["store", "--label", &format!("koe {}", keyring_key), "service", "koe", "key", &keyring_key])
        .stdin(std::process::Stdio::piped())
        .spawn()
        .context("launching secret-tool store")?;

    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        stdin.write_all(secret.as_bytes()).context("writing secret to stdin")?;
    }

    let status = child.wait().context("waiting for secret-tool")?;
    if !status.success() {
        anyhow::bail!("secret-tool store failed with status {}", status);
    }
    Ok(())
}

impl Config {
    /// Return the canonical config path (~/.config/koe/config.toml).
    pub fn config_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| expand_path("~/.config"))
            .join("koe/config.toml")
    }

    /// Load config from a TOML file, trying several default locations.
    pub fn load() -> Result<Self> {
        let candidates = vec![PathBuf::from("config.toml"), Self::config_path()];

        for path in &candidates {
            if path.exists() {
                return Self::load_from(path);
            }
        }

        anyhow::bail!("No config.toml found. Searched: {:?}", candidates)
    }

    pub fn load_from(path: &Path) -> Result<Self> {
        let content =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let config: Config =
            toml::from_str(&content).with_context(|| format!("parsing {}", path.display()))?;
        Ok(config)
    }

    /// Save config to the given path as TOML.
    pub fn save(&self, path: &Path) -> Result<()> {
        let content = toml::to_string_pretty(self).context("serializing config")?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating directory {}", parent.display()))?;
        }
        std::fs::write(path, content)
            .with_context(|| format!("writing config to {}", path.display()))?;
        Ok(())
    }

    /// Resolve the whisper model path (expand ~).
    pub fn whisper_model_path(&self) -> Option<PathBuf> {
        self.recognition
            .whisper_local
            .as_ref()
            .map(|w| expand_path(&w.model_path))
    }

    /// Resolve the memory directory path (expand ~).
    pub fn memory_dir(&self) -> PathBuf {
        expand_path(&self.memory.dir)
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_resolve_api_key_from_env() {
        // Set a temporary env var
        std::env::set_var("KOE_TEST_API_KEY_12345", "test-secret-value");
        let result = resolve_api_key("KOE_TEST_API_KEY_12345");
        std::env::remove_var("KOE_TEST_API_KEY_12345");
        assert_eq!(result.unwrap(), "test-secret-value");
    }

    #[test]
    fn test_resolve_api_key_empty_env_falls_through() {
        // Empty env var should not be returned
        std::env::set_var("KOE_TEST_EMPTY_KEY", "");
        let result = resolve_api_key("KOE_TEST_EMPTY_KEY");
        std::env::remove_var("KOE_TEST_EMPTY_KEY");
        // Should fail (no keyring entry either)
        assert!(result.is_err());
    }

    #[test]
    fn test_resolve_api_key_missing_env_and_keyring() {
        // Non-existent env var and no keyring entry
        std::env::remove_var("KOE_TEST_NONEXISTENT_KEY_99999");
        let result = resolve_api_key("KOE_TEST_NONEXISTENT_KEY_99999");
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("KOE_TEST_NONEXISTENT_KEY_99999"));
    }

    #[test]
    fn test_config_roundtrip_toml() {
        let config = Config {
            recognition: RecognitionConfig {
                engine: RecognitionEngine::WhisperLocal,
                whisper_local: Some(WhisperLocalConfig {
                    model_path: "/tmp/model.bin".to_string(),
                    language: "ja".to_string(),
                }),
                openai_api: None,
            },
            ai: AiConfig {
                engine: AiEngine::Claude,
                claude: Some(ClaudeConfig {
                    api_key_env: "ANTHROPIC_API_KEY".to_string(),
                    model: "claude-sonnet-4-6".to_string(),
                }),
                ollama: None,
            },
            hotkey: HotkeyConfig {
                mode: HotkeyMode::PushToTalk,
                key: "Super_R".to_string(),
            },
            input: InputConfig {
                method: "direct_type".to_string(),
            },
            dictionaries: DictionaryConfig { paths: vec![] },
            memory: MemoryConfig::default(),
            feedback: FeedbackConfig::default(),
        };

        // Save to temp file
        let dir = std::env::temp_dir().join("koe-test-config");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("test-config.toml");
        config.save(&path).unwrap();

        // Load back
        let loaded = Config::load_from(&path).unwrap();
        assert_eq!(loaded.ai.engine, AiEngine::Claude);
        assert_eq!(loaded.ai.claude.unwrap().model, "claude-sonnet-4-6");
        assert_eq!(loaded.recognition.engine, RecognitionEngine::WhisperLocal);
        assert_eq!(loaded.hotkey.mode, HotkeyMode::PushToTalk);
        assert_eq!(loaded.memory.consolidation_threshold, default_consolidation_threshold());
        assert!(loaded.feedback.sound_enabled);
        assert!(loaded.feedback.indicator_enabled);

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_config_deserialize_defaults() {
        let toml_str = r#"
[recognition]
engine = "whisper_local"

[recognition.whisper_local]
model_path = "/tmp/model.bin"

[ai]
engine = "claude"

[ai.claude]
api_key_env = "ANTHROPIC_API_KEY"

[hotkey]
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        // Defaults should be applied
        assert_eq!(config.hotkey.mode, HotkeyMode::PushToTalk);
        assert_eq!(config.hotkey.key, "Super_R");
        assert_eq!(config.ai.claude.unwrap().model, "claude-sonnet-4-6");
        assert_eq!(config.input.method, "direct_type");
        // feedback should have defaults
        assert!(config.feedback.sound_enabled);
        assert!(config.feedback.indicator_enabled);
    }

    #[test]
    fn test_feedback_config_defaults() {
        let feedback = FeedbackConfig::default();
        assert!(feedback.sound_enabled);
        assert!(feedback.indicator_enabled);
    }

    #[test]
    fn test_feedback_config_custom_values() {
        let toml_str = r#"
[recognition]
engine = "whisper_local"

[recognition.whisper_local]
model_path = "/tmp/model.bin"

[ai]
engine = "claude"

[ai.claude]
api_key_env = "ANTHROPIC_API_KEY"

[hotkey]

[feedback]
sound_enabled = false
indicator_enabled = false
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert!(!config.feedback.sound_enabled);
        assert!(!config.feedback.indicator_enabled);
    }

    /// Integration test: resolve API key from GNOME Keyring.
    /// Run with: cargo test test_resolve_api_key_from_keyring -- --ignored
    #[test]
    #[ignore]
    fn test_resolve_api_key_from_keyring() {
        // This requires a key stored in GNOME Keyring:
        //   secret-tool store --label "koe anthropic-api-key" service koe key anthropic-api-key
        std::env::remove_var("ANTHROPIC_API_KEY");
        let result = resolve_api_key("ANTHROPIC_API_KEY");
        assert!(result.is_ok(), "Expected keyring to have ANTHROPIC_API_KEY: {:?}", result.err());
        assert!(!result.unwrap().is_empty());
    }
}
