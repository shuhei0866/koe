//! Consent-based GNOME Wayland input; no raw-device or compositor bypasses.
use anyhow::{Context, Result};
use ashpd::desktop::{
    clipboard::{Clipboard, RequestClipboardOptions, SetSelectionOptions},
    global_shortcuts::{BindShortcutsOptions, GlobalShortcuts, NewShortcut},
    remote_desktop::{
        DeviceType, KeyState, NotifyKeyboardKeysymOptions, RemoteDesktop, SelectDevicesOptions,
        StartOptions,
    },
    CreateSessionOptions, PersistMode, ResponseError, Session,
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
/// The live keyboard session, or `None` while (re)connecting or after giving up.
static KEYBOARD: Mutex<Option<Arc<PortalKeyboard>>> = Mutex::const_new(None);

/// First retry delay after a failed or closed portal session; doubles per failure.
const RECONNECT_BASE_DELAY: Duration = Duration::from_secs(2);
const RECONNECT_MAX_DELAY: Duration = Duration::from_secs(60);
/// Consecutive failed attempts before giving up until an explicit restart.
const RECONNECT_MAX_FAILURES: u32 = 6;

/// A failure that retrying cannot fix without the user's involvement, such as a
/// declined permission dialog. Retrying would only show the dialog again.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
struct Permanent(String);

fn is_permanent(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause.downcast_ref::<Permanent>().is_some()
            || cause
                .downcast_ref::<ashpd::Error>()
                .is_some_and(is_permanent_portal_error)
    })
}

/// Portal answers that stay the same however often we ask: a declined dialog,
/// a desktop without the portal (or with too old a version), or a request the
/// portal rejects outright. Transport failures and `ResponseError::Other` are
/// left to the reconnect loop, because a restarting portal produces those.
fn is_permanent_portal_error(error: &ashpd::Error) -> bool {
    use ashpd::{Error, PortalError};
    matches!(
        error,
        Error::Response(ResponseError::Cancelled)
            | Error::PortalNotFound(_)
            | Error::RequiresVersion(..)
            | Error::InvalidAppID
            | Error::Portal(
                PortalError::Cancelled(_)
                    | PortalError::NotAllowed(_)
                    | PortalError::InvalidArgument(_)
            )
    )
}

/// Classify a failed `Registry.Register` call. A D-Bus method error is the
/// portal's answer (typically: no desktop file for `APP_ID`) and will not
/// change on retry; anything else is a transport failure, such as the portal
/// restarting mid-call, and is worth retrying.
fn registration_error(error: zbus::Error) -> anyhow::Error {
    match error {
        zbus::Error::MethodError(..) => Permanent(format!(
            "registering Koe desktop identity; install its desktop file first: {error}"
        ))
        .into(),
        other => anyhow::Error::from(other).context("registering Koe desktop identity"),
    }
}

/// The portal must grant exactly what we asked for: a keyboard, no screen
/// streams, and the clipboard. Anything else is the user's choice in the
/// dialog, so asking again would only repeat it.
fn ensure_keyboard_only_grant(
    devices: ashpd::enumflags2::BitFlags<DeviceType>,
    has_streams: bool,
    clipboard_enabled: bool,
) -> Result<()> {
    if devices != DeviceType::Keyboard || has_streams || !clipboard_enabled {
        return Err(Permanent(
            "Portal did not grant keyboard-only input and clipboard access".into(),
        )
        .into());
    }
    Ok(())
}

fn reconnect_delay(failures: u32) -> Duration {
    let factor = 1u32 << failures.saturating_sub(1).min(5);
    (RECONNECT_BASE_DELAY * factor).min(RECONNECT_MAX_DELAY)
}

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
    let connection = CONNECTION
        .get_or_try_init(|| async {
            zbus::Connection::session()
                .await
                .context("connecting to desktop session bus")
        })
        .await?;
    ensure_registered(connection).await?;
    Ok(connection)
}

