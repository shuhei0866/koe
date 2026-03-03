use gtk4::prelude::*;
use libadwaita::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

use crate::config::{
    AiConfig, AiEngine, ClaudeConfig, Config, DictionaryConfig, HotkeyConfig, HotkeyMode,
    InputConfig, OllamaConfig, OpenAiApiConfig, RecognitionConfig, RecognitionEngine,
    WhisperLocalConfig,
};

/// Known Claude models shown in the dropdown.
const CLAUDE_MODELS: &[&str] = &[
    "claude-sonnet-4-6",
    "claude-opus-4-6",
    "claude-haiku-4-5",
];

/// Read the selected model from a ComboRow + custom EntryRow pair.
/// If the selected item is "Other", returns the custom entry text.
fn read_model_selection(combo: &libadwaita::ComboRow, custom: &libadwaita::EntryRow) -> String {
    if let Some(item) = combo.selected_item() {
        if let Some(obj) = item.downcast_ref::<gtk4::StringObject>() {
            let val = obj.string();
            if val != "Other" {
                return val.to_string();
            }
        }
    }
    custom.text().to_string()
}

/// All widget handles needed to read settings on save.
struct Widgets {
    hotkey_mode: libadwaita::ComboRow,
    hotkey_key: libadwaita::EntryRow,
    rec_engine: libadwaita::ComboRow,
    whisper_model_path: libadwaita::EntryRow,
    whisper_lang: libadwaita::EntryRow,
    openai_key_env: libadwaita::EntryRow,
    openai_lang: libadwaita::EntryRow,
    ai_engine: libadwaita::ComboRow,
    claude_key_env: libadwaita::EntryRow,
    claude_model_combo: libadwaita::ComboRow,
    claude_model_custom: libadwaita::EntryRow,
    ollama_host: libadwaita::EntryRow,
    ollama_model_combo: libadwaita::ComboRow,
    ollama_model_custom: libadwaita::EntryRow,
}

impl Widgets {
    fn read_config(&self) -> Config {
        let hotkey_mode = match self.hotkey_mode.selected() {
            0 => HotkeyMode::PushToTalk,
            _ => HotkeyMode::Toggle,
        };
        let rec_engine = match self.rec_engine.selected() {
            0 => RecognitionEngine::WhisperLocal,
            _ => RecognitionEngine::OpenaiApi,
        };
        let ai_engine = match self.ai_engine.selected() {
            0 => AiEngine::Claude,
            _ => AiEngine::Ollama,
        };

        Config {
            hotkey: HotkeyConfig {
                mode: hotkey_mode,
                key: self.hotkey_key.text().to_string(),
            },
            recognition: RecognitionConfig {
                engine: rec_engine,
                whisper_local: Some(WhisperLocalConfig {
                    model_path: self.whisper_model_path.text().to_string(),
                    language: self.whisper_lang.text().to_string(),
                }),
                openai_api: Some(OpenAiApiConfig {
                    api_key_env: self.openai_key_env.text().to_string(),
                    language: self.openai_lang.text().to_string(),
                }),
            },
            ai: AiConfig {
                engine: ai_engine,
                claude: Some(ClaudeConfig {
                    api_key_env: self.claude_key_env.text().to_string(),
                    model: read_model_selection(&self.claude_model_combo, &self.claude_model_custom),
                }),
                ollama: Some(OllamaConfig {
                    host: self.ollama_host.text().to_string(),
                    model: read_model_selection(&self.ollama_model_combo, &self.ollama_model_custom),
                }),
            },
            input: InputConfig {
                method: "direct_type".to_string(),
            },
            dictionaries: DictionaryConfig { paths: vec![] },
            memory: Default::default(),
            feedback: Default::default(),
        }
    }
}

