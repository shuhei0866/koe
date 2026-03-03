use zbus::{connection, interface, object_server::SignalEmitter, Connection};

/// D-Bus interface for koe daemon state notifications.
///
/// Exposed on the session bus at `com.github.koe.Daemon`
/// with object path `/com/github/koe/Daemon`.
struct KoeDaemon;

#[interface(name = "com.github.koe.Daemon")]
impl KoeDaemon {
    /// Emitted when the daemon state changes (Idle, Recording, Processing, Typing).
    #[zbus(signal)]
    async fn state_changed(emitter: &SignalEmitter<'_>, state: &str) -> zbus::Result<()>;

    /// Emitted with the current audio RMS level during recording (~30fps).
    #[zbus(signal)]
    async fn audio_level(emitter: &SignalEmitter<'_>, level: f64) -> zbus::Result<()>;
}

/// Emitter handle for sending D-Bus signals from the daemon.
pub struct DbusEmitter {
    connection: Connection,
}

impl DbusEmitter {
    /// Connect to the session bus, claim the well-known name, and serve the interface.
    pub async fn new() -> zbus::Result<Self> {
        let connection = connection::Builder::session()?
            .name("com.github.koe.Daemon")?
            .serve_at("/com/github/koe/Daemon", KoeDaemon)?
            .build()
            .await?;
        Ok(Self { connection })
    }

    /// Emit a `StateChanged` signal with the given state string.
    pub async fn emit_state_changed(&self, state: &str) {
        let object_server = self.connection.object_server();
        if let Ok(iface_ref) = object_server
            .interface::<_, KoeDaemon>("/com/github/koe/Daemon")
            .await
        {
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
        assert!(emitter.is_ok(), "Failed to create DbusEmitter: {:?}", emitter.err());
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
