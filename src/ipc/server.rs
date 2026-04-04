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
                anyhow::bail!(
                    "koe daemon is already running (socket {} is active)",
                    sock_path.display()
                );
            }
            Err(e) if e.kind() == std::io::ErrorKind::ConnectionRefused => {
                // Socket is stale (no listener) — remove and continue
                std::fs::remove_file(&sock_path)
                    .with_context(|| format!("removing stale socket {}", sock_path.display()))?;
            }
            Err(e) => {
                // Unexpected error (permission denied, etc.) — do not remove, propagate
                anyhow::bail!(
                    "cannot check existing socket {}: {}",
                    sock_path.display(),
                    e
                );
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
                            // Sleep briefly to avoid busy-looping on persistent errors
                            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                        }
                    }
                }
            }
        }
    });

    Ok(rx)
}

/// Default IPC message size limit (64 KiB) used when no config is available.
const DEFAULT_MAX_IPC_MESSAGE_BYTES: usize = 64 * 1024;

async fn handle_connection(
    stream: tokio::net::UnixStream,
    tx: mpsc::Sender<IpcRequest>,
) -> Result<()> {
    handle_connection_with_limit(stream, tx, DEFAULT_MAX_IPC_MESSAGE_BYTES).await
}

/// Read a single newline-terminated line into `buf`, reading at most
/// `max_bytes` before giving up.  Returns `Ok(true)` when a complete
/// line was read, `Ok(false)` on EOF, and sets `*oversize = true` when
/// the limit was hit before a newline appeared.
async fn read_line_limited(
    reader: &mut BufReader<tokio::net::unix::OwnedReadHalf>,
    buf: &mut Vec<u8>,
    max_bytes: usize,
    oversize: &mut bool,
) -> std::io::Result<bool> {
    buf.clear();
    *oversize = false;
    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            return Ok(!buf.is_empty()); // EOF
        }
        if let Some(pos) = available.iter().position(|&b| b == b'\n') {
            let to_consume = pos + 1; // include the newline
            if buf.len() + to_consume > max_bytes {
                // Consume everything up to and including the newline so the
                // stream is positioned at the start of the next message.
                reader.consume(to_consume);
                *oversize = true;
                return Ok(true);
            }
            buf.extend_from_slice(&available[..to_consume]);
            reader.consume(to_consume);
            return Ok(true);
        }
        // No newline yet — consume everything available.
        let len = available.len();
        if buf.len() + len > max_bytes {
            reader.consume(len);
            *oversize = true;
            // Drain remaining bytes until newline or EOF.
            loop {
                let rest = reader.fill_buf().await?;
                if rest.is_empty() {
                    return Ok(true);
                }
                if let Some(pos) = rest.iter().position(|&b| b == b'\n') {
                    reader.consume(pos + 1);
                    return Ok(true);
                }
                let rlen = rest.len();
                reader.consume(rlen);
            }
        }
        buf.extend_from_slice(available);
        reader.consume(len);
    }
}

async fn handle_connection_with_limit(
    stream: tokio::net::UnixStream,
    tx: mpsc::Sender<IpcRequest>,
    max_message_bytes: usize,
) -> Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let mut buf = Vec::new();
    let mut oversize = false;

    while read_line_limited(&mut reader, &mut buf, max_message_bytes, &mut oversize).await? {
        if oversize {
            let response = IpcResponse::Error {
                message: format!(
                    "message too large (exceeded limit of {} bytes)",
                    max_message_bytes
                ),
            };
            let mut resp_json = serde_json::to_string(&response)?;
            resp_json.push('\n');
            writer.write_all(resp_json.as_bytes()).await?;
            continue;
        }

        let line = std::str::from_utf8(&buf)
            .map_err(|e| anyhow::anyhow!("invalid UTF-8 in IPC message: {}", e))?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
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
    }

    Ok(())
}