/// Build and show the settings window.
pub fn build(app: &libadwaita::Application) {
    let config = Config::load().unwrap_or_else(|_| default_config());

    let window = libadwaita::PreferencesWindow::builder()
        .application(app)
        .title("koe Settings")
        .default_width(700)
        .default_height(600)
        .build();

    // Build pages, collecting widget handles
    let (general_page, hotkey_mode, hotkey_key) = build_general_page(&config);
    let (rec_page, rec_engine, whisper_model_path, whisper_lang, openai_key_env, openai_lang) =
        build_recognition_page(&config);
    let (
        ai_page,
        ai_engine,
        claude_key_env,
        claude_model_combo,
        claude_model_custom,
        ollama_host,
        ollama_model_combo,
        ollama_model_custom,
    ) = build_ai_page(&config, &window);

    window.add(&general_page);
    window.add(&rec_page);
    window.add(&ai_page);
    window.add(&build_mic_test_page());

    let widgets = Rc::new(Widgets {
        hotkey_mode,
        hotkey_key,
        rec_engine,
        whisper_model_path,
        whisper_lang,
        openai_key_env,
        openai_lang,
        ai_engine,
        claude_key_env,
        claude_model_combo,
        claude_model_custom,
        ollama_host,
        ollama_model_combo,
        ollama_model_custom,
    });

    // Save on window close
    let widgets_close = widgets.clone();
    let window_close = window.clone();
    window.connect_close_request(move |_| {
        save_from_widgets(&widgets_close, Some(&window_close));
        gtk4::glib::Propagation::Proceed
    });

    window.present();
}

fn save_from_widgets(
    widgets: &Widgets,
    window: Option<&libadwaita::PreferencesWindow>,
) -> bool {
    let config = widgets.read_config();
    let path = Config::config_path();
    match config.save(&path) {
        Ok(()) => {
            // Notify daemon to reload (ignore errors — daemon may not be running)
            let _ = crate::ipc::client::reload_config();
            if let Some(w) = window {
                w.add_toast(libadwaita::Toast::new("Settings saved"));
            }
            true
        }
        Err(e) => {
            if let Some(w) = window {
                let msg = format!("{}", e);
                show_error_toast(w, &format!("Save failed: {}", truncate(&msg, 60)), &msg);
            }
            false
        }
    }
}

fn default_config() -> Config {
    Config {
        recognition: RecognitionConfig {
            engine: RecognitionEngine::WhisperLocal,
            whisper_local: Some(WhisperLocalConfig {
                model_path: "~/.local/share/koe/models/ggml-large-v3.bin".to_string(),
                language: "ja".to_string(),
            }),
            openai_api: Some(OpenAiApiConfig {
                api_key_env: "OPENAI_API_KEY".to_string(),
                language: "ja".to_string(),
            }),
        },
        ai: AiConfig {
            engine: AiEngine::Claude,
            claude: Some(ClaudeConfig {
                api_key_env: "ANTHROPIC_API_KEY".to_string(),
                model: "claude-sonnet-4-6".to_string(),
            }),
            ollama: Some(OllamaConfig {
                host: "http://localhost:11434".to_string(),
                model: "qwen2.5:14b".to_string(),
            }),
        },
        hotkey: HotkeyConfig {
            mode: HotkeyMode::PushToTalk,
            key: "Super_R".to_string(),
        },
        input: InputConfig {
            method: "direct_type".to_string(),
        },
        dictionaries: DictionaryConfig { paths: vec![] },
        memory: Default::default(),
        feedback: Default::default(),
    }
}

// ─── General Settings Page ──────────────────────────────────────────────────

