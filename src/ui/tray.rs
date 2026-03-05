use ksni::menu::StandardItem;
use ksni::{Handle, Tray, TrayMethods};

pub struct KoeTray {
    pub current_icon: String,
    shutdown_tx: tokio::sync::watch::Sender<bool>,
}

impl Tray for KoeTray {
    fn id(&self) -> String {
        "koe".to_string()
    }

    fn icon_name(&self) -> String {
        self.current_icon.clone()
    }

    fn title(&self) -> String {
        "koe".to_string()
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            title: "koe - Voice Input".to_string(),
            description: String::new(),
            icon_name: String::new(),
            icon_pixmap: Vec::new(),
        }
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        vec![
            StandardItem {
                label: "Settings...".to_string(),
                activate: Box::new(|_| {
                    if let Ok(exe) = std::env::current_exe() {
                        let _ = std::process::Command::new(exe).arg("settings").spawn();
                    }
                }),
                ..Default::default()
            }
            .into(),
            ksni::MenuItem::Separator,
            StandardItem {
                label: "Quit".to_string(),
                activate: Box::new(|this: &mut Self| {
                    let _ = this.shutdown_tx.send(true);
                }),
                ..Default::default()
            }
            .into(),
        ]
    }
}

/// Map AppState string to system icon name.
pub fn icon_for_state(state: &str) -> &'static str {
    match state {
        "Idle" => "audio-input-microphone",
        "Recording" => "media-record",
        "Processing" => "emblem-synchronizing",
        "Typing" => "input-keyboard",
        _ => "audio-input-microphone",
    }
}

/// Update the tray icon to reflect the current state.
pub async fn update_tray_icon(handle: &Handle<KoeTray>, state: &str) {
    let icon = icon_for_state(state).to_string();
    handle
        .update(|tray| {
            tray.current_icon = icon;
        })
        .await;
}

/// Start the system tray icon and return the handle for dynamic updates.
pub async fn start_tray(shutdown_tx: tokio::sync::watch::Sender<bool>) -> Result<Handle<KoeTray>, ksni::Error> {
    let tray = KoeTray {
        current_icon: "audio-input-microphone".to_string(),
        shutdown_tx,
    };
    let handle = tray.spawn().await?;
    tracing::info!("System tray started");
    Ok(handle)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_shutdown_tx() -> tokio::sync::watch::Sender<bool> {
        let (tx, _rx) = tokio::sync::watch::channel(false);
        tx
    }

    #[test]
    fn test_icon_for_state() {
        assert_eq!(icon_for_state("Idle"), "audio-input-microphone");
        assert_eq!(icon_for_state("Recording"), "media-record");
        assert_eq!(icon_for_state("Processing"), "emblem-synchronizing");
        assert_eq!(icon_for_state("Typing"), "input-keyboard");
        assert_eq!(icon_for_state("unknown"), "audio-input-microphone");
    }

    #[test]
    fn test_icon_for_state_empty_string() {
        assert_eq!(icon_for_state(""), "audio-input-microphone");
    }

    #[test]
    fn test_koe_tray_initial_icon() {
        let tray = KoeTray {
            current_icon: "audio-input-microphone".to_string(),
            shutdown_tx: make_shutdown_tx(),
        };
        assert_eq!(tray.icon_name(), "audio-input-microphone");
    }

    #[test]
    fn test_koe_tray_custom_icon() {
        let tray = KoeTray {
            current_icon: "media-record".to_string(),
            shutdown_tx: make_shutdown_tx(),
        };
        assert_eq!(tray.icon_name(), "media-record");
    }

    #[test]
    fn test_koe_tray_id() {
        let tray = KoeTray {
            current_icon: String::new(),
            shutdown_tx: make_shutdown_tx(),
        };
        assert_eq!(tray.id(), "koe");
    }

    #[test]
    fn test_koe_tray_title() {
        let tray = KoeTray {
            current_icon: String::new(),
            shutdown_tx: make_shutdown_tx(),
        };
        assert_eq!(tray.title(), "koe");
    }

    #[test]
    fn test_koe_tray_tooltip() {
        let tray = KoeTray {
            current_icon: String::new(),
            shutdown_tx: make_shutdown_tx(),
        };
        let tip = tray.tool_tip();
        assert_eq!(tip.title, "koe - Voice Input");
    }
}
