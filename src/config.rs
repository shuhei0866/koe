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
    #[serde(default)]
    pub history: HistoryConfig,
    #[serde(default)]
    pub limits: LimitsConfig,
    #[serde(default)]
    pub vad: VadConfig,
    #[serde(default)]
    pub live: LiveConfig,
}

/// Live transcript: transcribe repeatedly while recording and show the text
/// in the overlay. Only used with local recognition (cloud APIs bill per call).
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct LiveConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Pause between live transcriptions, in milliseconds.
    #[serde(default = "default_live_interval_ms")]
    pub interval_ms: u64,
}

impl Default for LiveConfig {
    fn default() -> Self {
        Self {
            enabled: default_true(),
            interval_ms: default_live_interval_ms(),
        }
    }
}

fn default_live_interval_ms() -> u64 {
    700
}

/// Voice activity detection before recognition (Silero VAD).
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct VadConfig {
    /// Drop silence before recognition. When the model file is missing,
    /// Koe logs a warning and transcribes the whole recording as before.
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_vad_model_path")]
    pub model_path: String,
    /// Speech probability threshold (0.0-1.0). Raise it if noise is kept.
    #[serde(default = "default_vad_threshold")]
    pub threshold: f32,
    /// Speech shorter than this is discarded as noise.
    #[serde(default = "default_vad_min_speech_ms")]
    pub min_speech_ms: u32,
    /// Padding kept around each speech segment so word edges are not cut.
    #[serde(default = "default_vad_speech_pad_ms")]
    pub speech_pad_ms: u32,
}

impl Default for VadConfig {
    fn default() -> Self {
        Self {
            enabled: default_true(),
            model_path: default_vad_model_path(),
            threshold: default_vad_threshold(),
            min_speech_ms: default_vad_min_speech_ms(),
            speech_pad_ms: default_vad_speech_pad_ms(),
        }
    }
}

fn default_vad_model_path() -> String {
    "~/.local/share/koe/models/ggml-silero-v5.1.2.bin".to_string()
}
fn default_vad_threshold() -> f32 {
    0.5
}
fn default_vad_min_speech_ms() -> u32 {
    250
}
fn default_vad_speech_pad_ms() -> u32 {
    200
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
    /// Transcription model. `whisper-1` stops working on 2027-02-26.
    #[serde(default = "default_openai_model")]
    pub model: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AiConfig {
    pub engine: AiEngine,
    #[serde(default = "default_true")]
    pub context_enabled: bool,
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
    #[serde(default)]
    pub wayland_paste_shortcut: WaylandPasteShortcut,
}

#[derive(Debug, Default, Deserialize, Serialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum WaylandPasteShortcut {
    #[default]
    CtrlV,
    CtrlShiftV,
}

impl Default for InputConfig {
    fn default() -> Self {
        Self {
            method: default_input_method(),
            wayland_paste_shortcut: WaylandPasteShortcut::default(),
        }
    }
}

#[derive(Debug, Default, Deserialize, Serialize, Clone)]
pub struct DictionaryConfig {
    #[serde(default)]
    pub paths: Vec<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_history_dir")]
    pub dir: String,
    #[serde(default = "default_max_entries")]
    pub max_entries: usize,
}

