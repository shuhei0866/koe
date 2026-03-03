use anyhow::{Context, Result};
use std::sync::mpsc;
use std::time::Duration;

use crate::ipc;
use crate::{ai, audio, config, context, dictionary, hotkey, input, memory, recognition};

/// Application state machine.
#[derive(Debug, Clone, PartialEq)]
pub enum AppState {
    Idle,
    Recording,
    Processing,
    Typing,
}

impl std::fmt::Display for AppState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AppState::Idle => write!(f, "Idle"),
            AppState::Recording => write!(f, "Recording"),
            AppState::Processing => write!(f, "Processing"),
            AppState::Typing => write!(f, "Typing"),
        }
    }
}

pub async fn run_daemon(config: config::Config) -> Result<()> {
    // Load memory (auto-learned data)
    let memory_dir = config.memory_dir();
    let mut mem = if config.memory.enabled {
        memory::Memory::load(&memory_dir).context("loading memory")?
    } else {
        memory::Memory::default()
    };
    tracing::info!(
        "Memory loaded: {} terms, {} context sections",
        mem.terms.len(),
        mem.context.sections.len()
    );

    // Load dictionaries
    let dict_paths = config.dictionary_paths();
    let mut dictionary =
        dictionary::Dictionary::load(&dict_paths).context("loading dictionaries")?;

    // Initialize speech recognizer
    let mut recognizer =
        recognition::create_recognizer(&config.recognition).context("creating recognizer")?;
    tracing::info!(
        "Speech recognizer ready: {:?}",
        config.recognition.engine
    );

    // Initialize AI processor
    let mut processor = ai::create_processor(&config.ai).context("creating AI processor")?;
    tracing::info!("AI processor ready: {:?}", config.ai.engine);

    // Initialize audio recorder
    let mut recorder = audio::AudioRecorder::new().context("creating audio recorder")?;

    // Start hotkey listener
    let hotkey_rx = hotkey::start_hotkey_listener(config.hotkey.mode.clone(), &config.hotkey.key)
        .context("starting hotkey listener")?;

    // Start IPC server
    let ipc_rx = ipc::server::start().await.context("starting IPC server")?;

    // Start system tray (if gui feature enabled)
    #[cfg(feature = "gui")]
    crate::ui::tray::start_tray();

    // Set initial Whisper hint from memory
    if config.memory.enabled && !mem.terms.is_empty() {
        let hint = mem.format_for_whisper_hint();
        recognizer.set_prompt_hint(&hint);
        tracing::info!("Whisper hint set: {} terms", mem.terms.len());
    }

    tracing::info!(
        "Ready! Press {} to start/stop recording (mode: {:?})",
        config.hotkey.key,
        config.hotkey.mode
    );

    let mut state = AppState::Idle;

    // Main event loop
    loop {
        // Check for IPC messages (non-blocking)
        match ipc_rx.try_recv() {
            Ok(ipc::IpcRequest::GetStatus) => {
                tracing::info!("IPC: GetStatus request (state={})", state);
            }
            Ok(ipc::IpcRequest::ReloadConfig) => {
                tracing::info!("IPC: ReloadConfig request received");
                if state != AppState::Idle {
                    tracing::warn!(
                        "Skipping config reload: state is {} (must be Idle)",
                        state
                    );
                } else {
                    match config::Config::load() {
                        Ok(new_config) => {
                            // Reload dictionary
                            let dict_paths = new_config.dictionary_paths();
                            match dictionary::Dictionary::load(&dict_paths) {
                                Ok(new_dict) => {
                                    dictionary = new_dict;
                                    tracing::info!("Dictionary reloaded");
                                }
                                Err(e) => {
                                    tracing::error!("Failed to reload dictionary: {}", e);
                                }
                            }

                            // Reload recognizer
                            match recognition::create_recognizer(&new_config.recognition) {
                                Ok(new_recognizer) => {
                                    recognizer = new_recognizer;
                                    tracing::info!(
                                        "Recognizer reloaded: {:?}",
                                        new_config.recognition.engine
                                    );
                                }
                                Err(e) => {
                                    tracing::error!("Failed to reload recognizer: {}", e);
                                }
                            }

                            // Reload AI processor
                            match ai::create_processor(&new_config.ai) {
                                Ok(new_processor) => {
                                    processor = new_processor;
                                    tracing::info!(
                                        "AI processor reloaded: {:?}",
                                        new_config.ai.engine
                                    );
                                }
                                Err(e) => {
                                    tracing::error!("Failed to reload AI processor: {}", e);
                                }
                            }

                            // Reload memory
                            if new_config.memory.enabled {
                                let new_memory_dir = new_config.memory_dir();
                                match memory::Memory::load(&new_memory_dir) {
                                    Ok(new_mem) => {
                                        mem = new_mem;
                                        tracing::info!("Memory reloaded");
                                    }
                                    Err(e) => {
                                        tracing::error!("Failed to reload memory: {}", e);
                                    }
                                }
                            }

                            tracing::info!("Config reloaded successfully");
                        }
                        Err(e) => {
                            tracing::error!("Failed to load config: {}", e);
                        }
                    }
                }
            }
            Ok(ipc::IpcRequest::Shutdown) => {
                tracing::info!("IPC: Shutdown request received");
                break;
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                tracing::warn!("IPC channel disconnected");
            }
        }

        match hotkey_rx.recv_timeout(Duration::from_millis(100)) {
            Ok(hotkey::HotkeyEvent::RecordStart) => {
                if state == AppState::Idle {
                    tracing::info!(">>> Recording started");
                    state = AppState::Recording;
                    if let Err(e) = recorder.start() {
                        tracing::error!("Failed to start recording: {}", e);
                        state = AppState::Idle;
                    }
                }
            }
            Ok(hotkey::HotkeyEvent::RecordStop) => {
                if state == AppState::Recording {
                    tracing::info!("<<< Recording stopped, processing...");
                    state = AppState::Processing;

                    match recorder.stop() {
                        Ok(audio_data) => {
                            if audio_data.samples.is_empty() {
                                tracing::warn!("No audio captured");
                                state = AppState::Idle;
                                continue;
                            }

                            // Get window context before processing
                            let window_ctx =
                                context::get_active_window_context().unwrap_or_default();

                            // Speech recognition
                            match recognizer.transcribe(&audio_data).await {
                                Ok(raw_text) => {
                                    if raw_text.is_empty() {
                                        tracing::warn!("Empty transcription");
                                        state = AppState::Idle;
                                        continue;
                                    }

                                    tracing::info!("Raw transcription: {}", raw_text);

                                    // Apply dictionary term corrections first
                                    let corrected = dictionary.apply_terms(&raw_text);

                                    // AI post-processing
                                    let memory_context = mem.format_for_prompt();
                                    match processor
                                        .process(&corrected, &window_ctx, &dictionary, &memory_context)
                                        .await
                                    {
                                        Ok(result) => {
                                            tracing::info!(
                                                "Processed text: {} (learnings: {})",
                                                result.text,
                                                result.learnings.len()
                                            );

                                            // Save learnings to memory
                                            for learning in &result.learnings {
                                                match learning {
                                                    ai::Learning::Term { from, to } => {
                                                        tracing::info!("Learned term: {} → {}", from, to);
                                                        mem.add_term(from, to);
                                                    }
                                                    ai::Learning::Context { category, content } => {
                                                        tracing::info!("Learned context [{}]: {}", category, content);
                                                        mem.add_context(category, content);
                                                    }
                                                }
                                            }
                                            if !result.learnings.is_empty() {
                                                if let Err(e) = mem.save() {
                                                    tracing::error!("Failed to save memory: {}", e);
                                                }

                                                // Update Whisper hint with new terms
                                                let hint = mem.format_for_whisper_hint();
                                                recognizer.set_prompt_hint(&hint);
                                                tracing::debug!("Whisper hint updated: {} terms", mem.terms.len());
                                            }

                                            state = AppState::Typing;

                                            if let Err(e) =
                                                input::type_text(&result.text)
                                            {
                                                tracing::error!(
                                                    "Failed to type text: {}",
                                                    e
                                                );
                                                if let Err(e2) =
                                                    input::paste_text(&result.text)
                                                {
                                                    tracing::error!(
                                                        "Clipboard paste also failed: {}",
                                                        e2
                                                    );
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            tracing::error!(
                                                "AI processing failed: {}",
                                                e
                                            );
                                            tracing::info!(
                                                "Falling back to raw transcription"
                                            );
                                            state = AppState::Typing;
                                            let _ = input::type_text(&corrected);
                                        }
                                    }
                                }
                                Err(e) => {
                                    tracing::error!("Transcription failed: {}", e);
                                }
                            }
                        }
                        Err(e) => {
                            tracing::error!("Failed to stop recording: {}", e);
                        }
                    }

                    state = AppState::Idle;
                    tracing::info!("Ready for next input");

                    // Check if memory needs consolidation
                    if config.memory.enabled
                        && mem.needs_consolidation(config.memory.consolidation_threshold)
                    {
                        tracing::info!(
                            "Memory has {} entries (threshold: {}), starting consolidation...",
                            mem.total_entries(),
                            config.memory.consolidation_threshold
                        );
                        match processor.consolidate_memory(&mem.format_for_prompt()).await {
                            Ok(Some(result)) => {
                                let old_count = mem.total_entries();
                                // Rebuild memory from consolidation result
                                mem.terms = result.terms;
                                mem.context = memory::Memory::parse_context_markdown(&result.context_markdown);
                                if let Err(e) = mem.save() {
                                    tracing::error!("Failed to save consolidated memory: {}", e);
                                } else {
                                    tracing::info!(
                                        "Memory consolidated: {} → {} entries",
                                        old_count,
                                        mem.total_entries()
                                    );
                                }
                                // Update Whisper hint after consolidation
                                let hint = mem.format_for_whisper_hint();
                                recognizer.set_prompt_hint(&hint);
                            }
                            Ok(None) => {
                                tracing::debug!("Consolidation skipped (processor returned None)");
                            }
                            Err(e) => {
                                tracing::error!("Memory consolidation failed: {}", e);
                            }
                        }
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                tracing::error!("Hotkey listener disconnected");
                break;
            }
        }
    }

    Ok(())
}
