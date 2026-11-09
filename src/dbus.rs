use zbus::{
    connection, fdo, interface, message::Header, names::BusName, object_server::SignalEmitter,
    Connection,
};

/// Only GNOME Shell (our bundled extension runs inside it) may report the
/// focused app. Other session clients could otherwise pick our paste key.
const SHELL_BUS_NAME: &str = "org.gnome.Shell";

/// D-Bus interface for koe daemon state notifications.
///
/// Exposed on the session bus at `com.github.koe.Daemon`
/// with object path `/com/github/koe/Daemon`.
struct KoeDaemon {
    state: String,
    /// WM_CLASS of the focused window as reported by the GNOME Shell
    /// extension; None until it reports, or when nothing has focus.
    focused_app: Option<String>,
}

/// Whether `header` comes from the current owner of `org.gnome.Shell`.
async fn sent_by_shell(connection: &Connection, header: &Header<'_>) -> bool {
    let Some(sender) = header.sender() else {
        return false;
    };
    let Ok(dbus) = fdo::DBusProxy::new(connection).await else {
        return false;
    };
    let Ok(shell) = BusName::try_from(SHELL_BUS_NAME) else {
        return false;
    };
    match dbus.get_name_owner(shell).await {
        Ok(owner) => owner.as_str() == sender.as_str(),
        Err(_) => false,
    }
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

    /// Called by the GNOME Shell extension whenever focus moves, so the
    /// daemon can pick Ctrl+Shift+V for terminals on Wayland. An empty string
    /// means no window has focus (or the extension is being disabled).
    async fn set_focused_app(
        &mut self,
        wm_class: String,
        #[zbus(connection)] connection: &Connection,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<()> {
        if !sent_by_shell(connection, &header).await {
            return Err(fdo::Error::AccessDenied(
                "SetFocusedApp is only accepted from GNOME Shell".into(),
            ));
        }
        tracing::debug!("Focused app: {:?}", wm_class);
        self.focused_app = (!wm_class.is_empty()).then_some(wm_class);
        Ok(())
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
                    focused_app: None,
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

    /// The focused app last reported by the GNOME Shell extension.
    pub async fn focused_app(&self) -> Option<String> {
        let object_server = self.connection.object_server();
        let iface_ref = object_server
            .interface::<_, KoeDaemon>("/com/github/koe/Daemon")
            .await
            .ok()?;
        let focused = iface_ref.get().await.focused_app.clone();
        focused
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
