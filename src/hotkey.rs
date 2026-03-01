use anyhow::Result;
use rdev::{listen, Event, EventType, Key};
use std::sync::mpsc;

use crate::config::HotkeyMode;

/// Events sent from the hotkey listener to the main loop.
#[derive(Debug, Clone)]
pub enum HotkeyEvent {
    RecordStart,
    RecordStop,
}

/// Parse a key name string into an rdev Key.
pub fn parse_key(key_name: &str) -> Result<Key> {
    let key = match key_name {
        "Super_R" => Key::MetaRight,
        "Super_L" => Key::MetaLeft,
        "Control_R" => Key::ControlRight,
        "Control_L" => Key::ControlLeft,
        "Alt_R" => Key::Alt,
        "Alt_L" => Key::Alt,
        "Shift_R" => Key::ShiftRight,
        "Shift_L" => Key::ShiftLeft,
        "F1" => Key::F1,
        "F2" => Key::F2,
        "F3" => Key::F3,
        "F4" => Key::F4,
        "F5" => Key::F5,
        "F6" => Key::F6,
        "F7" => Key::F7,
        "F8" => Key::F8,
        "F9" => Key::F9,
        "F10" => Key::F10,
        "F11" => Key::F11,
        "F12" => Key::F12,
        "Pause" => Key::Pause,
        "ScrollLock" => Key::ScrollLock,
        "PrintScreen" => Key::PrintScreen,
        _ => anyhow::bail!("Unknown key name: {}", key_name),
    };
    Ok(key)
}

/// Start the hotkey listener in a separate thread.
/// Returns a channel receiver for hotkey events.
pub fn start_hotkey_listener(
    mode: HotkeyMode,
    key_name: &str,
) -> Result<mpsc::Receiver<HotkeyEvent>> {
    let (tx, rx) = mpsc::channel();
    let target_key = parse_key(key_name)?;

    tracing::info!("Hotkey listener starting: key={}, mode={:?}", key_name, mode);

    std::thread::spawn(move || {
        let mut is_recording = false;

        let callback = move |event: Event| {
            match event.event_type {
                EventType::KeyPress(key) if key == target_key => {
                    match mode {
                        HotkeyMode::PushToTalk => {
                            if !is_recording {
                                is_recording = true;
                                let _ = tx.send(HotkeyEvent::RecordStart);
                            }
                        }
                        HotkeyMode::Toggle => {
                            if is_recording {
                                is_recording = false;
                                let _ = tx.send(HotkeyEvent::RecordStop);
                            } else {
                                is_recording = true;
                                let _ = tx.send(HotkeyEvent::RecordStart);
                            }
                        }
                    }
                }
                EventType::KeyRelease(key) if key == target_key => {
                    if mode == HotkeyMode::PushToTalk && is_recording {
                        is_recording = false;
                        let _ = tx.send(HotkeyEvent::RecordStop);
                    }
                }
                _ => {}
            }
        };

        if let Err(e) = listen(callback) {
            tracing::error!("Hotkey listener error: {:?}", e);
        }
    });

    Ok(rx)
}
