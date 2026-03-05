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

/// Capture the current active window ID via xdotool.
/// Call this before showing overlays (e.g. indicator window) to preserve
/// the real target window for later paste operations.
pub fn capture_active_window() -> Option<String> {
    let output = std::process::Command::new("xdotool")
        .arg("getactivewindow")
        .output()
        .ok()?;

    let id = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if id.is_empty() {
        None
    } else {
        tracing::debug!("Captured active window: {}", id);
        Some(id)
    }
}

/// Check if the given window ID belongs to a terminal emulator.
fn is_window_terminal(window_id: &str) -> bool {
    let wm_class = std::process::Command::new("xprop")
        .args(["-id", window_id, "WM_CLASS"])
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
/// Uses a pre-captured window ID to determine terminal vs GUI paste key.
/// If `target_window` is None, falls back to detecting the current active window.
pub fn paste_text(text: &str, target_window: Option<&str>) -> Result<()> {
    if text.is_empty() {
        return Ok(());
    }

    // Focus the target window if we have one, to ensure paste goes to the right place
    if let Some(wid) = target_window {
        let _ = std::process::Command::new("xdotool")
            .args(["windowfocus", "--sync", wid])
            .status();
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
    let is_terminal = match target_window {
        Some(wid) => is_window_terminal(wid),
        None => {
            // Fallback: detect current active window
            capture_active_window()
                .map(|wid| is_window_terminal(&wid))
                .unwrap_or(false)
        }
    };
    let paste_key = if is_terminal {
        "ctrl+shift+v"
    } else {
        "ctrl+v"
    };

    tracing::info!(
        "Pasting {} chars via {} (terminal={}, window={:?})",
        text.len(),
        paste_key,
        is_terminal,
        target_window
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