fn build_general_page(
    config: &Config,
) -> (
    libadwaita::PreferencesPage,
    libadwaita::ComboRow,
    libadwaita::EntryRow,
) {
    let page = libadwaita::PreferencesPage::builder()
        .title("General")
        .icon_name("preferences-system-symbolic")
        .build();

    // Hotkey group
    let hotkey_group = libadwaita::PreferencesGroup::builder()
        .title("Hotkey")
        .build();

    let mode_row = libadwaita::ComboRow::builder()
        .title("Mode")
        .subtitle("How the hotkey triggers recording")
        .build();
    let mode_list = gtk4::StringList::new(&["Push-to-Talk", "Toggle"]);
    mode_row.set_model(Some(&mode_list));
    mode_row.set_selected(match config.hotkey.mode {
        HotkeyMode::PushToTalk => 0,
        HotkeyMode::Toggle => 1,
    });
    hotkey_group.add(&mode_row);

    let key_row = libadwaita::EntryRow::builder()
        .title("Key")
        .text(&config.hotkey.key)
        .build();
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

    autostart_row.set_active(autostart_desktop_path().exists());

    autostart_row.connect_active_notify(move |row| {
        if row.is_active() {
            let _ = create_autostart_entry();
        } else {
            let _ = remove_autostart_entry();
        }
    });

    autostart_group.add(&autostart_row);
    page.add(&autostart_group);

    // Info about save behavior
    let info_group = libadwaita::PreferencesGroup::builder().build();
    let info_row = libadwaita::ActionRow::builder()
        .title("Settings are saved when you close this window")
        .css_classes(vec!["dim-label".to_string()])
        .build();
    info_group.add(&info_row);
    page.add(&info_group);

    (page, mode_row, key_row)
}

// ─── Recognition Settings Page ──────────────────────────────────────────────

