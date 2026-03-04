use anyhow::{Context, Result};
use std::sync::mpsc;
use std::time::Duration;
use tokio::task::JoinHandle;

use crate::ipc;
use crate::{ai, audio, config, context, dbus, dictionary, hotkey, input, memory, recognition, sound};

#[cfg(feature = "gui")]
mod indicator_bridge {
    use crate::ui::indicator::IndicatorWindow;

    pub enum IndicatorMsg {
        StateChanged(String),
        AudioLevel(f32),
        Shutdown,
    }

    /// Spawn a dedicated GTK thread for the indicator window.
    ///
    /// Returns a Sender to communicate with the GTK thread, or None if
    /// the indicator is disabled or GTK init fails.
    pub fn start_indicator_thread(
        enabled: bool,
    ) -> Option<async_channel::Sender<IndicatorMsg>> {
        if !enabled {
            return None;
        }
        let (tx, rx) = async_channel::bounded::<IndicatorMsg>(64);
        std::thread::Builder::new()
            .name("indicator-gtk".into())
            .spawn(move || {
                if gtk4::init().is_err() {
                    tracing::warn!("GTK init failed for indicator thread");
                    return;
                }
                tracing::info!("Indicator thread started");
                let ctx = gtk4::glib::MainContext::default();
                let indicator = IndicatorWindow::new();
                ctx.spawn_local(async move {
                    tracing::debug!("Indicator receiver loop started");
                    while let Ok(msg) = rx.recv().await {
                        match msg {
                            IndicatorMsg::StateChanged(state) => {
                                tracing::debug!("Indicator: state → {}", state);
                                indicator.show_state(&state);
                            }
                            IndicatorMsg::AudioLevel(level) => {
                                indicator.update_audio_level(level);
                            }
                            IndicatorMsg::Shutdown => break,
                        }
                    }
                });
                let main_loop = gtk4::glib::MainLoop::new(None, false);
                main_loop.run();
            })
            .ok()?;
        Some(tx)
    }
}

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

/// Notify D-Bus, tray icon, and indicator window of a state change.
async fn notify_state_change(
    state: &AppState,
    dbus_emitter: &Option<dbus::DbusEmitter>,
    #[cfg(feature = "gui")] tray_handle: &Option<ksni::Handle<crate::ui::tray::KoeTray>>,
    #[cfg(feature = "gui")] indicator_tx: &Option<async_channel::Sender<indicator_bridge::IndicatorMsg>>,
) {
    if let Some(ref emitter) = dbus_emitter {
        emitter.emit_state_changed(&state.to_string()).await;
    }
    #[cfg(feature = "gui")]
    if let Some(ref handle) = tray_handle {
        crate::ui::tray::update_tray_icon(handle, &state.to_string()).await;
    }
    #[cfg(feature = "gui")]
    if let Some(ref tx) = indicator_tx {
        let _ = tx.try_send(indicator_bridge::IndicatorMsg::StateChanged(
            state.to_string(),
        ));
    }
}

