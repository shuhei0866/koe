//! Consent-based GNOME Wayland input; no raw-device or compositor bypasses.
use anyhow::{Context, Result};
use ashpd::desktop::{
    clipboard::{Clipboard, RequestClipboardOptions, SetSelectionOptions},
    global_shortcuts::{BindShortcutsOptions, GlobalShortcuts, NewShortcut},
    remote_desktop::{
        DeviceType, KeyState, NotifyKeyboardKeysymOptions, RemoteDesktop, SelectDevicesOptions,
        StartOptions,
    },
    CreateSessionOptions, PersistMode, Session,
};
use futures_util::StreamExt;
use std::{
    collections::HashMap,
    sync::{mpsc, Arc},
    time::Duration,
};
use tokio::{
    io::AsyncWriteExt,
    sync::{Mutex, OnceCell},
};

use crate::{
    config::{HotkeyMode, WaylandPasteShortcut},
    hotkey::HotkeyEvent,
};

pub const APP_ID: &str = "io.github.shuhei0866.koe";
const SHORTCUT_ID: &str = "dictation";
static CONNECTION: OnceCell<zbus::Connection> = OnceCell::const_new();
static KEYBOARD: OnceCell<std::result::Result<PortalKeyboard, String>> = OnceCell::const_new();

fn wayland_session(session: Option<&str>, display: Option<&str>) -> bool {
    match session {
        Some("wayland") => true,
        Some("x11") => false,
        _ => display.is_some_and(|value| !value.is_empty()),
    }
}

pub fn is_wayland() -> bool {
    wayland_session(
        std::env::var("XDG_SESSION_TYPE").ok().as_deref(),
        std::env::var("WAYLAND_DISPLAY").ok().as_deref(),
    )
}

async fn connection() -> Result<&'static zbus::Connection> {
    CONNECTION
        .get_or_try_init(|| async {
            let connection = zbus::Connection::session()
                .await
                .context("connecting to desktop session bus")?;
            let registry = zbus::Proxy::new(
                &connection,
                "org.freedesktop.portal.Desktop",
                "/org/freedesktop/portal/desktop",
                "org.freedesktop.host.portal.Registry",
            )
            .await?;
            let options: HashMap<&str, zbus::zvariant::Value<'_>> = HashMap::new();
            registry
                .call::<_, _, ()>("Register", &(APP_ID, options))
                .await
                .context("registering Koe desktop identity; install its desktop file first")?;
            drop(registry);
            Ok(connection)
        })
        .await
}

struct PortalKeyboard {
    desktop: RemoteDesktop,
    session: Arc<Session<RemoteDesktop>>,
    clipboard: Arc<Clipboard>,
    text: Arc<Mutex<String>>,
    paste_lock: Mutex<()>,
}

impl PortalKeyboard {
    async fn new() -> Result<Self> {
        let desktop = RemoteDesktop::with_connection(connection().await?.clone()).await?;
        let session = Arc::new(
            desktop
                .create_session(CreateSessionOptions::default())
                .await?,
        );
        let result = Self::initialize(desktop, session.clone()).await;
        if result.is_err() {
            let _ = session.close().await;
        }
        result
    }