fn build_recognition_page(
    config: &Config,
) -> (
    libadwaita::PreferencesPage,
    libadwaita::ComboRow,
    libadwaita::EntryRow,
    libadwaita::EntryRow,
    libadwaita::EntryRow,
    libadwaita::EntryRow,
) {
    let page = libadwaita::PreferencesPage::builder()
        .title("Recognition")
        .icon_name("audio-input-microphone-symbolic")
        .build();

    let engine_group = libadwaita::PreferencesGroup::builder()
        .title("Speech Recognition Engine")
        .build();

    let engine_row = libadwaita::ComboRow::builder()
        .title("Engine")
        .build();
    let engine_list = gtk4::StringList::new(&["Whisper Local", "OpenAI API"]);
    engine_row.set_model(Some(&engine_list));
    let current_engine = match config.recognition.engine {
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

    let wl = config.recognition.whisper_local.clone().unwrap_or(WhisperLocalConfig {
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

    let oa = config.recognition.openai_api.clone().unwrap_or(OpenAiApiConfig {
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

    // Toggle visibility based on engine
    let whisper_group_clone = whisper_group.clone();
    let openai_group_clone = openai_group.clone();
    let update_visibility = move |selected: u32| {
        whisper_group_clone.set_visible(selected == 0);
        openai_group_clone.set_visible(selected == 1);
    };
    update_visibility(current_engine);

    engine_row.connect_selected_notify(move |row| {
        update_visibility(row.selected());
    });

    (page, engine_row, model_path_row, whisper_lang_row, openai_key_row, openai_lang_row)
}

// ─── AI Settings Page ───────────────────────────────────────────────────────

fn build_ai_page(
    config: &Config,
    window: &libadwaita::PreferencesWindow,
) -> (
    libadwaita::PreferencesPage,
    libadwaita::ComboRow,
    libadwaita::EntryRow,
    libadwaita::ComboRow,
    libadwaita::EntryRow,
    libadwaita::EntryRow,
    libadwaita::ComboRow,
    libadwaita::EntryRow,
) {
    let page = libadwaita::PreferencesPage::builder()
        .title("AI")
        .icon_name("applications-science-symbolic")
        .build();

    let engine_group = libadwaita::PreferencesGroup::builder()
        .title("AI Post-Processing Engine")
        .build();

    let engine_row = libadwaita::ComboRow::builder()
        .title("Engine")
        .build();
    let engine_list = gtk4::StringList::new(&["Claude", "Ollama"]);
    engine_row.set_model(Some(&engine_list));
    let current_engine = match config.ai.engine {
        AiEngine::Claude => 0,
        AiEngine::Ollama => 1,
    };
    engine_row.set_selected(current_engine);
    engine_group.add(&engine_row);
    page.add(&engine_group);

    // ── Claude settings ──

    let claude_group = libadwaita::PreferencesGroup::builder()
        .title("Claude")
        .build();

    let cc = config.ai.claude.clone().unwrap_or(ClaudeConfig {
        api_key_env: "ANTHROPIC_API_KEY".to_string(),
        model: "claude-sonnet-4-6".to_string(),
    });

    let claude_key_row = libadwaita::EntryRow::builder()
        .title("API key env variable")
        .text(&cc.api_key_env)
        .build();

    // Model dropdown: known models + "Other"
    let is_known_claude = CLAUDE_MODELS.contains(&cc.model.as_str());
    let mut claude_items: Vec<&str> = CLAUDE_MODELS.to_vec();
    claude_items.push("Other");
    let claude_model_list = gtk4::StringList::new(&claude_items);

    let claude_model_combo = libadwaita::ComboRow::builder()
        .title("Model")
        .model(&claude_model_list)
        .build();

    if is_known_claude {
        let idx = CLAUDE_MODELS.iter().position(|&m| m == cc.model).unwrap_or(0);
        claude_model_combo.set_selected(idx as u32);
    } else {
        claude_model_combo.set_selected(CLAUDE_MODELS.len() as u32); // "Other"
    }

    let claude_model_custom = libadwaita::EntryRow::builder()
        .title("Custom model name")
        .text(if is_known_claude { "" } else { &cc.model })
        .build();
    claude_model_custom.set_visible(!is_known_claude);

    let custom_vis = claude_model_custom.clone();
    let other_idx = CLAUDE_MODELS.len() as u32;
    claude_model_combo.connect_selected_notify(move |combo| {
        custom_vis.set_visible(combo.selected() == other_idx);
    });

    claude_group.add(&claude_key_row);
    claude_group.add(&claude_model_combo);
    claude_group.add(&claude_model_custom);

    // Test Connection button for Claude
    let test_claude_row = libadwaita::ActionRow::builder()
        .title("Test Connection")
        .subtitle("Send a test request to verify API key and model")
        .activatable(true)
        .build();
    let test_icon = gtk4::Image::from_icon_name("network-transmit-receive-symbolic");
    test_claude_row.add_suffix(&test_icon);

    let window_for_claude_test = window.clone();
    let claude_key_for_test = claude_key_row.clone();
    let claude_combo_for_test = claude_model_combo.clone();
    let claude_custom_for_test = claude_model_custom.clone();
    test_claude_row.connect_activated(move |row| {
        let api_key_env = claude_key_for_test.text().to_string();
        let model = read_model_selection(&claude_combo_for_test, &claude_custom_for_test);

        row.set_subtitle("Testing...");
        let row_clone = row.clone();
        let window = window_for_claude_test.clone();

        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(test_claude_api(&api_key_env, &model));
        });

        gtk4::glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
            match rx.try_recv() {
                Ok(Ok(response)) => {
                    row_clone.set_subtitle("Test passed!");
                    window.add_toast(libadwaita::Toast::new(&format!("Claude API OK: {}", response)));
                    gtk4::glib::ControlFlow::Break
                }
                Ok(Err(e)) => {
                    let short = format!("Claude: {}", truncate(&e, 60));
                    row_clone.set_subtitle(&format!("Test failed: {}", truncate(&e, 80)));
                    show_error_toast(&window, &short, &e);
                    gtk4::glib::ControlFlow::Break
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => gtk4::glib::ControlFlow::Continue,
                Err(_) => gtk4::glib::ControlFlow::Break,
            }
        });
    });

    claude_group.add(&test_claude_row);
    page.add(&claude_group);

    // ── Ollama settings ──

    let ollama_group = libadwaita::PreferencesGroup::builder()
        .title("Ollama")
        .build();

    let ol = config.ai.ollama.clone().unwrap_or(OllamaConfig {
        host: "http://localhost:11434".to_string(),
        model: "qwen2.5:14b".to_string(),
    });

    let ollama_host_row = libadwaita::EntryRow::builder()
        .title("Host URL")
        .text(&ol.host)
        .build();

    // Model dropdown: start with current model + "Other", fetch list in background
    let ollama_model_list = gtk4::StringList::new(&[ol.model.as_str(), "Other"]);
    let ollama_model_combo = libadwaita::ComboRow::builder()
        .title("Model")
        .model(&ollama_model_list)
        .build();
    ollama_model_combo.set_selected(0);

    let ollama_model_custom = libadwaita::EntryRow::builder()
        .title("Custom model name")
        .build();
    ollama_model_custom.set_visible(false);

    let custom_vis = ollama_model_custom.clone();
    ollama_model_combo.connect_selected_notify(move |combo| {
        let is_other = combo
            .selected_item()
            .and_then(|item| item.downcast_ref::<gtk4::StringObject>().map(|s| s.string()))
            .map_or(false, |s| s == "Other");
        custom_vis.set_visible(is_other);
    });

    // Fetch installed Ollama models in the background
    {
        let host = ol.host.clone();
        let current_model = ol.model.clone();
        let list = ollama_model_list.clone();
        let combo = ollama_model_combo.clone();
        let custom_entry = ollama_model_custom.clone();

        let (tx, rx) = std::sync::mpsc::channel::<Vec<String>>();
        std::thread::spawn(move || {
            if let Ok(models) = fetch_ollama_models(&host) {
                let _ = tx.send(models);
            }
        });

        gtk4::glib::timeout_add_local(std::time::Duration::from_millis(200), move || {
            match rx.try_recv() {
                Ok(models) => {
                    while list.n_items() > 0 {
                        list.remove(0);
                    }
                    for m in &models {
                        list.append(m);
                    }
                    list.append("Other");
                    if let Some(idx) = models.iter().position(|m| *m == current_model) {
                        combo.set_selected(idx as u32);
                    } else {
                        // Current model not in fetched list: fall back to "Other"
                        // and pre-populate the custom entry so the model name is preserved.
                        combo.set_selected(models.len() as u32); // "Other"
                        custom_entry.set_text(&current_model);
                    }
                    gtk4::glib::ControlFlow::Break
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => gtk4::glib::ControlFlow::Continue,
                Err(_) => gtk4::glib::ControlFlow::Break,
            }
        });
    }

    // Test Connection button for Ollama
    let test_ollama_row = libadwaita::ActionRow::builder()
        .title("Test Connection")
        .subtitle("Send a test request to verify Ollama is reachable")
        .activatable(true)
        .build();
    let test_icon = gtk4::Image::from_icon_name("network-transmit-receive-symbolic");
    test_ollama_row.add_suffix(&test_icon);

    let window_for_ollama_test = window.clone();
    let ollama_host_for_test = ollama_host_row.clone();
    let ollama_combo_for_test = ollama_model_combo.clone();
    let ollama_custom_for_test = ollama_model_custom.clone();
    test_ollama_row.connect_activated(move |row| {
        let host = ollama_host_for_test.text().to_string();
        let model = read_model_selection(&ollama_combo_for_test, &ollama_custom_for_test);

        row.set_subtitle("Testing...");
        let row_clone = row.clone();
        let window = window_for_ollama_test.clone();

        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(test_ollama_api(&host, &model));
        });

        gtk4::glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
            match rx.try_recv() {
                Ok(Ok(response)) => {
                    row_clone.set_subtitle("Test passed!");
                    window.add_toast(libadwaita::Toast::new(&format!("Ollama OK: {}", response)));
                    gtk4::glib::ControlFlow::Break
                }
                Ok(Err(e)) => {
                    let short = format!("Ollama: {}", truncate(&e, 60));
                    row_clone.set_subtitle(&format!("Test failed: {}", truncate(&e, 80)));
                    show_error_toast(&window, &short, &e);
                    gtk4::glib::ControlFlow::Break
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => gtk4::glib::ControlFlow::Continue,
                Err(_) => gtk4::glib::ControlFlow::Break,
            }
        });
    });

    ollama_group.add(&ollama_host_row);
    ollama_group.add(&ollama_model_combo);
    ollama_group.add(&ollama_model_custom);
    ollama_group.add(&test_ollama_row);
    page.add(&ollama_group);

    // Toggle visibility
    let claude_group_clone = claude_group.clone();
    let ollama_group_clone = ollama_group.clone();
    let update_visibility = move |selected: u32| {
        claude_group_clone.set_visible(selected == 0);
        ollama_group_clone.set_visible(selected == 1);
    };
    update_visibility(current_engine);

    engine_row.connect_selected_notify(move |row| {
        update_visibility(row.selected());
    });

    (
        page,
        engine_row,
        claude_key_row,
        claude_model_combo,
        claude_model_custom,
        ollama_host_row,
        ollama_model_combo,
        ollama_model_custom,
    )
}

