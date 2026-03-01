use anyhow::{Context, Result};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::*;
use x11rb::rust_connection::RustConnection;

/// Information about the currently active window.
#[derive(Debug, Clone, Default)]
pub struct WindowContext {
    pub window_title: String,
    pub app_name: String,
    pub window_class: String,
}

/// Capture context from the active X11 window.
pub fn get_active_window_context() -> Result<WindowContext> {
    let (conn, screen_num) = RustConnection::connect(None).context("connecting to X11")?;
    let screen = &conn.setup().roots[screen_num];
    let root = screen.root;

    // Get the _NET_ACTIVE_WINDOW atom
    let active_window_atom = conn
        .intern_atom(false, b"_NET_ACTIVE_WINDOW")
        .context("interning _NET_ACTIVE_WINDOW")?
        .reply()
        .context("getting _NET_ACTIVE_WINDOW reply")?
        .atom;

    // Get the active window ID
    let active_window_reply = conn
        .get_property(false, root, active_window_atom, AtomEnum::WINDOW, 0, 1)
        .context("getting active window property")?
        .reply()
        .context("getting active window reply")?;

    let window_id = if active_window_reply.value_len > 0 && active_window_reply.format == 32 {
        match active_window_reply.value32() {
            Some(mut iter) => iter.next().unwrap_or(0),
            None => 0,
        }
    } else {
        tracing::warn!("Could not determine active window");
        return Ok(WindowContext::default());
    };

    if window_id == 0 {
        return Ok(WindowContext::default());
    }

    // Get window title (_NET_WM_NAME or WM_NAME)
    let window_title = get_window_name(&conn, window_id).unwrap_or_default();

    // Get WM_CLASS
    let (app_name, window_class) = get_wm_class(&conn, window_id).unwrap_or_default();

    let ctx = WindowContext {
        window_title,
        app_name,
        window_class,
    };

    tracing::debug!("Window context: {:?}", ctx);
    Ok(ctx)
}

fn get_window_name(conn: &RustConnection, window: u32) -> Result<String> {
    // Try _NET_WM_NAME first (UTF-8)
    let net_wm_name_atom = conn
        .intern_atom(false, b"_NET_WM_NAME")
        .context("interning _NET_WM_NAME")?
        .reply()?
        .atom;

    let utf8_string_atom = conn
        .intern_atom(false, b"UTF8_STRING")
        .context("interning UTF8_STRING")?
        .reply()?
        .atom;

    let reply = conn
        .get_property(false, window, net_wm_name_atom, utf8_string_atom, 0, 1024)
        .context("getting _NET_WM_NAME")?
        .reply()?;

    if reply.value_len > 0 {
        return Ok(String::from_utf8_lossy(&reply.value).to_string());
    }

    // Fallback to WM_NAME
    let reply = conn
        .get_property(
            false,
            window,
            AtomEnum::WM_NAME,
            AtomEnum::STRING,
            0,
            1024,
        )
        .context("getting WM_NAME")?
        .reply()?;

    Ok(String::from_utf8_lossy(&reply.value).to_string())
}

fn get_wm_class(conn: &RustConnection, window: u32) -> Result<(String, String)> {
    let reply = conn
        .get_property(
            false,
            window,
            AtomEnum::WM_CLASS,
            AtomEnum::STRING,
            0,
            1024,
        )
        .context("getting WM_CLASS")?
        .reply()?;

    if reply.value_len == 0 {
        return Ok((String::new(), String::new()));
    }

    // WM_CLASS is two null-terminated strings: instance\0class\0
    let value = String::from_utf8_lossy(&reply.value).to_string();
    let parts: Vec<&str> = value.split('\0').collect();

    let instance = parts.first().unwrap_or(&"").to_string();
    let class = parts.get(1).unwrap_or(&"").to_string();

    Ok((instance, class))
}