pub async fn run_daemon(mut config: config::Config) -> Result<()> {
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
    let rms_rx = recorder.rms_receiver();

    // Start hotkey listener
    let hotkey_rx = hotkey::start_hotkey_listener(config.hotkey.mode.clone(), &config.hotkey.key)
        .context("starting hotkey listener")?;

    // Create shutdown channel
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

    // Spawn signal handler (SIGTERM + SIGINT)
    {
        let shutdown_tx = shutdown_tx.clone();
        tokio::spawn(async move {
            let mut sigterm = tokio::signal::unix::signal(
                tokio::signal::unix::SignalKind::terminate(),
            )
            .expect("failed to register SIGTERM handler");

            tokio::select! {
                _ = sigterm.recv() => {
                    tracing::info!("Received SIGTERM, shutting down...");
                }
                result = tokio::signal::ctrl_c() => {
                    if let Err(e) = result {
                        tracing::error!("Failed to listen for SIGINT: {}", e);
                        return;
                    }
                    tracing::info!("Received SIGINT, shutting down...");
                }
            }

            let _ = shutdown_tx.send(true);
        });
    }

    // Start IPC server
    let ipc_rx = ipc::server::start(shutdown_rx.clone())
        .await
        .context("starting IPC server")?;

    // Initialize D-Bus emitter for state notifications
    let dbus_emitter = match dbus::DbusEmitter::new().await {
        Ok(emitter) => {
            tracing::info!("D-Bus emitter ready");
            Some(emitter)
        }
        Err(e) => {
            tracing::warn!("D-Bus emitter unavailable (non-fatal): {}", e);
            None
        }
    };

    // Start system tray (if gui feature enabled)
    #[cfg(feature = "gui")]
    let tray_handle = {
        match crate::ui::tray::start_tray(shutdown_tx.clone()).await {
            Ok(handle) => {
                tracing::info!("System tray started");
                Some(handle)
            }
            Err(e) => {
                tracing::error!("Failed to start system tray: {}", e);
                None
            }
        }
    };
    #[cfg(not(feature = "gui"))]
    let _tray_handle: Option<()> = None;

    // Start indicator window thread (if gui feature enabled)
    #[cfg(feature = "gui")]
    let indicator_tx =
        indicator_bridge::start_indicator_thread(config.feedback.indicator_enabled);

    // Audio level forwarding task handle (active only during Recording)
    let mut audio_level_handle: Option<JoinHandle<()>> = None;

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
    let mut last_consolidation_entry_count: usize = 0;

    // Main event loop
    loop {
        // Check for shutdown signal (from SIGTERM/SIGINT/tray).
        // Worst-case latency is ~100ms (hotkey recv_timeout below).
        if *shutdown_rx.borrow() {
            tracing::info!("Shutdown signal received, exiting main loop");
            break;
        }

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

                            // Re-apply Whisper hint after reload
                            if new_config.memory.enabled && !mem.terms.is_empty() {
                                let hint = mem.format_for_whisper_hint();
                                recognizer.set_prompt_hint(&hint);
                                tracing::info!("Whisper hint restored: {} terms", mem.terms.len());
                            }

                            config = new_config;
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
                let _ = shutdown_tx.send(true);
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
                        notify_state_change(&state, &dbus_emitter, #[cfg(feature = "gui")] &tray_handle, #[cfg(feature = "gui")] &indicator_tx).await;
                    } else {
                        // Recording started successfully
                        sound::play_if_enabled("message-new-instant", config.feedback.sound_enabled);
                        notify_state_change(&state, &dbus_emitter, #[cfg(feature = "gui")] &tray_handle, #[cfg(feature = "gui")] &indicator_tx).await;

                        // Spawn audio level forwarding task (~30fps)
                        let mut rms_rx_clone = rms_rx.clone();
                        let dbus_emitter_clone = dbus_emitter.clone();
                        #[cfg(feature = "gui")]
                        let indicator_tx_clone = indicator_tx.clone();
                        audio_level_handle = Some(tokio::spawn(async move {
                            loop {
                                if rms_rx_clone.changed().await.is_err() {
                                    break;
                                }
                                let level = *rms_rx_clone.borrow_and_update();
                                if let Some(ref emitter) = dbus_emitter_clone {
                                    emitter.emit_audio_level(level as f64).await;
                                }
                                #[cfg(feature = "gui")]
                                if let Some(ref tx) = indicator_tx_clone {
                                    let _ = tx.try_send(
                                        indicator_bridge::IndicatorMsg::AudioLevel(level),
                                    );
                                }
                                tokio::time::sleep(Duration::from_millis(33)).await;
                            }
                        }));
                    }
                }
            }
            Ok(hotkey::HotkeyEvent::RecordStop) => {
                if state == AppState::Recording {
                    // Stop audio level forwarding task
                    if let Some(handle) = audio_level_handle.take() {
                        handle.abort();
                    }

                    tracing::info!("<<< Recording stopped, processing...");
                    state = AppState::Processing;
                    sound::play_if_enabled("complete", config.feedback.sound_enabled);
                    notify_state_change(&state, &dbus_emitter, #[cfg(feature = "gui")] &tray_handle, #[cfg(feature = "gui")] &indicator_tx).await;

                    match recorder.stop() {
                        Ok(audio_data) => {
                            if audio_data.samples.is_empty() {
                                tracing::warn!("No audio captured");
                                state = AppState::Idle;
                                notify_state_change(&state, &dbus_emitter, #[cfg(feature = "gui")] &tray_handle, #[cfg(feature = "gui")] &indicator_tx).await;
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
                                        notify_state_change(&state, &dbus_emitter, #[cfg(feature = "gui")] &tray_handle, #[cfg(feature = "gui")] &indicator_tx).await;
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

                                            // Save learnings to memory (only when memory is enabled)
                                            if config.memory.enabled && !result.learnings.is_empty() {
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
                                                if let Err(e) = mem.save() {
                                                    tracing::error!("Failed to save memory: {}", e);
                                                }

                                                // Update Whisper hint with new terms
                                                let hint = mem.format_for_whisper_hint();
                                                recognizer.set_prompt_hint(&hint);
                                                tracing::debug!("Whisper hint updated: {} terms", mem.terms.len());
                                            }

                                            state = AppState::Typing;
                                            notify_state_change(&state, &dbus_emitter, #[cfg(feature = "gui")] &tray_handle, #[cfg(feature = "gui")] &indicator_tx).await;

                                            if let Err(e) =
                                                input::paste_text(&result.text)
                                            {
                                                tracing::error!(
                                                    "Failed to paste text: {}",
                                                    e
                                                );
                                                if let Err(e2) =
                                                    input::type_text(&result.text)
                                                {
                                                    tracing::error!(
                                                        "Direct type also failed: {}",
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
                                            notify_state_change(&state, &dbus_emitter, #[cfg(feature = "gui")] &tray_handle, #[cfg(feature = "gui")] &indicator_tx).await;
                                            let _ = input::paste_text(&corrected);
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
                    notify_state_change(&state, &dbus_emitter, #[cfg(feature = "gui")] &tray_handle, #[cfg(feature = "gui")] &indicator_tx).await;
                    tracing::info!("Ready for next input");

                    // Check if memory needs consolidation
                    // Only trigger if entries grew since last consolidation
                    let current_entries = mem.total_entries();
                    if config.memory.enabled
                        && mem.needs_consolidation(config.memory.consolidation_threshold)
                        && current_entries > last_consolidation_entry_count
                    {
                        tracing::info!(
                            "Memory has {} entries (threshold: {}), starting consolidation...",
                            current_entries,
                            config.memory.consolidation_threshold
                        );
                        match processor.consolidate_memory(&mem.format_for_prompt()).await {
                            Ok(Some(result)) => {
                                // Guard: reject consolidation results that would lose data
                                let would_lose_terms = result.terms.is_empty() && !mem.terms.is_empty();
                                if (result.terms.is_empty() && result.context_markdown.trim().is_empty())
                                    || would_lose_terms
                                {
                                    tracing::warn!(
                                        "Consolidation result rejected (terms: {} → {}, context empty: {})",
                                        mem.terms.len(),
                                        result.terms.len(),
                                        result.context_markdown.trim().is_empty()
                                    );
                                } else {
                                    let old_terms = std::mem::take(&mut mem.terms);
                                    let old_context = std::mem::take(&mut mem.context);
                                    // Only replace terms if consolidation produced non-empty terms;
                                    // an empty map likely means the LLM omitted the terms JSON.
                                    if result.terms.is_empty() {
                                        tracing::warn!("Consolidation returned empty terms, keeping existing {} terms", old_terms.len());
                                        mem.terms = old_terms.clone();
                                    } else {
                                        mem.terms = result.terms;
                                    }
                                    mem.context = memory::Memory::parse_context_markdown(&result.context_markdown);
                                    if let Err(e) = mem.save() {
                                        tracing::error!("Failed to save consolidated memory, rolling back: {}", e);
                                        mem.terms = old_terms;
                                        mem.context = old_context;
                                    } else {
                                        tracing::info!(
                                            "Memory consolidated: {} → {} entries",
                                            current_entries,
                                            mem.total_entries()
                                        );
                                    }
                                    // Update Whisper hint after consolidation
                                    let hint = mem.format_for_whisper_hint();
                                    recognizer.set_prompt_hint(&hint);
                                }
                                last_consolidation_entry_count = mem.total_entries();
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

    // Stop audio level task if still running
    if let Some(handle) = audio_level_handle.take() {
        handle.abort();
    }

    // Shutdown indicator window
    #[cfg(feature = "gui")]
    if let Some(ref tx) = indicator_tx {
        let _ = tx.try_send(indicator_bridge::IndicatorMsg::Shutdown);
    }

    // Cleanup socket on exit
    ipc::server::cleanup_socket();
    tracing::info!("Daemon stopped");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_state_display_idle() {
        assert_eq!(AppState::Idle.to_string(), "Idle");
    }

    #[test]
    fn test_app_state_display_recording() {
        assert_eq!(AppState::Recording.to_string(), "Recording");
    }

    #[test]
    fn test_app_state_display_processing() {
        assert_eq!(AppState::Processing.to_string(), "Processing");
    }

    #[test]
    fn test_app_state_display_typing() {
        assert_eq!(AppState::Typing.to_string(), "Typing");
    }

    #[test]
    fn test_app_state_display_matches_dbus_expectations() {
        // The D-Bus emitter and tray icon both use string representations.
        // Verify that our Display impl produces the exact strings expected
        // by those APIs.
        let states = [
            (AppState::Idle, "Idle"),
            (AppState::Recording, "Recording"),
            (AppState::Processing, "Processing"),
            (AppState::Typing, "Typing"),
        ];
        for (state, expected) in &states {
            assert_eq!(
                &state.to_string(),
                expected,
                "AppState::{:?} should display as {:?}",
                state,
                expected
            );
        }
    }

    #[test]
    fn test_sound_events_for_state_transitions() {
        // Verify that sound::play_if_enabled doesn't panic for the events we use.
        // The actual canberra-gtk-play may not be available in test env,
        // but the function should handle that gracefully.
        sound::play_if_enabled("message-new-instant", false);
        sound::play_if_enabled("complete", false);
    }

    #[test]
    fn test_sound_events_enabled() {
        // Even with enabled=true, should not panic (fire-and-forget).
        sound::play_if_enabled("message-new-instant", true);
        sound::play_if_enabled("complete", true);
    }
}
