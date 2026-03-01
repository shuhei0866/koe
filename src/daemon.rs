use anyhow::{Context, Result};
use std::sync::mpsc;
use std::time::Duration;

use crate::ipc;
use crate::{ai, audio, config, context, dictionary, hotkey, input, recognition};

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
    // Load dictionaries
    let dict_paths = config.dictionary_paths();
    let dictionary = dictionary::Dictionary::load(&dict_paths).context("loading dictionaries")?;

    // Initialize speech recognizer
    let recognizer =
        recognition::create_recognizer(&config.recognition).context("creating recognizer")?;
    tracing::info!(
        "Speech recognizer ready: {:?}",
        config.recognition.engine
    );

    // Initialize AI processor
    let processor = ai::create_processor(&config.ai).context("creating AI processor")?;
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
                                    match processor
                                        .process(&corrected, &window_ctx, &dictionary)
                                        .await
                                    {
                                        Ok(processed_text) => {
                                            tracing::info!(
                                                "Processed text: {}",
                                                processed_text
                                            );
                                            state = AppState::Typing;

                                            if let Err(e) =
                                                input::type_text(&processed_text)
                                            {
                                                tracing::error!(
                                                    "Failed to type text: {}",
                                                    e
                                                );
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