// ─── API Test Functions ─────────────────────────────────────────────────────

fn test_claude_api(api_key_env: &str, model: &str) -> Result<String, String> {
    let api_key = crate::config::resolve_api_key(api_key_env)
        .map_err(|e| e.to_string())?;

    crate::ai::claude::test_connectivity(&api_key, model)
        .map(|text| truncate(&text, 60).to_string())
        .map_err(|e| e.to_string())
}

fn test_ollama_api(host: &str, model: &str) -> Result<String, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("HTTP client error: {}", e))?;

    let url = format!("{}/api/generate", host.trim_end_matches('/'));

    let body = serde_json::json!({
        "model": model,
        "prompt": "Reply with exactly: OK",
        "stream": false,
        "options": { "num_predict": 16 }
    });

    let response = client
        .post(&url)
        .json(&body)
        .send()
        .map_err(|e| format!("Request failed: {}", e))?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().unwrap_or_default();
        return Err(format!("API error ({}): {}", status, truncate(&body, 100)));
    }

    let resp: serde_json::Value = response
        .json()
        .map_err(|e| format!("Invalid response: {}", e))?;

    let text = resp["response"]
        .as_str()
        .unwrap_or("(no response)");

    Ok(truncate(text, 60).to_string())
}

fn fetch_ollama_models(host: &str) -> Result<Vec<String>, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .map_err(|e| e.to_string())?;

    let url = format!("{}/api/tags", host.trim_end_matches('/'));
    let resp: serde_json::Value = client
        .get(&url)
        .send()
        .map_err(|e| e.to_string())?
        .json()
        .map_err(|e| e.to_string())?;

    Ok(resp["models"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|m| m["name"].as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default())
}