impl Default for HistoryConfig {
    fn default() -> Self {
        Self {
            enabled: default_true(),
            dir: default_history_dir(),
            max_entries: default_max_entries(),
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

/// Size limits for external input parsing (DoS / OOM protection).
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct LimitsConfig {
    /// Maximum API response body size in bytes (default: 10 MiB).
    #[serde(default = "default_max_api_response_bytes")]
    pub max_api_response_bytes: usize,
    /// Maximum IPC message length in bytes (default: 64 KiB).
    #[serde(default = "default_max_ipc_message_bytes")]
    pub max_ipc_message_bytes: usize,
    /// Maximum config/dictionary/memory file size in bytes (default: 1 MiB).
    #[serde(default = "default_max_file_size_bytes")]
    pub max_file_size_bytes: usize,
}

impl Default for LimitsConfig {
    fn default() -> Self {
        Self {
            max_api_response_bytes: default_max_api_response_bytes(),
            max_ipc_message_bytes: default_max_ipc_message_bytes(),
            max_file_size_bytes: default_max_file_size_bytes(),
        }
    }
}

fn default_max_api_response_bytes() -> usize {
    10 * 1024 * 1024 // 10 MiB
}
fn default_max_ipc_message_bytes() -> usize {
    64 * 1024 // 64 KiB
}
fn default_max_file_size_bytes() -> usize {
    1024 * 1024 // 1 MiB
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
pub fn default_openai_model() -> String {
    "gpt-transcribe".to_string()
}
fn default_openai_key_env() -> String {
    "OPENAI_API_KEY".to_string()
}
fn default_anthropic_key_env() -> String {
    "ANTHROPIC_API_KEY".to_string()
}
fn default_claude_model() -> String {
    "claude-sonnet-5-5".to_string()
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
fn default_true() -> bool {
    true
}
fn default_history_dir() -> String {
    "~/.local/share/koe/history".to_string()
}
fn default_max_entries() -> usize {
    1000
}

/// Read a file into a String, rejecting files larger than `max_bytes`.
///
/// Reads the raw bytes first and checks the actual size to avoid TOCTOU
/// races between a metadata check and the subsequent read.
pub fn read_to_string_limited(path: &Path, max_bytes: u64) -> Result<String> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    if bytes.len() as u64 > max_bytes {
        anyhow::bail!(
            "file {} is too large ({} bytes, limit {} bytes)",
            path.display(),
            bytes.len(),
            max_bytes,
        );
    }
    String::from_utf8(bytes)
        .map_err(|e| anyhow::anyhow!("file {} is not valid UTF-8: {}", path.display(), e))
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
#[allow(dead_code)]
pub fn store_api_key_in_keyring(env_var: &str, secret: &str) -> Result<()> {
    let keyring_key = env_var.to_lowercase().replace('_', "-");
    let mut child = std::process::Command::new("secret-tool")
        .args([
            "store",
            "--label",
            &format!("koe {}", keyring_key),
            "service",
            "koe",
            "key",
            &keyring_key,
        ])
        .stdin(std::process::Stdio::piped())
        .spawn()
        .context("launching secret-tool store")?;

    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        stdin
            .write_all(secret.as_bytes())
            .context("writing secret to stdin")?;
    }

    let status = child.wait().context("waiting for secret-tool")?;
    if !status.success() {
        anyhow::bail!("secret-tool store failed with status {}", status);
    }
    Ok(())
}

/// Config template installed by the .deb package.
const PACKAGED_CONFIG_PATH: &str = "/usr/share/koe/config.toml";

/// Copy the packaged config template to `dest` if `dest` does not exist yet.
///
/// Returns the path that was written, or `None` when there is no template
/// (e.g. running from a source checkout).
fn seed_config(template: &Path, dest: &Path) -> Result<Option<PathBuf>> {
    if dest.exists() || !template.exists() {
        return Ok(None);
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating directory {}", parent.display()))?;
    }
    std::fs::copy(template, dest).with_context(|| {
        format!(
            "copying config template {} to {}",
            template.display(),
            dest.display()
        )
    })?;
    tracing::info!("Created {} from {}", dest.display(), template.display());
    Ok(Some(dest.to_path_buf()))
}

impl Config {
    /// Return the canonical config path (~/.config/koe/config.toml).
    pub fn config_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| expand_path("~/.config"))
            .join("koe/config.toml")
    }

    /// Load config from a TOML file, trying several default locations.
    ///
    /// When no config exists yet (a fresh package install), the packaged
    /// template is copied to the canonical path first, so the user gets a
    /// file they can edit instead of an error.
    pub fn load() -> Result<Self> {
        let candidates = vec![PathBuf::from("config.toml"), Self::config_path()];

        for path in &candidates {
            if path.exists() {
                return Self::load_from(path);
            }
        }

        if let Some(path) = seed_config(Path::new(PACKAGED_CONFIG_PATH), &Self::config_path())? {
            return Self::load_from(&path);
        }

        anyhow::bail!("No config.toml found. Searched: {:?}", candidates)
    }

    pub fn load_from(path: &Path) -> Result<Self> {
        // Use a generous limit for the config file itself (1 MiB) since we don't
        // know the user's limits config yet — this is the bootstrap read.
        let content = read_to_string_limited(path, default_max_file_size_bytes() as u64)?;
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
    #[allow(dead_code)]
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

    /// Resolve the history directory path (expand ~).
    pub fn history_dir(&self) -> PathBuf {
        expand_path(&self.history.dir)
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
    fn legacy_input_config_defaults_to_gui_paste_and_preserves_terminal_choice() {
        let legacy: InputConfig = toml::from_str("method = 'direct_type'").unwrap();
        assert_eq!(legacy.wayland_paste_shortcut, WaylandPasteShortcut::CtrlV);
        let terminal: InputConfig =
            toml::from_str("wayland_paste_shortcut = 'ctrl_shift_v'").unwrap();
        assert_eq!(
            terminal.wayland_paste_shortcut,
            WaylandPasteShortcut::CtrlShiftV
        );
        assert!(toml::from_str::<InputConfig>("wayland_paste_shortcut = 'Enter'").is_err());
    }
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
                context_enabled: true,
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
                ..Default::default()
            },
            dictionaries: DictionaryConfig { paths: vec![] },
            memory: MemoryConfig::default(),
            feedback: FeedbackConfig::default(),
            history: HistoryConfig::default(),
            limits: LimitsConfig::default(),
            vad: VadConfig::default(),
            live: LiveConfig::default(),
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
        assert_eq!(
            loaded.memory.consolidation_threshold,
            default_consolidation_threshold()
        );
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
        assert_eq!(config.ai.claude.unwrap().model, "claude-sonnet-5-5");
        assert_eq!(config.input.method, "direct_type");
        // feedback should have defaults
        assert!(config.feedback.sound_enabled);
        assert!(config.feedback.indicator_enabled);
    }

    #[test]
    fn test_openai_model_defaults_to_gpt_transcribe() {
        let config: OpenAiApiConfig = toml::from_str(r#"api_key_env = "OPENAI_API_KEY""#).unwrap();
        assert_eq!(config.model, "gpt-transcribe");
        assert_eq!(config.language, "ja");

        let config: OpenAiApiConfig = toml::from_str(r#"model = "whisper-1""#).unwrap();
        assert_eq!(config.model, "whisper-1");
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

    #[test]
    fn test_history_config_defaults() {
        let history = HistoryConfig::default();
        assert!(history.enabled);
        assert_eq!(history.dir, "~/.local/share/koe/history");
        assert_eq!(history.max_entries, 1000);
    }

    #[test]
    fn test_history_config_serialization_roundtrip() {
        let history = HistoryConfig::default();
        let toml_str = toml::to_string(&history).unwrap();
        let loaded: HistoryConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(loaded.enabled, history.enabled);
        assert_eq!(loaded.dir, history.dir);
        assert_eq!(loaded.max_entries, history.max_entries);
    }

    #[test]
    fn test_history_config_custom_values() {
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

[history]
enabled = false
dir = "/tmp/koe-history"
max_entries = 500
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert!(!config.history.enabled);
        assert_eq!(config.history.dir, "/tmp/koe-history");
        assert_eq!(config.history.max_entries, 500);
    }

    #[test]
    fn test_history_config_defaults_in_config() {
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
        assert!(config.history.enabled);
        assert_eq!(config.history.dir, "~/.local/share/koe/history");
        assert_eq!(config.history.max_entries, 1000);
    }

    #[test]
    fn test_context_enabled_defaults_to_true() {
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
        assert!(config.ai.context_enabled);
    }

    #[test]
    fn test_context_enabled_can_be_disabled() {
        let toml_str = r#"
[recognition]
engine = "whisper_local"

[recognition.whisper_local]
model_path = "/tmp/model.bin"

[ai]
engine = "claude"
context_enabled = false

[ai.claude]
api_key_env = "ANTHROPIC_API_KEY"

[hotkey]
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert!(!config.ai.context_enabled);
    }

    #[test]
    fn test_limits_config_defaults() {
        let limits = LimitsConfig::default();
        assert_eq!(limits.max_api_response_bytes, 10 * 1024 * 1024);
        assert_eq!(limits.max_ipc_message_bytes, 64 * 1024);
        assert_eq!(limits.max_file_size_bytes, 1024 * 1024);
    }

    #[test]
    fn test_limits_config_custom_values() {
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

[limits]
max_api_response_bytes = 5242880
max_ipc_message_bytes = 32768
max_file_size_bytes = 524288
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.limits.max_api_response_bytes, 5 * 1024 * 1024);
        assert_eq!(config.limits.max_ipc_message_bytes, 32 * 1024);
        assert_eq!(config.limits.max_file_size_bytes, 512 * 1024);
    }

    #[test]
    fn test_limits_config_defaults_in_config() {
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
        assert_eq!(config.limits.max_api_response_bytes, 10 * 1024 * 1024);
        assert_eq!(config.limits.max_ipc_message_bytes, 64 * 1024);
        assert_eq!(config.limits.max_file_size_bytes, 1024 * 1024);
    }

    #[test]
    fn test_read_to_string_limited_within_limit() {
        let dir = std::env::temp_dir().join("koe-test-limits");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("small.txt");
        std::fs::write(&path, "hello").unwrap();

        let content = read_to_string_limited(&path, 1024).unwrap();
        assert_eq!(content, "hello");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_read_to_string_limited_exceeds_limit() {
        let dir = std::env::temp_dir().join("koe-test-limits-exceed");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("big.txt");
        std::fs::write(&path, "a".repeat(200)).unwrap();

        let result = read_to_string_limited(&path, 100);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("too large"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_read_to_string_limited_exact_boundary() {
        let dir = std::env::temp_dir().join("koe-test-limits-exact");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("exact.txt");
        std::fs::write(&path, "12345").unwrap();

        let content = read_to_string_limited(&path, 5).unwrap();
        assert_eq!(content, "12345");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn seed_config_copies_template_into_missing_user_config() {
        let dir = std::env::temp_dir().join("koe-test-seed-copy");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let template = dir.join("template.toml");
        std::fs::write(&template, "[hotkey]\nkey = \"Super_R\"\n").unwrap();
        let dest = dir.join("home/.config/koe/config.toml");

        let seeded = seed_config(&template, &dest).unwrap();

        assert_eq!(seeded.as_deref(), Some(dest.as_path()));
        assert_eq!(
            std::fs::read_to_string(&dest).unwrap(),
            "[hotkey]\nkey = \"Super_R\"\n"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn seed_config_never_overwrites_an_existing_user_config() {
        let dir = std::env::temp_dir().join("koe-test-seed-keep");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let template = dir.join("template.toml");
        std::fs::write(&template, "template").unwrap();
        let dest = dir.join("config.toml");
        std::fs::write(&dest, "mine").unwrap();

        assert_eq!(seed_config(&template, &dest).unwrap(), None);
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "mine");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn seed_config_does_nothing_without_a_packaged_template() {
        let dir = std::env::temp_dir().join("koe-test-seed-none");
        let _ = std::fs::remove_dir_all(&dir);
        let dest = dir.join("config.toml");

        assert_eq!(seed_config(&dir.join("missing.toml"), &dest).unwrap(), None);
        assert!(!dest.exists());
    }

    #[test]
    fn packaged_config_template_parses() {
        let content = include_str!("../packaging/deb/config.toml");
        let config: Config = toml::from_str(content).unwrap();
        assert_eq!(
            config.dictionaries.paths,
            vec!["/usr/share/koe/dictionaries/default.toml".to_string()]
        );
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
        assert!(
            result.is_ok(),
            "Expected keyring to have ANTHROPIC_API_KEY: {:?}",
            result.err()
        );
        assert!(!result.unwrap().is_empty());
    }
}