/// Register Koe's desktop identity with the current portal process.
///
/// The registration belongs to the portal process, not to our connection, so a
/// restarted portal needs it again before any new session is created.
async fn ensure_registered(connection: &zbus::Connection) -> Result<()> {
    static REGISTERED_OWNER: Mutex<Option<String>> = Mutex::const_new(None);
    let owner = portal_owner(connection).await?;
    let mut registered = REGISTERED_OWNER.lock().await;
    if registered.as_deref() == Some(owner.as_str()) {
        return Ok(());
    }
    let registry = zbus::Proxy::new(
        connection,
        "org.freedesktop.portal.Desktop",
        "/org/freedesktop/portal/desktop",
        "org.freedesktop.host.portal.Registry",
    )
    .await?;
    let options: HashMap<&str, zbus::zvariant::Value<'_>> = HashMap::new();
    registry
        .call::<_, _, ()>("Register", &(APP_ID, options))
        .await
        .map_err(registration_error)?;
    *registered = Some(owner);
    Ok(())
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
        // `PortalKeyboard::new` closes the session on any error. Closing here
        // with `?` would let a failed close replace this permanent refusal
        // with a retryable error and show the dialog again.
        ensure_keyboard_only_grant(
            selected.devices(),
            !selected.streams().is_empty(),
            selected.is_clipboard_enabled(),
        )?;
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

/// The keyboard session owned by `run_hotkeys`. Pasting never opens a session
/// itself, so typing cannot cause extra permission dialogs or silently fall
/// back to XWayland.
async fn keyboard() -> Result<Arc<PortalKeyboard>> {
    KEYBOARD.lock().await.clone().ok_or_else(|| {
        anyhow::anyhow!("Wayland input is not connected; the transcript remains in history")
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
    let mut failures = 0u32;
    loop {
        let mut ready = false;
        let error = connect_and_listen(&mode, &key, &sender, &mut ready)
            .await
            .unwrap_err();
        if is_permanent(&error) {
            return Err(error.context("restart Koe to retry"));
        }
        // A session that reached the ready state worked; its closure starts a
        // fresh retry budget instead of counting toward giving up.
        failures = if ready { 1 } else { failures + 1 };
        if failures > RECONNECT_MAX_FAILURES {
            return Err(error.context(format!(
                "gave up after {RECONNECT_MAX_FAILURES} reconnection attempts; restart Koe to retry"
            )));
        }
        let delay = reconnect_delay(failures);
        tracing::warn!(
            "Wayland voice input disconnected: {error:#}; reconnecting in {}s (attempt {failures}/{RECONNECT_MAX_FAILURES})",
            delay.as_secs()
        );
        tokio::time::sleep(delay).await;
    }
}

/// Open the keyboard and shortcut sessions and forward shortcut events until
/// either session ends. Always returns the reason it stopped.
async fn connect_and_listen(
    mode: &HotkeyMode,
    key: &str,
    sender: &mpsc::Sender<HotkeyEvent>,
    ready: &mut bool,
) -> Result<std::convert::Infallible> {
    wait_for_unlocked_session().await?;
    let keyboard = Arc::new(PortalKeyboard::new().await?);
    *KEYBOARD.lock().await = Some(keyboard.clone());
    let result = shortcut_loop(&keyboard, mode, key, sender, ready).await;
    KEYBOARD.lock().await.take();
    // Revocation or a failed bind must not leave an unused control session alive.
    let _ = keyboard.session.close().await;
    result
}

async fn shortcut_loop(
    keyboard: &PortalKeyboard,
    mode: &HotkeyMode,
    key: &str,
    sender: &mpsc::Sender<HotkeyEvent>,
    ready: &mut bool,
) -> Result<std::convert::Infallible> {
    let portal = GlobalShortcuts::with_connection(connection().await?.clone()).await?;
    let session = portal
        .create_session(CreateSessionOptions::default())
        .await?;
    let result = shortcut_events(keyboard, &portal, &session, mode, key, sender, ready).await;
    let _ = session.close().await;
    result
}

async fn shortcut_events(
    keyboard: &PortalKeyboard,
    portal: &GlobalShortcuts,
    session: &Session<GlobalShortcuts>,
    mode: &HotkeyMode,
    key: &str,
    sender: &mpsc::Sender<HotkeyEvent>,
    ready: &mut bool,
) -> Result<std::convert::Infallible> {
    let session_id: String = serde_json::from_value(serde_json::to_value(session)?)?;
    // One subscription preserves D-Bus arrival order. Selecting between two
    // per-member streams can observe a queued release before its press.
    let conn = connection().await?;
    let mut signals = ordered_shortcut_signals(conn).await?;
    let mut closed = session.receive_closed().await?;
    let mut keyboard_closed = keyboard.session.receive_closed().await?;
    // A crashed or restarted portal never sends Closed; its sessions just
    // vanish with the old bus name owner.
    let mut owner_changed = zbus::fdo::DBusProxy::new(conn)
        .await?
        .receive_name_owner_changed_with_args(&[(0, "org.freedesktop.portal.Desktop")])
        .await?;
    tracing::info!("Waiting for Wayland voice shortcut permission");
    let shortcuts = [
        NewShortcut::new(SHORTCUT_ID, "音声入力を録音").preferred_trigger(preferred_trigger(key))
    ];
    portal
        .bind_shortcuts(session, &shortcuts, None, BindShortcutsOptions::default())
        .await?
        .response()?;
    tracing::info!("Wayland voice shortcut ready");
    *ready = true;
    let mut state = ShortcutState::default();
    let reason = loop {
        let pressed = tokio::select! {
            signal = signals.next() => match signal {
                Some(Ok(message)) => match shortcut_signal(&message, &session_id) {
                    Ok(Some(pressed)) => pressed,
                    Ok(None) => continue,
                    Err(error) => break format!("invalid shortcut signal: {error}"),
                },
                Some(Err(error)) => break format!("shortcut connection failed: {error}"),
                None => break "shortcut signal stream ended".to_string(),
            },
            _ = closed.next() => break "the portal closed the shortcut session".to_string(),
            _ = keyboard_closed.next() => break "the portal closed the keyboard session".to_string(),
            _ = owner_changed.next() => break "the portal service restarted".to_string(),
        };
        if let Some(event) = state.event(mode, pressed) {
            if sender.send(event).is_err() {
                return Err(Permanent("recording event receiver disconnected".into()).into());
            }
        }
    };
    if state.recording {
        let _ = sender.send(HotkeyEvent::RecordStop);
    }
    anyhow::bail!("Wayland shortcut session ended: {reason}")
}

/// Unique bus name of the running portal, starting it first if needed.
///
/// `GetNameOwner` alone does not activate a service. After a login, the portal
/// can fail to start (it was activated before the graphical session was ready)
/// and stays down until something activates it again.
async fn portal_owner(connection: &zbus::Connection) -> Result<String> {
    let bus = zbus::fdo::DBusProxy::new(connection).await?;
    let name: zbus::names::BusName<'_> = "org.freedesktop.portal.Desktop".try_into()?;
    if let zbus::names::BusName::WellKnown(ref well_known) = name {
        if let Err(e) = bus.start_service_by_name(well_known.clone(), 0).await {
            tracing::debug!("Could not start the portal service: {}", e);
        }
    }
    Ok(bus.get_name_owner(name).await?.to_string())
}

async fn ordered_shortcut_signals(conn: &zbus::Connection) -> Result<zbus::MessageStream> {
    let owner = portal_owner(conn).await?;
    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .sender(owner.as_str())?
        .path("/org/freedesktop/portal/desktop")?
        .interface("org.freedesktop.portal.GlobalShortcuts")?
        .build();
    Ok(zbus::MessageStream::for_match_rule(rule, conn, Some(64)).await?)
}

fn shortcut_signal(message: &zbus::Message, session_id: &str) -> Result<Option<bool>> {
    let header = message.header();
    let pressed = match header.member().map(|member| member.as_str()) {
        Some("Activated") => true,
        Some("Deactivated") => false,
        _ => return Ok(None),
    };
    let (session, shortcut, _timestamp, _options): (
        zbus::zvariant::OwnedObjectPath,
        String,
        u64,
        HashMap<String, zbus::zvariant::OwnedValue>,
    ) = message.body().deserialize()?;
    Ok((session.as_str() == session_id && shortcut == SHORTCUT_ID).then_some(pressed))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct PrivateBus {
        child: std::process::Child,
        dir: std::path::PathBuf,
        address: String,
    }

    impl Drop for PrivateBus {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    impl PrivateBus {
        fn start() -> Self {
            use std::{
                io::{BufRead, BufReader},
                process::{Command, Stdio},
            };
            let dir =
                std::env::temp_dir().join(format!("koe-portal-test-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir(&dir).unwrap();
            let child = Command::new("dbus-daemon")
                .args([
                    "--session",
                    "--nofork",
                    "--print-address=1",
                    &format!("--address=unix:dir={}", dir.display()),
                ])
                .stdout(Stdio::piped())
                .spawn()
                .expect("dbus-daemon is a test dependency");
            let mut bus = PrivateBus {
                child,
                dir,
                address: String::new(),
            };
            let mut address = String::new();
            BufReader::new(bus.child.stdout.take().unwrap())
                .read_line(&mut address)
                .unwrap();
            bus.address = address.trim().to_owned();
            bus
        }

        async fn connect(&self) -> zbus::Connection {
            zbus::connection::Builder::address(self.address.as_str())
                .unwrap()
                .build()
                .await
                .unwrap()
        }

        /// A connection that owns the portal's bus name but serves nothing.
        async fn fake_portal(&self) -> zbus::Connection {
            let server = self.connect().await;
            server
                .request_name("org.freedesktop.portal.Desktop")
                .await
                .unwrap();
            server
        }
    }

    #[tokio::test]
    async fn dbus_subscription_keeps_a_queued_press_before_its_release() {
        let bus = PrivateBus::start();
        let server = bus.fake_portal().await;
        let client = bus.connect().await;
        let mut signals = ordered_shortcut_signals(&client).await.unwrap();
        const SESSION: &str = "/org/freedesktop/portal/desktop/session/test";
        // Queue the complete burst before polling the receiver. This also
        // checks the sender match and the real wire signature on a private bus.
        for member in ["Activated", "Deactivated", "Activated", "Deactivated"] {
            server
                .emit_signal(
                    None::<&str>,
                    "/org/freedesktop/portal/desktop",
                    "org.freedesktop.portal.GlobalShortcuts",
                    member,
                    &(
                        zbus::zvariant::OwnedObjectPath::try_from(SESSION).unwrap(),
                        SHORTCUT_ID,
                        42u64,
                        HashMap::<String, zbus::zvariant::OwnedValue>::new(),
                    ),
                )
                .await
                .unwrap();
        }
        for expected in [true, false, true, false] {
            let message = tokio::time::timeout(Duration::from_secs(5), signals.next())
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            assert_eq!(shortcut_signal(&message, SESSION).unwrap(), Some(expected));
        }
    }

    fn signal(member: &str, session: &str, shortcut: &str) -> zbus::Message {
        zbus::Message::signal(
            "/org/freedesktop/portal/desktop",
            "org.freedesktop.portal.GlobalShortcuts",
            member,
        )
        .unwrap()
        .build(&(
            zbus::zvariant::OwnedObjectPath::try_from(session).unwrap(),
            shortcut,
            42u64,
            HashMap::<String, zbus::zvariant::OwnedValue>::new(),
        ))
        .unwrap()
    }

    #[tokio::test]
    async fn queued_press_release_bursts_keep_both_edges_in_order() {
        const SESSION: &str = "/org/freedesktop/portal/desktop/session/test";
        for (mode, count) in [(HotkeyMode::PushToTalk, 4), (HotkeyMode::Toggle, 2)] {
            let mut signals = futures_util::stream::iter([
                signal("Activated", SESSION, SHORTCUT_ID),
                signal("Deactivated", SESSION, SHORTCUT_ID),
                signal("Activated", SESSION, SHORTCUT_ID),
                signal("Deactivated", SESSION, SHORTCUT_ID),
            ]);
            let mut state = ShortcutState::default();
            let mut events = Vec::new();
            while let Some(message) = signals.next().await {
                if let Some(pressed) = shortcut_signal(&message, SESSION).unwrap() {
                    if let Some(event) = state.event(&mode, pressed) {
                        events.push(event);
                    }
                }
            }
            assert_eq!(events.len(), count);
            assert!(matches!(events[0], HotkeyEvent::RecordStart));
            assert!(matches!(events[count - 1], HotkeyEvent::RecordStop));
            assert!(!state.recording);
        }
    }

    #[test]
    fn shortcut_signals_from_other_sessions_or_actions_are_ignored() {
        const SESSION: &str = "/org/freedesktop/portal/desktop/session/test";
        assert_eq!(
            shortcut_signal(&signal("Activated", "/other", SHORTCUT_ID), SESSION).unwrap(),
            None
        );
        assert_eq!(
            shortcut_signal(&signal("Activated", SESSION, "other"), SESSION).unwrap(),
            None
        );
        assert_eq!(
            shortcut_signal(&signal("Deactivated", SESSION, SHORTCUT_ID), SESSION).unwrap(),
            Some(false)
        );
    }
    #[test]
    fn explicit_x11_session_does_not_request_portal_permissions() {
        assert!(!wayland_session(Some("x11"), Some("stale-wayland-0")));
        assert!(wayland_session(Some("wayland"), None));
        assert!(wayland_session(None, Some("wayland-0")));
    }
    #[test]
    fn reconnect_delay_doubles_up_to_the_cap() {
        let delays: Vec<u64> = (1..=8).map(|n| reconnect_delay(n).as_secs()).collect();
        assert_eq!(delays, [2, 4, 8, 16, 32, 60, 60, 60]);
    }
    #[test]
    fn declined_dialog_is_permanent() {
        let cancelled = anyhow::Error::from(ashpd::Error::Response(ResponseError::Cancelled));
        assert!(is_permanent(&cancelled.context("starting session")));
        let refused = anyhow::Error::from(Permanent("not granted".into()));
        assert!(is_permanent(&refused));
    }
    #[test]
    fn portal_crashes_and_closed_sessions_are_retried() {
        let crashed = anyhow::Error::from(ashpd::Error::Response(ResponseError::Other));
        assert!(!is_permanent(&crashed));
        let closed = anyhow::anyhow!("Wayland shortcut session ended: the portal closed it");
        assert!(!is_permanent(&closed));
    }
    #[test]
    fn missing_or_outdated_portals_are_permanent() {
        use ashpd::{Error, PortalError};
        let missing =
            Error::PortalNotFound("org.freedesktop.portal.GlobalShortcuts".try_into().unwrap());
        for error in [
            missing,
            Error::RequiresVersion(2, 1),
            Error::Portal(PortalError::NotAllowed("no".into())),
            Error::Portal(PortalError::Cancelled("no".into())),
        ] {
            let error = anyhow::Error::from(error).context("binding shortcut");
            assert!(is_permanent(&error), "{error:#}");
        }
    }

    #[test]
    fn transient_portal_failures_are_retried() {
        use ashpd::{Error, PortalError};
        let io = zbus::Error::InputOutput(Arc::new(std::io::Error::other("bus reset")));
        for error in [
            Error::Portal(PortalError::Failed("portal busy".into())),
            Error::NoResponse,
            Error::Zbus(io),
        ] {
            let error = anyhow::Error::from(error).context("starting session");
            assert!(!is_permanent(&error), "{error:#}");
        }
    }

    #[test]
    fn registration_transport_failures_are_retried() {
        let io = zbus::Error::InputOutput(Arc::new(std::io::Error::other("portal restarting")));
        let error = registration_error(io);
        assert!(!is_permanent(&error));
        assert!(format!("{error:#}").contains("portal restarting"));
    }

    #[tokio::test]
    async fn registration_refused_by_the_portal_is_permanent() {
        let bus = PrivateBus::start();
        // The fake portal owns the name but has no Registry object, so the bus
        // answers with a D-Bus method error, as a real portal does when it
        // rejects the app id.
        let server = bus.fake_portal().await;
        // zbus starts answering method calls once the object server exists.
        let _ = server.object_server();
        let client = bus.connect().await;
        let error = tokio::time::timeout(Duration::from_secs(5), ensure_registered(&client))
            .await
            .expect("Register must be answered, not hang")
            .unwrap_err();
        assert!(is_permanent(&error), "{error:#}");
        assert!(format!("{error:#}").contains("desktop file"));
    }

    #[test]
    fn incomplete_grants_are_permanent_refusals() {
        use ashpd::enumflags2::BitFlags;
        let keyboard: BitFlags<DeviceType> = DeviceType::Keyboard.into();
        assert!(ensure_keyboard_only_grant(keyboard, false, true).is_ok());
        for (devices, streams, clipboard) in [
            (DeviceType::Keyboard | DeviceType::Pointer, false, true),
            (BitFlags::empty(), false, true),
            (keyboard, true, true),
            (keyboard, false, false),
        ] {
            let error = ensure_keyboard_only_grant(devices, streams, clipboard).unwrap_err();
            assert!(is_permanent(&error));
        }
    }

    #[test]
    fn giving_up_keeps_the_cause_in_the_alternate_format() {
        let error = anyhow::Error::from(ashpd::Error::Response(ResponseError::Cancelled))
            .context("restart Koe to retry");
        let shown = format!("{error:#}");
        assert!(shown.contains("restart Koe to retry"));
        assert!(shown.contains("cancelled"));
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