/// Show an error toast with a "Copy" button that copies the full error to clipboard.
fn show_error_toast(window: &libadwaita::PreferencesWindow, short_msg: &str, full_msg: &str) {
    let toast = libadwaita::Toast::builder()
        .title(short_msg)
        .button_label("Copy Error")
        .timeout(0) // stay until dismissed
        .build();

    let full_msg = full_msg.to_string();
    let display = gtk4::prelude::WidgetExt::display(window);
    toast.connect_button_clicked(move |_| {
        let clipboard = display.clipboard();
        clipboard.set_text(&full_msg);
    });

    window.add_toast(toast);
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        s
    } else {
        let mut end = max;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        &s[..end]
    }
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
                    Err(_) => {}
                }
            }
            row.set_subtitle("Press to test mic input level");
            *is_rec = false;
        } else {
            if let Some(ref mut r) = *rec {
                if r.start().is_err() {
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
    let local_bin = dirs::home_dir()
        .unwrap_or_default()
        .join(".local/bin/koe");
    let exe = if local_bin.exists() {
        local_bin
    } else {
        std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("koe"))
    };
    let content = format!(
        "[Desktop Entry]\nType=Application\nName=koe\nExec={}\nX-GNOME-Autostart-enabled=true\n",
        exe.display()
    );
    std::fs::write(&path, content)?;
    Ok(())
}

fn remove_autostart_entry() -> anyhow::Result<()> {
    let path = autostart_desktop_path();
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    Ok(())
}
