/// Play an XDG sound event using `canberra-gtk-play`.
///
/// This is a non-blocking fire-and-forget call. If the command fails to
/// spawn (e.g. `canberra-gtk-play` is not installed), the error is silently
/// ignored since sound feedback is a nice-to-have, not critical.
pub fn play_event(event_id: &str) {
    match std::process::Command::new("canberra-gtk-play")
        .arg("--id")
        .arg(event_id)
        .spawn()
    {
        Ok(mut child) => {
            // Reap the child in a detached thread to prevent zombie accumulation.
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
        Err(_) => {} // canberra-gtk-play not installed; silently ignore
    }
}

/// Play a sound event if sound feedback is enabled.
pub fn play_if_enabled(event_id: &str, enabled: bool) {
    if enabled {
        play_event(event_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_play_event_does_not_panic() {
        // Even if canberra-gtk-play is not installed, this should not panic.
        play_event("bell");
    }

    #[test]
    fn test_play_if_enabled_respects_flag() {
        // When disabled, nothing should happen (and no panic).
        play_if_enabled("bell", false);
        // When enabled, it should attempt to play (and not panic even if binary missing).
        play_if_enabled("bell", true);
    }
}
