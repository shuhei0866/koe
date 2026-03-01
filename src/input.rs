use anyhow::{Context, Result};

/// Known terminal WM_CLASS values (lowercase for comparison).
const TERMINAL_CLASSES: &[&str] = &[
    "gnome-terminal",
    "kitty",
    "alacritty",
    "wezterm",
    "foot",
    "konsole",
    "xterm",
    "urxvt",
    "terminator",
    "tilix",
    "st-256color",
    "sakura",
];

/// Check if the active window is a terminal emulator.
fn is_active_window_terminal() -> bool {
    let window_id = std::process::Command::new("xdotool")
        .arg("getactivewindow")
        .output()
        .ok();

    let Some(output) = window_id else {
        return false;
    };

    let id = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if id.is_empty() {
        return false;
    }

    let wm_class = std::process::Command::new("xprop")
        .args(["-id", &id, "WM_CLASS"])
        .output()
        .ok();

    let Some(output) = wm_class else {
        return false;
    };

    let class_str = String::from_utf8_lossy(&output.stdout).to_lowercase();
    TERMINAL_CLASSES.iter().any(|t| class_str.contains(t))
}

/// Type text into the active window using xdotool.
pub fn type_text(text: &str) -> Result<()> {
    if text.is_empty() {
        return Ok(());
    }

    tracing::info!("Typing {} chars via xdotool", text.len());

    let status = std::process::Command::new("xdotool")
        .args(["type", "--clearmodifiers", "--", text])
        .status()
        .context("running xdotool type")?;

    if !status.success() {
        anyhow::bail!("xdotool type failed with status {}", status);
    }

    Ok(())
}

/// Type text using clipboard paste via xclip + xdotool.
/// Auto-detects terminal windows and uses Ctrl+Shift+V instead of Ctrl+V.
pub fn paste_text(text: &str) -> Result<()> {
    if text.is_empty() {
        return Ok(());
    }

    // Set clipboard via xclip
    let mut child = std::process::Command::new("xclip")
        .args(["-selection", "clipboard"])
        .stdin(std::process::Stdio::piped())
        .spawn()
        .context("running xclip")?;

    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        stdin
            .write_all(text.as_bytes())
            .context("writing to xclip")?;
    }

    let status = child.wait().context("waiting for xclip")?;
    if !status.success() {
        anyhow::bail!("xclip failed with status {}", status);
    }

    // Use Ctrl+Shift+V for terminals, Ctrl+V for GUI apps
    let is_terminal = is_active_window_terminal();
    let paste_key = if is_terminal {
        "ctrl+shift+v"
    } else {
        "ctrl+v"
    };

    tracing::info!(
        "Pasting {} chars via {} (terminal={})",
        text.len(),
        paste_key,
        is_terminal
    );

    let status = std::process::Command::new("xdotool")
        .args(["key", "--clearmodifiers", paste_key])
        .status()
        .context("running xdotool key")?;

    if !status.success() {
        anyhow::bail!("xdotool key {} failed with status {}", paste_key, status);
    }

    Ok(())
}
