//! IPC (Inter-Process Communication) via Unix sockets
//!
//! Provides message-based communication between GUI Manager and Preview process.
//! Uses length-prefixed JSON over Unix domain sockets.

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};

mod messages;
pub use messages::{PreviewRequest, PreviewResponse};

/// Maximum message size (10 MB) to prevent DoS via memory exhaustion
const MAX_MESSAGE_SIZE: usize = 10 * 1024 * 1024;

/// Get default socket path (XDG_RUNTIME_DIR with fallback to cache)
pub fn default_socket_path() -> Result<PathBuf> {
    if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
        return Ok(PathBuf::from(runtime_dir).join("eve-l-preview/preview.sock"));
    }
    
    // Fallback to cache dir
    let cache = dirs::cache_dir()
        .context("Failed to determine cache directory (no XDG_RUNTIME_DIR or HOME)")?;
    Ok(cache.join("eve-l-preview/preview.sock"))
}

/// Client connection to Preview process (used by GUI)
pub struct PreviewClient {
    pub(crate) stream: UnixStream,
}

impl PreviewClient {
    /// Connect to Preview process socket
    pub fn connect() -> Result<Self> {
        let path = default_socket_path()?;
        Self::connect_to(&path)
    }
    
    /// Connect to specific socket path
    pub fn connect_to(path: &Path) -> Result<Self> {
        let stream = UnixStream::connect(path)
            .context(format!("Failed to connect to preview at {}", path.display()))?;
        Ok(Self { stream })
    }
    
    /// Send request to Preview process
    pub fn send_request(&mut self, req: &PreviewRequest) -> Result<()> {
        write_message(&mut self.stream, req)
    }
    
    /// Receive response from Preview process (blocking)
    pub fn recv_response(&mut self) -> Result<PreviewResponse> {
        read_message(&mut self.stream)
    }
    
    /// Send request and wait for response (convenience method)
    pub fn request(&mut self, req: PreviewRequest) -> Result<PreviewResponse> {
        self.send_request(&req)?;
        self.recv_response()
    }
}

/// Server listener for Preview process
pub struct PreviewServer {
    listener: UnixListener,
    socket_path: PathBuf,
}

impl PreviewServer {
    /// Create server and bind to default socket path
    pub fn bind() -> Result<Self> {
        let socket_path = default_socket_path()?;
        Self::bind_to(socket_path)
    }
    
    /// Create server and bind to specific socket path
    pub fn bind_to(socket_path: PathBuf) -> Result<Self> {
        // Create directory if needed
        if let Some(parent) = socket_path.parent() {
            std::fs::create_dir_all(parent)
                .context(format!("Failed to create socket directory: {}", parent.display()))?;
        }
        
        // Remove stale socket if exists
        if socket_path.exists() {
            std::fs::remove_file(&socket_path)
                .context(format!("Failed to remove stale socket: {}", socket_path.display()))?;
        }
        
        let listener = UnixListener::bind(&socket_path)
            .context(format!("Failed to bind socket at {}", socket_path.display()))?;
        
        // Set permissions to 0700 (owner only)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&socket_path, std::fs::Permissions::from_mode(0o700))
                .context("Failed to set socket permissions")?;
        }
        
        Ok(Self {
            listener,
            socket_path,
        })
    }
    
    /// Accept incoming connection (blocking)
    pub fn accept(&self) -> Result<PreviewClient> {
        let (stream, _addr) = self.listener.accept()
            .context("Failed to accept IPC connection")?;
        Ok(PreviewClient { stream })
    }
    
    /// Get socket path
    pub fn path(&self) -> &Path {
        &self.socket_path
    }
}

impl Drop for PreviewServer {
    fn drop(&mut self) {
        // Clean up socket file
        let _ = std::fs::remove_file(&self.socket_path);
    }
}

/// Write length-prefixed message to stream
fn write_message<T: Serialize>(stream: &mut UnixStream, msg: &T) -> Result<()> {
    let json = serde_json::to_vec(msg).context("Failed to serialize message to JSON")?;
    
    // Write length prefix (u32 little-endian)
    let len = json.len() as u32;
    stream
        .write_all(&len.to_le_bytes())
        .context("Failed to write message length")?;
    
    // Write JSON payload
    stream
        .write_all(&json)
        .context("Failed to write message payload")?;
    
    stream.flush().context("Failed to flush stream")?;
    
    Ok(())
}

/// Read length-prefixed message from stream
fn read_message<T: for<'de> Deserialize<'de>>(stream: &mut UnixStream) -> Result<T> {
    // Read length prefix
    let mut len_buf = [0u8; 4];
    stream
        .read_exact(&mut len_buf)
        .context("Failed to read message length")?;
    let len = u32::from_le_bytes(len_buf) as usize;
    
    // Sanity check (prevent DoS via huge allocation)
    if len > MAX_MESSAGE_SIZE {
        return Err(anyhow!("Message too large: {} bytes (max: {})", len, MAX_MESSAGE_SIZE));
    }
    
    // Read JSON payload
    let mut json_buf = vec![0u8; len];
    stream
        .read_exact(&mut json_buf)
        .context("Failed to read message payload")?;
    
    // Deserialize
    serde_json::from_slice(&json_buf).context("Failed to deserialize message from JSON")
}
