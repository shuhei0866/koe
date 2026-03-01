use anyhow::{Context, Result};
use enigo::{Enigo, Keyboard, Settings};

/// Type text into the active window using enigo.
pub fn type_text(text: &str) -> Result<()> {
    if text.is_empty() {
        return Ok(());
    }

    let mut enigo =
        Enigo::new(&Settings::default()).map_err(|e| anyhow::anyhow!("enigo init: {}", e))?;

    tracing::info!("Typing {} chars into active window", text.len());

    // Type the text character by character for reliability
    enigo
        .text(text)
        .map_err(|e| anyhow::anyhow!("enigo type: {}", e))?;

    Ok(())
}

/// Type text using clipboard paste for better Unicode support.
pub fn paste_text(text: &str) -> Result<()> {
    use arboard::Clipboard;
    use enigo::{Direction, Key};

    if text.is_empty() {
        return Ok(());
    }

    // Save current clipboard content
    let mut clipboard = Clipboard::new().context("opening clipboard")?;
    let old_content = clipboard.get_text().ok();

    // Set new content
    clipboard
        .set_text(text)
        .context("setting clipboard text")?;

    // Simulate Ctrl+V
    let mut enigo =
        Enigo::new(&Settings::default()).map_err(|e| anyhow::anyhow!("enigo init: {}", e))?;

    enigo
        .key(Key::Control, Direction::Press)
        .map_err(|e| anyhow::anyhow!("key press: {}", e))?;
    enigo
        .key(Key::Unicode('v'), Direction::Press)
        .map_err(|e| anyhow::anyhow!("key press v: {}", e))?;
    enigo
        .key(Key::Unicode('v'), Direction::Release)
        .map_err(|e| anyhow::anyhow!("key release v: {}", e))?;
    enigo
        .key(Key::Control, Direction::Release)
        .map_err(|e| anyhow::anyhow!("key release: {}", e))?;

    // Small delay then restore clipboard
    std::thread::sleep(std::time::Duration::from_millis(100));
    if let Some(old) = old_content {
        let _ = clipboard.set_text(old);
    }

    tracing::info!("Pasted {} chars via clipboard", text.len());
    Ok(())
}