    async fn initialize(
        desktop: RemoteDesktop,
        session: Arc<Session<RemoteDesktop>>,
    ) -> Result<Self> {
        desktop
            .select_devices(
                &session,
                SelectDevicesOptions::default()
                    .set_devices(Some(DeviceType::Keyboard.into()))
                    .set_persist_mode(PersistMode::DoNot),
            )
            .await?
            .response()?;
        let clipboard = Arc::new(Clipboard::with_connection(connection().await?.clone()).await?);
        clipboard
            .request(&session, RequestClipboardOptions::default())
            .await?;
        tracing::info!("Waiting for keyboard-only Wayland input permission");
        let selected = desktop
            .start(&session, None, StartOptions::default())
            .await?
            .response()?;
        if selected.devices() != DeviceType::Keyboard
            || !selected.streams().is_empty()
            || !selected.is_clipboard_enabled()
        {
            session.close().await?;
            anyhow::bail!("Portal did not grant keyboard-only input and clipboard access");
        }
        tracing::info!("Wayland keyboard input permission granted");
        let text = Arc::new(Mutex::new(String::new()));
        let server_clipboard = clipboard.clone();
        let server_session = session.clone();
        let server_text = text.clone();
        let session_id = serde_json::to_value(session.as_ref())?;
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel::<Result<()>>();
        tokio::spawn(async move {
            let mut transfers = match server_clipboard
                .receive_selection_transfer::<RemoteDesktop>()
                .await
            {
                Ok(stream) => Box::pin(stream),
                Err(error) => {
                    let _ = ready_tx.send(Err(error.into()));
                    return;
                }
            };
            let mut closed = match server_session.receive_closed().await {
                Ok(stream) => stream,
                Err(error) => {
                    let _ = ready_tx.send(Err(error.into()));
                    return;
                }
            };
            let _ = ready_tx.send(Ok(()));
            loop {
                let transfer = tokio::select! {
                    transfer = transfers.next() => match transfer { Some(t) => t, None => break },
                    _ = closed.next() => break,
                };
                let (owner, mime, serial) = transfer;
                if !serde_json::to_value(&owner).is_ok_and(|value| value == session_id) {
                    continue;
                }
                let result: Result<()> = async {
                    anyhow::ensure!(
                        TEXT_MIMES.contains(&mime.as_str()),
                        "Unsupported clipboard format"
                    );
                    let text = server_text.lock().await.clone();
                    let fd: std::os::fd::OwnedFd = server_clipboard
                        .selection_write(&server_session, serial)
                        .await?
                        .into();
                    let mut file = tokio::fs::File::from_std(std::fs::File::from(fd));
                    tokio::time::timeout(Duration::from_secs(5), file.write_all(text.as_bytes()))
                        .await??;
                    file.shutdown().await?;
                    Ok(())
                }
                .await;
                let _ = server_clipboard
                    .selection_write_done(&server_session, serial, result.is_ok())
                    .await;
                if let Err(error) = result {
                    tracing::warn!("Wayland clipboard transfer failed: {error}");
                }
            }
        });
        ready_rx.await??;
        Ok(Self {
            desktop,
            session,
            clipboard,
            text,
            paste_lock: Mutex::new(()),
        })
    }
}

async fn keyboard() -> Result<&'static PortalKeyboard> {
    // Remember refusal for this process. Typing must not cause repeated dialogs
    // or silently fall back to XWayland after consent has been declined.
    let result = KEYBOARD
        .get_or_init(|| async {
            PortalKeyboard::new()
                .await
                .map_err(|error| format!("{error:#}"))
        })
        .await;
    result.as_ref().map_err(|error| {
        anyhow::anyhow!("Wayland input unavailable: {error}; restart Koe to retry")
    })
}

async fn wait_for_unlocked_session() -> Result<()> {
    if !std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .split(':')
        .any(|name| name == "GNOME")
    {
        return Ok(());
    }
    let saver = zbus::Proxy::new(
        connection().await?,
        "org.gnome.ScreenSaver",
        "/org/gnome/ScreenSaver",
        "org.gnome.ScreenSaver",
    )
    .await?;
    let mut changes = saver.receive_signal("ActiveChanged").await?;
    let mut reported = false;
    while saver.call::<_, _, bool>("GetActive", &()).await? {
        if !reported {
            tracing::info!(
                "Waiting for the user to unlock GNOME before requesting Wayland input permission"
            );
            reported = true;
        }
        anyhow::ensure!(
            changes.next().await.is_some(),
            "Lock-state connection closed"
        );
    }
    Ok(())
}

const TEXT_MIMES: &[&str] = &["text/plain;charset=utf-8", "text/plain"];

