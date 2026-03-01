pub mod settings_window;
pub mod tray;

use anyhow::Result;
use gtk4::prelude::*;

/// Launch the GTK4 settings window.
pub fn run_settings() -> Result<()> {
    let app = libadwaita::Application::builder()
        .application_id("com.github.koe.settings")
        .build();

    app.connect_activate(|app| {
        settings_window::build(app);
    });

    app.run_with_args::<&str>(&[]);
    Ok(())
}
