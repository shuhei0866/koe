use gtk4::prelude::*;
use libadwaita::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

use crate::config::{
    AiConfig, AiEngine, ClaudeConfig, Config, DictionaryConfig, HotkeyConfig, HotkeyMode,
    InputConfig, OllamaConfig, OpenAiApiConfig, RecognitionConfig, RecognitionEngine,
    WhisperLocalConfig,
};

/// Build and show the settings window.
pub fn build(app: &libadwaita::Application) {
    let config = Config::load().unwrap_or_else(|_| default_config());
    let config = Rc::new(RefCell::new(config));

    let window = libadwaita::PreferencesWindow::builder()
        .application(app)
        .title("koe Settings")
        .default_width(700)
        .default_height(600)
        .build();

    window.add(&build_general_page(&config));
    window.add(&build_recognition_page(&config));
    window.add(&build_ai_page(&config));
    window.add(&build_mic_test_page());
    window.present();
}

fn default_config() -> Config {
    Config {
        recognition: RecognitionConfig {
            engine: RecognitionEngine::WhisperLocal,
            whisper_local: Some(WhisperLocalConfig {
                model_path: "~/.local/share/koe/models/ggml-large-v3.bin".to_string(),
                language: "ja".to_string(),
            }),
            openai_api: None,
        },
        ai: AiConfig {
            engine: AiEngine::Claude,
            claude: Some(ClaudeConfig {
                api_key_env: "ANTHROPIC_API_KEY".to_string(),
                model: "claude-sonnet-4-6-20250514".to_string(),
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
    }
}

/// Save config and notify daemon to reload.
fn save_config(config: &Rc<RefCell<Config>>) {
    let cfg = config.borrow();
    let path = Config::config_path();
    if let Err(e) = cfg.save(&path) {
        tracing::error!("Failed to save config: {}", e);
        return;
    }
    tracing::info!("Config saved to {}", path.display());

    // Try to notify the daemon to reload
    if let Err(e) = crate::ipc::client::reload_config() {
        tracing::debug!("Could not notify daemon (may not be running): {}", e);
    }
}

// ─── General Settings Page ──────────────────────────────────────────────────

fn build_general_page(config: &Rc<RefCell<Config>>) -> libadwaita::PreferencesPage {
    let page = libadwaita::PreferencesPage::builder()
        .title("General")
        .icon_name("preferences-system-symbolic")
        .build();

    // Hotkey group
    let hotkey_group = libadwaita::PreferencesGroup::builder()
        .title("Hotkey")
        .build();

    // Mode selection
    let mode_row = libadwaita::ComboRow::builder()
        .title("Mode")
        .subtitle("How the hotkey triggers recording")
        .build();
    let mode_list = gtk4::StringList::new(&["Push-to-Talk", "Toggle"]);
    mode_row.set_model(Some(&mode_list));
    let current_mode = match config.borrow().hotkey.mode {
        HotkeyMode::PushToTalk => 0,
        HotkeyMode::Toggle => 1,
    };
    mode_row.set_selected(current_mode);

    let config_clone = config.clone();
    mode_row.connect_selected_notify(move |row| {
        let mode = match row.selected() {
            0 => HotkeyMode::PushToTalk,
            _ => HotkeyMode::Toggle,
        };
        config_clone.borrow_mut().hotkey.mode = mode;
        save_config(&config_clone);
    });

    hotkey_group.add(&mode_row);

    // Key entry
    let key_row = libadwaita::EntryRow::builder()
        .title("Key")
        .text(&config.borrow().hotkey.key)
        .build();

    let config_clone = config.clone();
    key_row.connect_changed(move |row| {
        let text = row.text().to_string();
        if !text.is_empty() {
            config_clone.borrow_mut().hotkey.key = text;
            save_config(&config_clone);
        }
    });

    hotkey_group.add(&key_row);
    page.add(&hotkey_group);

    // Autostart group
    let autostart_group = libadwaita::PreferencesGroup::builder()
        .title("Startup")
        .build();

    let autostart_row = libadwaita::SwitchRow::builder()
        .title("Launch at login")
        .subtitle("Automatically start koe when you log in")
        .build();

    let desktop_path = autostart_desktop_path();
    autostart_row.set_active(desktop_path.exists());

    autostart_row.connect_active_notify(move |row| {
        if row.is_active() {
            if let Err(e) = create_autostart_entry() {
                tracing::error!("Failed to create autostart entry: {}", e);
            }
        } else {
            if let Err(e) = remove_autostart_entry() {
                tracing::error!("Failed to remove autostart entry: {}", e);
            }
        }
    });

    autostart_group.add(&autostart_row);
    page.add(&autostart_group);

    page
}

// ─── Recognition Settings Page ──────────────────────────────────────────────

fn build_recognition_page(config: &Rc<RefCell<Config>>) -> libadwaita::PreferencesPage {
    let page = libadwaita::PreferencesPage::builder()
        .title("Recognition")
        .icon_name("audio-input-microphone-symbolic")
        .build();

    // Engine selection
    let engine_group = libadwaita::PreferencesGroup::builder()
        .title("Speech Recognition Engine")
        .build();

    let engine_row = libadwaita::ComboRow::builder()
        .title("Engine")
        .build();
    let engine_list = gtk4::StringList::new(&["Whisper Local", "OpenAI API"]);
    engine_row.set_model(Some(&engine_list));
    let current_engine = match config.borrow().recognition.engine {
        RecognitionEngine::WhisperLocal => 0,
        RecognitionEngine::OpenaiApi => 1,
    };
    engine_row.set_selected(current_engine);

    engine_group.add(&engine_row);
    page.add(&engine_group);

    // Whisper Local settings
    let whisper_group = libadwaita::PreferencesGroup::builder()
        .title("Whisper Local")
        .build();

    let wl = config
        .borrow()
        .recognition
        .whisper_local
        .clone()
        .unwrap_or(WhisperLocalConfig {
            model_path: "~/.local/share/koe/models/ggml-large-v3.bin".to_string(),
            language: "ja".to_string(),
        });

    let model_path_row = libadwaita::EntryRow::builder()
        .title("Model path")
        .text(&wl.model_path)
        .build();

    let whisper_lang_row = libadwaita::EntryRow::builder()
        .title("Language")
        .text(&wl.language)
        .build();

    whisper_group.add(&model_path_row);
    whisper_group.add(&whisper_lang_row);
    page.add(&whisper_group);

    // OpenAI API settings
    let openai_group = libadwaita::PreferencesGroup::builder()
        .title("OpenAI API")
        .build();

    let oa = config
        .borrow()
        .recognition
        .openai_api
        .clone()
        .unwrap_or(OpenAiApiConfig {
            api_key_env: "OPENAI_API_KEY".to_string(),
            language: "ja".to_string(),
        });

    let openai_key_row = libadwaita::EntryRow::builder()
        .title("API key env variable")
        .text(&oa.api_key_env)
        .build();

    let openai_lang_row = libadwaita::EntryRow::builder()
        .title("Language")
        .text(&oa.language)
        .build();

    openai_group.add(&openai_key_row);
    openai_group.add(&openai_lang_row);
    page.add(&openai_group);

    // Update visibility based on engine selection
    let whisper_group_clone = whisper_group.clone();
    let openai_group_clone = openai_group.clone();
    let update_visibility = move |selected: u32| {
        whisper_group_clone.set_visible(selected == 0);
        openai_group_clone.set_visible(selected == 1);
    };
    update_visibility(current_engine);

    let config_clone = config.clone();
    engine_row.connect_selected_notify(move |row| {
        let selected = row.selected();
        update_visibility(selected);

        let engine = match selected {
            0 => RecognitionEngine::WhisperLocal,
            _ => RecognitionEngine::OpenaiApi,
        };
        config_clone.borrow_mut().recognition.engine = engine;
        save_config(&config_clone);
    });

    // Connect entry change handlers
    let config_clone = config.clone();
    model_path_row.connect_changed(move |row| {
        let text = row.text().to_string();
        let mut cfg = config_clone.borrow_mut();
        if let Some(ref mut wl) = cfg.recognition.whisper_local {
            wl.model_path = text;
        } else {
            cfg.recognition.whisper_local = Some(WhisperLocalConfig {
                model_path: text,
                language: "ja".to_string(),
            });
        }
        drop(cfg);
        save_config(&config_clone);
    });

    let config_clone = config.clone();
    whisper_lang_row.connect_changed(move |row| {
        let text = row.text().to_string();
        let mut cfg = config_clone.borrow_mut();
        if let Some(ref mut wl) = cfg.recognition.whisper_local {
            wl.language = text;
        }
        drop(cfg);
        save_config(&config_clone);
    });

    let config_clone = config.clone();
    openai_key_row.connect_changed(move |row| {
        let text = row.text().to_string();
        let mut cfg = config_clone.borrow_mut();
        if let Some(ref mut oa) = cfg.recognition.openai_api {
            oa.api_key_env = text;
        } else {
            cfg.recognition.openai_api = Some(OpenAiApiConfig {
                api_key_env: text,
                language: "ja".to_string(),
            });
        }
        drop(cfg);
        save_config(&config_clone);
    });

    let config_clone = config.clone();
    openai_lang_row.connect_changed(move |row| {
        let text = row.text().to_string();
        let mut cfg = config_clone.borrow_mut();
        if let Some(ref mut oa) = cfg.recognition.openai_api {
            oa.language = text;
        }
        drop(cfg);
        save_config(&config_clone);
    });

    page
}

// ─── AI Settings Page ───────────────────────────────────────────────────────

fn build_ai_page(config: &Rc<RefCell<Config>>) -> libadwaita::PreferencesPage {
    let page = libadwaita::PreferencesPage::builder()
        .title("AI")
        .icon_name("applications-science-symbolic")
        .build();

    // Engine selection
    let engine_group = libadwaita::PreferencesGroup::builder()
        .title("AI Post-Processing Engine")
        .build();

    let engine_row = libadwaita::ComboRow::builder()
        .title("Engine")
        .build();
    let engine_list = gtk4::StringList::new(&["Claude", "Ollama"]);
    engine_row.set_model(Some(&engine_list));
    let current_engine = match config.borrow().ai.engine {
        AiEngine::Claude => 0,
        AiEngine::Ollama => 1,
    };
    engine_row.set_selected(current_engine);

    engine_group.add(&engine_row);
    page.add(&engine_group);

    // Claude settings
    let claude_group = libadwaita::PreferencesGroup::builder()
        .title("Claude")
        .build();

    let cc = config.borrow().ai.claude.clone().unwrap_or(ClaudeConfig {
        api_key_env: "ANTHROPIC_API_KEY".to_string(),
        model: "claude-sonnet-4-6-20250514".to_string(),
    });

    let claude_key_row = libadwaita::EntryRow::builder()
        .title("API key env variable")
        .text(&cc.api_key_env)
        .build();

    let claude_model_row = libadwaita::EntryRow::builder()
        .title("Model")
        .text(&cc.model)
        .build();

    claude_group.add(&claude_key_row);
    claude_group.add(&claude_model_row);
    page.add(&claude_group);

    // Ollama settings
    let ollama_group = libadwaita::PreferencesGroup::builder()
        .title("Ollama")
        .build();

    let ol = config.borrow().ai.ollama.clone().unwrap_or(OllamaConfig {
        host: "http://localhost:11434".to_string(),
        model: "qwen2.5:14b".to_string(),
    });

    let ollama_host_row = libadwaita::EntryRow::builder()
        .title("Host URL")
        .text(&ol.host)
        .build();

    let ollama_model_row = libadwaita::EntryRow::builder()
        .title("Model")
        .text(&ol.model)
        .build();

    ollama_group.add(&ollama_host_row);
    ollama_group.add(&ollama_model_row);
    page.add(&ollama_group);

    // Update visibility
    let claude_group_clone = claude_group.clone();
    let ollama_group_clone = ollama_group.clone();
    let update_visibility = move |selected: u32| {
        claude_group_clone.set_visible(selected == 0);
        ollama_group_clone.set_visible(selected == 1);
    };
    update_visibility(current_engine);

    let config_clone = config.clone();
    engine_row.connect_selected_notify(move |row| {
        let selected = row.selected();
        update_visibility(selected);

        let engine = match selected {
            0 => AiEngine::Claude,
            _ => AiEngine::Ollama,
        };
        config_clone.borrow_mut().ai.engine = engine;
        save_config(&config_clone);
    });

    // Claude change handlers
    let config_clone = config.clone();
    claude_key_row.connect_changed(move |row| {
        let text = row.text().to_string();
        let mut cfg = config_clone.borrow_mut();
        if let Some(ref mut cc) = cfg.ai.claude {
            cc.api_key_env = text;
        } else {
            cfg.ai.claude = Some(ClaudeConfig {
                api_key_env: text,
                model: "claude-sonnet-4-6-20250514".to_string(),
            });
        }
        drop(cfg);
        save_config(&config_clone);
    });

    let config_clone = config.clone();
    claude_model_row.connect_changed(move |row| {
        let text = row.text().to_string();
        let mut cfg = config_clone.borrow_mut();
        if let Some(ref mut cc) = cfg.ai.claude {
            cc.model = text;
        }
        drop(cfg);
        save_config(&config_clone);
    });

    // Ollama change handlers
    let config_clone = config.clone();
    ollama_host_row.connect_changed(move |row| {
        let text = row.text().to_string();
        let mut cfg = config_clone.borrow_mut();
        if let Some(ref mut ol) = cfg.ai.ollama {
            ol.host = text;
        } else {
            cfg.ai.ollama = Some(OllamaConfig {
                host: text,
                model: "qwen2.5:14b".to_string(),
            });
        }
        drop(cfg);
        save_config(&config_clone);
    });

    let config_clone = config.clone();
    ollama_model_row.connect_changed(move |row| {
        let text = row.text().to_string();
        let mut cfg = config_clone.borrow_mut();
        if let Some(ref mut ol) = cfg.ai.ollama {
            ol.model = text;
        }
        drop(cfg);
        save_config(&config_clone);
    });

    page
}

// ─── Mic Test Page ──────────────────────────────────────────────────────────

fn build_mic_test_page() -> libadwaita::PreferencesPage {
    let page = libadwaita::PreferencesPage::builder()
        .title("Mic Test")
        .icon_name("audio-input-microphone-symbolic")
        .build();

    let group = libadwaita::PreferencesGroup::builder()
        .title("Input Device")
        .build();

    // Device selection
    let device_row = libadwaita::ComboRow::builder()
        .title("Device")
        .subtitle("Select input device for recording")
        .build();

    let devices = crate::audio::list_input_devices().unwrap_or_default();
    let device_names: Vec<&str> = devices.iter().map(|s| s.as_str()).collect();
    let device_list = gtk4::StringList::new(&device_names);
    device_row.set_model(Some(&device_list));

    group.add(&device_row);

    // Level meter
    let level_group = libadwaita::PreferencesGroup::builder()
        .title("Mic Level")
        .build();

    let level_bar = gtk4::LevelBar::builder()
        .min_value(0.0)
        .max_value(1.0)
        .value(0.0)
        .build();
    level_bar.set_margin_start(12);
    level_bar.set_margin_end(12);
    level_bar.set_margin_top(8);
    level_bar.set_margin_bottom(8);

    let test_row = libadwaita::ActionRow::builder()
        .title("Test Recording")
        .subtitle("Press to test mic input level")
        .activatable(true)
        .build();

    let level_bar_clone = level_bar.clone();
    let recording = Rc::new(RefCell::new(false));
    let recorder = Rc::new(RefCell::new(crate::audio::AudioRecorder::new().ok()));

    let recording_clone = recording.clone();
    let recorder_clone = recorder.clone();
    test_row.connect_activated(move |row| {
        let mut is_rec = recording_clone.borrow_mut();
        let mut rec = recorder_clone.borrow_mut();
        if *is_rec {
            // Stop recording
            if let Some(ref mut r) = *rec {
                match r.stop() {
                    Ok(audio_data) => {
                        let peak = audio_data
                            .samples
                            .iter()
                            .map(|s| s.abs())
                            .fold(0.0f32, f32::max);
                        level_bar_clone.set_value(peak as f64);
                    }
                    Err(e) => {
                        tracing::error!("Failed to stop test recording: {}", e);
                    }
                }
            }
            row.set_subtitle("Press to test mic input level");
            *is_rec = false;
        } else {
            // Start recording
            if let Some(ref mut r) = *rec {
                if let Err(e) = r.start() {
                    tracing::error!("Failed to start test recording: {}", e);
                    return;
                }
            }
            row.set_subtitle("Recording... press again to stop");
            *is_rec = true;
        }
    });

    level_group.add(&test_row);

    let level_action_row = libadwaita::ActionRow::builder()
        .title("Level")
        .build();
    level_action_row.add_suffix(&level_bar);
    level_group.add(&level_action_row);

    page.add(&group);
    page.add(&level_group);

    page
}

// ─── Autostart helpers ──────────────────────────────────────────────────────

fn autostart_desktop_path() -> std::path::PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("~/.config"))
        .join("autostart/koe.desktop")
}

fn create_autostart_entry() -> anyhow::Result<()> {
    let path = autostart_desktop_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let exe = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("koe"));

    let content = format!(
        "[Desktop Entry]\nType=Application\nName=koe\nExec={}\nX-GNOME-Autostart-enabled=true\n",
        exe.display()
    );
    std::fs::write(&path, content)?;
    tracing::info!("Created autostart entry: {}", path.display());
    Ok(())
}

fn remove_autostart_entry() -> anyhow::Result<()> {
    let path = autostart_desktop_path();
    if path.exists() {
        std::fs::remove_file(&path)?;
        tracing::info!("Removed autostart entry: {}", path.display());
    }
    Ok(())
}