fn validate_text(text: &str) -> Result<()> {
    anyhow::ensure!(
        !text
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t')),
        "Wayland paste rejects non-text control characters; the transcript remains in history"
    );
    Ok(())
}

fn paste_keys(shortcut: WaylandPasteShortcut) -> &'static [i32] {
    match shortcut {
        WaylandPasteShortcut::CtrlV => &[0xffe3, 0x76],
        WaylandPasteShortcut::CtrlShiftV => &[0xffe3, 0xffe1, 0x76],
    }
}

pub async fn paste_text(text: &str, shortcut: WaylandPasteShortcut) -> Result<()> {
    validate_text(text)?;
    if text.is_empty() {
        return Ok(());
    }
    let keyboard = keyboard().await?;
    let _guard = keyboard.paste_lock.lock().await;
    *keyboard.text.lock().await = text.to_owned();
    keyboard
        .clipboard
        .set_selection(
            &keyboard.session,
            SetSelectionOptions::default().set_mime_types(TEXT_MIMES),
        )
        .await?;
    let mut pressed_keys = Vec::new();
    let mut result = Ok(());
    for &symbol in paste_keys(shortcut) {
        // Track even a failed request: the compositor may have received it.
        pressed_keys.push(symbol);
        if let Err(error) = keyboard
            .desktop
            .notify_keyboard_keysym(
                &keyboard.session,
                symbol,
                KeyState::Pressed,
                NotifyKeyboardKeysymOptions::default(),
            )
            .await
        {
            result = Err(anyhow::Error::from(error));
            break;
        }
    }
    for symbol in pressed_keys.into_iter().rev() {
        let released = keyboard
            .desktop
            .notify_keyboard_keysym(
                &keyboard.session,
                symbol,
                KeyState::Released,
                NotifyKeyboardKeysymOptions::default(),
            )
            .await;
        if let Err(error) = released {
            if result.is_ok() {
                result = Err(error.into());
            }
        }
    }
    result.context("pasting Wayland text")
}

#[derive(Default)]
struct ShortcutState {
    down: bool,
    recording: bool,
}

impl ShortcutState {
    fn event(&mut self, mode: &HotkeyMode, pressed: bool) -> Option<HotkeyEvent> {
        if self.down == pressed {
            return None;
        }
        self.down = pressed;
        if pressed {
            self.recording = if *mode == HotkeyMode::PushToTalk {
                true
            } else {
                !self.recording
            };
            Some(if self.recording {
                HotkeyEvent::RecordStart
            } else {
                HotkeyEvent::RecordStop
            })
        } else if *mode == HotkeyMode::PushToTalk && self.recording {
            self.recording = false;
            Some(HotkeyEvent::RecordStop)
        } else {
            None
        }
    }
}

fn preferred_trigger(key: &str) -> &str {
    match key {
        "ScrollLock" => "Scroll_Lock",
        "PrintScreen" => "Print",
        other => other,
    }
}

pub async fn run_hotkeys(
    mode: HotkeyMode,
    key: String,
    sender: mpsc::Sender<HotkeyEvent>,
) -> Result<()> {
    wait_for_unlocked_session().await?;
    let keyboard = keyboard().await?;
    let result = shortcut_loop(keyboard, mode, &key, sender).await;
    // Revocation or a failed bind must not leave an unused control session alive.
    let _ = keyboard.session.close().await;
    result
}

async fn shortcut_loop(
    keyboard: &PortalKeyboard,
    mode: HotkeyMode,
    key: &str,
    sender: mpsc::Sender<HotkeyEvent>,
) -> Result<()> {
    let portal = GlobalShortcuts::with_connection(connection().await?.clone()).await?;
    let session = portal
        .create_session(CreateSessionOptions::default())
        .await?;
    let result = shortcut_events(keyboard, &portal, &session, mode, key, sender).await;
    let _ = session.close().await;
    result
}

