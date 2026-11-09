use anyhow::{Context, Result};

use crate::config::WaylandPasteShortcut;

/// Known terminal WM_CLASS / app-id fragments (lowercase for comparison).
/// Matched as substrings, so `org.gnome.ptyxis` and `com.mitchellh.ghostty`
/// are covered by `ptyxis` and `ghostty`.
const TERMINAL_CLASSES: &[&str] = &[
    "gnome-terminal",
    // GNOME Terminal's native Wayland app id; Mutter reports it as WM_CLASS.
    "org.gnome.terminal",
    "ptyxis",
    "ghostty",
    "org.gnome.console",
    "kgx",
    "blackbox",
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

/// Whether a WM_CLASS (or Wayland app id) belongs to a terminal emulator.
pub fn is_terminal_class(class: &str) -> bool {
    let class = class.to_lowercase();
    TERMINAL_CLASSES.iter().any(|t| class.contains(t))
}

/// Pick the concrete paste shortcut for Wayland. An explicit setting always
/// wins; `Auto` uses the focused app reported by the GNOME Shell extension
/// and falls back to Ctrl+V when nothing has been reported.
pub fn resolve_wayland_paste_shortcut(
    setting: WaylandPasteShortcut,
    focused_app: Option<&str>,
) -> WaylandPasteShortcut {
    match setting {
        WaylandPasteShortcut::Auto => match focused_app {
            Some(class) if is_terminal_class(class) => WaylandPasteShortcut::CtrlShiftV,
            _ => WaylandPasteShortcut::CtrlV,
        },
        explicit => explicit,
    }
}

/// Capture the current active window ID via xdotool.
/// Call this before showing overlays (e.g. indicator window) to preserve
/// the real target window for later paste operations.
pub fn capture_active_window() -> Option<String> {
    if crate::portal::is_wayland() {
        return None;
    }
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

    is_terminal_class(&String::from_utf8_lossy(&output.stdout))
}

/// Type text into the active window using xdotool.
pub async fn type_text(text: &str) -> Result<()> {
    if crate::portal::is_wayland() {
        return crate::portal::paste_text(text, Default::default()).await;
    }
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
/// On Wayland, `focused_app` is the WM_CLASS reported by the GNOME Shell
/// extension (None when the extension is not running).
pub async fn paste_text(
    text: &str,
    target_window: Option<&str>,
    focused_app: Option<&str>,
    options: &crate::config::InputConfig,
) -> Result<()> {
    if crate::portal::is_wayland() {
        let shortcut = resolve_wayland_paste_shortcut(options.wayland_paste_shortcut, focused_app);
        tracing::info!(
            "Pasting {} chars via {:?} (setting={:?}, focused_app={:?})",
            text.len(),
            shortcut,
            options.wayland_paste_shortcut,
            focused_app
        );
        return crate::portal::paste_text(text, shortcut).await;
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_classes_cover_wayland_app_ids() {
        for class in [
            "org.gnome.Ptyxis",
            "com.mitchellh.ghostty",
            "gnome-terminal-server",
            "org.gnome.Terminal",
            "org.gnome.Console",
            "kitty",
            "Alacritty",
            "org.wezfurlong.wezterm",
        ] {
            assert!(is_terminal_class(class), "{class} should be a terminal");
        }
        for class in [
            "firefox",
            "google-chrome",
            "Slack",
            "org.gnome.TextEditor",
            "",
        ] {
            assert!(
                !is_terminal_class(class),
                "{class} should not be a terminal"
            );
        }
    }

    #[test]
    fn auto_follows_the_focused_app_and_falls_back_to_ctrl_v() {
        use WaylandPasteShortcut::*;
        let resolve = resolve_wayland_paste_shortcut;
        assert_eq!(resolve(Auto, Some("com.mitchellh.ghostty")), CtrlShiftV);
        assert_eq!(resolve(Auto, Some("firefox")), CtrlV);
        assert_eq!(resolve(Auto, None), CtrlV);
    }

    #[test]
    fn explicit_setting_wins_over_the_focused_app() {
        use WaylandPasteShortcut::*;
        let resolve = resolve_wayland_paste_shortcut;
        assert_eq!(resolve(CtrlV, Some("org.gnome.Ptyxis")), CtrlV);
        assert_eq!(resolve(CtrlShiftV, Some("firefox")), CtrlShiftV);
        assert_eq!(resolve(CtrlShiftV, None), CtrlShiftV);
    }
}
