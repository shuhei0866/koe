use anyhow::{Context, Result};
use std::sync::mpsc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;

use super::{IpcRequest, IpcResponse};

/// Remove the IPC socket file if it exists.
pub fn cleanup_socket() {
    let sock_path = super::socket_path();
    if sock_path.exists() {
        if let Err(e) = std::fs::remove_file(&sock_path) {
            tracing::warn!("Failed to remove socket {}: {}", sock_path.display(), e);
        } else {
            tracing::info!("Removed socket {}", sock_path.display());
        }
    }
}

/// Start the IPC server on a Unix Domain Socket.
/// Returns a channel receiver for incoming requests.
pub async fn start(
    mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
) -> Result<mpsc::Receiver<IpcRequest>> {
    let sock_path = super::socket_path();

    // Check for already-running instance via the socket
    if sock_path.exists() {
        match std::os::unix::net::UnixStream::connect(&sock_path) {
            Ok(_) => {
                anyhow::bail!("koe daemon is already running (socket {} is active)", sock_path.display());
            }
            Err(e) if e.kind() == std::io::ErrorKind::ConnectionRefused => {
                // Socket is stale (no listener) — remove and continue
                std::fs::remove_file(&sock_path)
                    .with_context(|| format!("removing stale socket {}", sock_path.display()))?;
            }
            Err(e) => {
                // Unexpected error (permission denied, etc.) — do not remove, propagate
                anyhow::bail!("cannot check existing socket {}: {}", sock_path.display(), e);
            }
        }
    }

    let listener = UnixListener::bind(&sock_path)
        .with_context(|| format!("binding {}", sock_path.display()))?;

    tracing::info!("IPC server listening on {}", sock_path.display());

    let (tx, rx) = mpsc::channel();

    tokio::spawn(async move {
        loop {
            tokio::select! {
                result = shutdown_rx.changed() => {
                    let explicit = result.is_ok() && *shutdown_rx.borrow();
                    tracing::info!("IPC server shutting down (explicit={})", explicit);
                    break;
                }
                result = listener.accept() => {
                    match result {
                        Ok((stream, _)) => {
                            let tx = tx.clone();
                            tokio::spawn(async move {
                                if let Err(e) = handle_connection(stream, tx).await {
                                    tracing::error!("IPC connection error: {}", e);
                                }
                            });
                        }
                        Err(e) => {
                            tracing::error!("IPC accept error: {}", e);
                            break;
                        }
                    }
                }
            }
        }
    });

    Ok(rx)
}

async fn handle_connection(
    stream: tokio::net::UnixStream,
    tx: mpsc::Sender<IpcRequest>,
) -> Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let mut line = String::new();

    while reader.read_line(&mut line).await? > 0 {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            line.clear();
            continue;
        }

        match serde_json::from_str::<IpcRequest>(trimmed) {
            Ok(request) => {
                tracing::debug!("IPC request: {:?}", request);
                let response = match &request {
                    IpcRequest::GetStatus => {
                        let _ = tx.send(IpcRequest::GetStatus);
                        IpcResponse::Status {
                            state: "idle".to_string(),
                            is_recording: false,
                        }
                    }
                    IpcRequest::ReloadConfig => {
                        let _ = tx.send(IpcRequest::ReloadConfig);
                        IpcResponse::Ok
                    }
                    IpcRequest::Shutdown => {
                        let _ = tx.send(IpcRequest::Shutdown);
                        IpcResponse::Ok
                    }
                };

                let mut resp_json = serde_json::to_string(&response)?;
                resp_json.push('\n');
                writer.write_all(resp_json.as_bytes()).await?;
            }
            Err(e) => {
                let response = IpcResponse::Error {
                    message: format!("invalid request: {}", e),
                };
                let mut resp_json = serde_json::to_string(&response)?;
                resp_json.push('\n');
                writer.write_all(resp_json.as_bytes()).await?;
            }
        }

        line.clear();
    }

    Ok(())
}
