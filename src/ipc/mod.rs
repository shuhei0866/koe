pub mod client;
pub mod server;

use serde::{Deserialize, Serialize};

/// Requests sent from the settings UI (or other clients) to the daemon.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum IpcRequest {
    GetStatus,
    ReloadConfig,
    Shutdown,
}

/// Responses sent from the daemon back to clients.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum IpcResponse {
    Status { state: String, is_recording: bool },
    Ok,
    Error { message: String },
}

/// Return the IPC socket path.
pub fn socket_path() -> std::path::PathBuf {
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
    std::path::PathBuf::from(runtime_dir).join("koe.sock")
}