async fn shortcut_events(
    keyboard: &PortalKeyboard,
    portal: &GlobalShortcuts,
    session: &Session<GlobalShortcuts>,
    mode: HotkeyMode,
    key: &str,
    sender: mpsc::Sender<HotkeyEvent>,
) -> Result<()> {
    let session_id = serde_json::to_value(session)?;
    let mut activated = portal.receive_activated().await?;
    let mut deactivated = portal.receive_deactivated().await?;
    let mut closed = session.receive_closed().await?;
    let mut keyboard_closed = keyboard.session.receive_closed().await?;
    tracing::info!("Waiting for Wayland voice shortcut permission");
    let shortcuts = [
        NewShortcut::new(SHORTCUT_ID, "音声入力を録音").preferred_trigger(preferred_trigger(key))
    ];
    portal
        .bind_shortcuts(session, &shortcuts, None, BindShortcutsOptions::default())
        .await?
        .response()?;
    tracing::info!("Wayland voice shortcut ready");
    let mut state = ShortcutState::default();
    loop {
        let pressed = tokio::select! {
            event = activated.next() => match event {
                Some(event) if event.shortcut_id() == SHORTCUT_ID
                    && session_id.as_str() == Some(event.session_handle().as_str()) => true,
                Some(_) => continue,
                None => break,
            },
            event = deactivated.next() => match event {
                Some(event) if event.shortcut_id() == SHORTCUT_ID
                    && session_id.as_str() == Some(event.session_handle().as_str()) => false,
                Some(_) => continue,
                None => break,
            },
            _ = closed.next() => break,
            _ = keyboard_closed.next() => break,
        };
        if let Some(event) = state.event(&mode, pressed) {
            sender
                .send(event)
                .context("recording event receiver disconnected")?;
        }
    }
    if state.recording {
        let _ = sender.send(HotkeyEvent::RecordStop);
    }
    anyhow::bail!("Wayland shortcut session closed; restart Koe to reconnect")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_x11_session_does_not_request_portal_permissions() {
        assert!(!wayland_session(Some("x11"), Some("stale-wayland-0")));
        assert!(wayland_session(Some("wayland"), None));
        assert!(wayland_session(None, Some("wayland-0")));
    }
    #[test]
    fn japanese_and_multiline_are_clipboard_text() {
        assert!(validate_text("日本語。\n二行目\t✓").is_ok());
        assert!(!paste_keys(WaylandPasteShortcut::CtrlV).contains(&0xff0d));
    }
    #[tokio::test]
    async fn controls_are_rejected_before_requesting_input_permission() {
        assert!(paste_text("safe\u{1b}unsafe", WaylandPasteShortcut::CtrlV)
            .await
            .unwrap_err()
            .to_string()
            .contains("control"));
    }
    #[test]
    fn push_to_talk_has_one_start_and_one_stop() {
        let mut state = ShortcutState::default();
        assert!(matches!(
            state.event(&HotkeyMode::PushToTalk, true),
            Some(HotkeyEvent::RecordStart)
        ));
        assert!(state.event(&HotkeyMode::PushToTalk, true).is_none());
        assert!(matches!(
            state.event(&HotkeyMode::PushToTalk, false),
            Some(HotkeyEvent::RecordStop)
        ));
        assert!(state.event(&HotkeyMode::PushToTalk, false).is_none());
    }
    #[test]
    fn toggle_changes_only_on_press_edges() {
        let mut state = ShortcutState::default();
        assert!(matches!(
            state.event(&HotkeyMode::Toggle, true),
            Some(HotkeyEvent::RecordStart)
        ));
        assert!(state.event(&HotkeyMode::Toggle, true).is_none());
        assert!(state.event(&HotkeyMode::Toggle, false).is_none());
        assert!(matches!(
            state.event(&HotkeyMode::Toggle, true),
            Some(HotkeyEvent::RecordStop)
        ));
    }
}
