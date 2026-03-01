#![allow(unused_assignments)]

mod ai;
mod audio;
mod config;
mod context;
mod dictionary;
mod hotkey;
mod input;
mod recognition;

use anyhow::{Context, Result};
use std::sync::mpsc;
use std::time::Duration;

/// Application state machine.
#[derive(Debug, Clone, PartialEq)]
enum AppState {
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

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    tracing::info!("koe - Ubuntu Voice Input System starting...");

    // Load config
    let config = config::Config::load().context("loading config")?;
    tracing::info!("Config loaded: recognition={:?}, ai={:?}", config.recognition.engine, config.ai.engine);

    // Load dictionaries
    let dict_paths = config.dictionary_paths();
    let dictionary = dictionary::Dictionary::load(&dict_paths).context("loading dictionaries")?;

    // Initialize speech recognizer
    let recognizer =
        recognition::create_recognizer(&config.recognition).context("creating recognizer")?;
    tracing::info!("Speech recognizer ready: {:?}", config.recognition.engine);

    // Initialize AI processor
    let processor = ai::create_processor(&config.ai).context("creating AI processor")?;
    tracing::info!("AI processor ready: {:?}", config.ai.engine);

    // Initialize audio recorder
    let mut recorder = audio::AudioRecorder::new().context("creating audio recorder")?;

    // Start hotkey listener
    let hotkey_rx = hotkey::start_hotkey_listener(config.hotkey.mode.clone(), &config.hotkey.key)
        .context("starting hotkey listener")?;

    tracing::info!(
        "Ready! Press {} to start/stop recording (mode: {:?})",
        config.hotkey.key,
        config.hotkey.mode
    );

    let mut state = AppState::Idle;

    // Main event loop
    loop {
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
                                    match processor
                                        .process(&corrected, &window_ctx, &dictionary)
                                        .await
                                    {
                                        Ok(processed_text) => {
                                            tracing::info!("Processed text: {}", processed_text);
                                            state = AppState::Typing;

                                            // Type the result into the active window
                                            if let Err(e) = input::type_text(&processed_text) {
                                                tracing::error!("Failed to type text: {}", e);
                                                // Fallback to clipboard paste
                                                if let Err(e2) =
                                                    input::paste_text(&processed_text)
                                                {
                                                    tracing::error!(
                                                        "Clipboard paste also failed: {}",
                                                        e2
                                                    );
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            tracing::error!("AI processing failed: {}", e);
                                            // Fallback: type raw text
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
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // Normal timeout, continue loop
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                tracing::error!("Hotkey listener disconnected");
                break;
            }
        }
    }

    Ok(())
}
