use anyhow::{Context, Result};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;

use super::{IpcRequest, IpcResponse};

/// Send a request to the daemon via IPC and return the response.
pub fn send_request(request: &IpcRequest) -> Result<IpcResponse> {
    let sock_path = super::socket_path();
    let mut stream = UnixStream::connect(&sock_path)
        .with_context(|| format!("connecting to daemon at {}", sock_path.display()))?;

    let mut req_json = serde_json::to_string(request)?;
    req_json.push('\n');
    stream
        .write_all(req_json.as_bytes())
        .context("sending IPC request")?;

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).context("reading IPC response")?;

    let response: IpcResponse =
        serde_json::from_str(line.trim()).context("parsing IPC response")?;
    Ok(response)
}

/// Send a ReloadConfig request to the daemon.
pub fn reload_config() -> Result<()> {
    match send_request(&IpcRequest::ReloadConfig)? {
        IpcResponse::Ok => Ok(()),
        IpcResponse::Error { message } => anyhow::bail!("daemon error: {}", message),
        _ => Ok(()),
    }
}
