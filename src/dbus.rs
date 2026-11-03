use zbus::{connection, interface, object_server::SignalEmitter, Connection};

/// D-Bus interface for koe daemon state notifications.
///
/// Exposed on the session bus at `com.github.koe.Daemon`
/// with object path `/com/github/koe/Daemon`.
struct KoeDaemon {
    state: String,
}

#[interface(name = "com.github.koe.Daemon")]
impl KoeDaemon {
    /// The current daemon state (Idle, Recording, Processing, Typing), so a
    /// client that starts mid-recording can show it without waiting for the
    /// next `StateChanged`. Updates are delivered by `StateChanged`, not by
    /// `PropertiesChanged`.
    #[zbus(property(emits_changed_signal = "false"))]
    async fn state(&self) -> String {
        self.state.clone()
    }

    /// Emitted when the daemon state changes (Idle, Recording, Processing, Typing).
    #[zbus(signal)]
    async fn state_changed(emitter: &SignalEmitter<'_>, state: &str) -> zbus::Result<()>;

    /// Emitted with the current audio RMS level during recording (~30fps).
    #[zbus(signal)]
    async fn audio_level(emitter: &SignalEmitter<'_>, level: f64) -> zbus::Result<()>;

    /// Text recognized so far while recording. Display only; it may change
    /// until the recording stops, and the final text is what gets typed.
    #[zbus(signal)]
    async fn live_transcript(emitter: &SignalEmitter<'_>, text: &str) -> zbus::Result<()>;
}

/// Emitter handle for sending D-Bus signals from the daemon.
#[derive(Clone)]
pub struct DbusEmitter {
    connection: Connection,
}

impl DbusEmitter {
    /// Connect to the session bus, claim the well-known name, and serve the interface.
    pub async fn new() -> zbus::Result<Self> {
        let connection = connection::Builder::session()?
            .name("com.github.koe.Daemon")?
            .serve_at(
                "/com/github/koe/Daemon",
                KoeDaemon {
                    state: "Idle".to_string(),
                },
            )?
            .build()
            .await?;
        Ok(Self { connection })
    }

    /// Record the new state and emit a `StateChanged` signal for it.
    pub async fn emit_state_changed(&self, state: &str) {
        let object_server = self.connection.object_server();
        if let Ok(iface_ref) = object_server
            .interface::<_, KoeDaemon>("/com/github/koe/Daemon")
            .await
        {
            iface_ref.get_mut().await.state = state.to_string();
            let ctxt = iface_ref.signal_emitter();
            let _ = KoeDaemon::state_changed(ctxt, state).await;
        }
    }

    /// Emit an `AudioLevel` signal with the given RMS level.
    pub async fn emit_audio_level(&self, level: f64) {
        let object_server = self.connection.object_server();
        if let Ok(iface_ref) = object_server
            .interface::<_, KoeDaemon>("/com/github/koe/Daemon")
            .await
        {
            let ctxt = iface_ref.signal_emitter();
            let _ = KoeDaemon::audio_level(ctxt, level).await;
        }
    }

    /// Emit a `LiveTranscript` signal with the text recognized so far.
    pub async fn emit_live_transcript(&self, text: &str) {
        let object_server = self.connection.object_server();
        if let Ok(iface_ref) = object_server
            .interface::<_, KoeDaemon>("/com/github/koe/Daemon")
            .await
        {
            let ctxt = iface_ref.signal_emitter();
            let _ = KoeDaemon::live_transcript(ctxt, text).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// D-Bus tests require a running session bus, so they are ignored by default.
    /// Run with: cargo test test_dbus_emitter_creation -- --ignored
    #[tokio::test]
    #[ignore]
    async fn test_dbus_emitter_creation() {
        let emitter = DbusEmitter::new().await;
        assert!(
            emitter.is_ok(),
            "Failed to create DbusEmitter: {:?}",
            emitter.err()
        );
    }

    /// Verify that emit methods don't panic even without a real bus connection.
    /// This test is ignored because it needs a session bus.
    #[tokio::test]
    #[ignore]
    async fn test_dbus_emit_signals() {
        let emitter = DbusEmitter::new().await.expect("DbusEmitter::new failed");
        emitter.emit_state_changed("Recording").await;
        emitter.emit_audio_level(0.42).await;
    }
}
