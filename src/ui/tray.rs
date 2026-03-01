use ksni::menu::StandardItem;
use ksni::{Tray, TrayMethods};

struct KoeTray;

impl Tray for KoeTray {
    fn id(&self) -> String {
        "koe".to_string()
    }

    fn icon_name(&self) -> String {
        "audio-input-microphone".to_string()
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
                activate: Box::new(|_| {
                    std::process::exit(0);
                }),
                ..Default::default()
            }
            .into(),
        ]
    }
}

/// Start the system tray icon (async, runs in a tokio task).
pub fn start_tray() {
    tokio::spawn(async {
        match KoeTray.spawn().await {
            Ok(_handle) => {
                tracing::info!("System tray started");
            }
            Err(e) => {
                tracing::error!("Failed to start system tray: {}", e);
            }
        }
    });
}
