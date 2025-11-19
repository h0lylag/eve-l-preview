//! IPC message handler for preview process

use anyhow::{Context, Result};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use tracing::{debug, error, info, warn};

use crate::config::daemon_state::PersistentState;
use crate::ipc::{PreviewRequest, PreviewResponse, PreviewServer};

/// Connection handle for a single GUI client
pub struct ClientConnection {
    stream: std::os::unix::net::UnixStream,
}

impl ClientConnection {
    /// Send response to GUI (can be either a reply or an unsolicited event)
    pub fn send_response(&mut self, resp: &PreviewResponse) -> Result<()> {
        use std::io::Write;
        let json = serde_json::to_vec(resp).context("Failed to serialize response")?;
        let len = json.len() as u32;
        self.stream.write_all(&len.to_le_bytes())?;
        self.stream.write_all(&json)?;
        self.stream.flush()?;
        Ok(())
    }

    /// Receive request from GUI (blocking)
    fn recv_request(&mut self) -> Result<PreviewRequest> {
        use std::io::Read;
        let mut len_buf = [0u8; 4];
        self.stream.read_exact(&mut len_buf)?;
        let len = u32::from_le_bytes(len_buf) as usize;
        
        const MAX_MESSAGE_SIZE: usize = 10 * 1024 * 1024;
        if len > MAX_MESSAGE_SIZE {
            anyhow::bail!("Message too large: {} bytes", len);
        }
        
        let mut json_buf = vec![0u8; len];
        self.stream.read_exact(&mut json_buf)?;
        Ok(serde_json::from_slice(&json_buf)?)
    }
}

/// Spawn IPC listener thread to handle GUI requests
pub fn spawn_ipc_listener(
    server: PreviewServer,
    state: Arc<Mutex<PersistentState>>,
    shutdown_tx: mpsc::Sender<()>,
    client_tx: mpsc::Sender<Arc<Mutex<ClientConnection>>>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        if let Err(e) = run_ipc_loop(&server, &state, &shutdown_tx, &client_tx) {
            error!(error = ?e, "IPC listener thread crashed");
        }
    })
}

fn run_ipc_loop(
    server: &PreviewServer,
    state: &Arc<Mutex<PersistentState>>,
    shutdown_tx: &mpsc::Sender<()>,
    client_tx: &mpsc::Sender<Arc<Mutex<ClientConnection>>>,
) -> Result<()> {
    info!(socket = ?server.path(), "IPC listener started");

    loop {
        // Accept connection (blocks until GUI connects)
        let client = server
            .accept()
            .context("Failed to accept IPC connection")?;
        
        // Wrap in our connection type and share with main loop
        let client = Arc::new(Mutex::new(ClientConnection {
            stream: client.stream,
        }));
        
        // Send client to main loop so it can send unsolicited events
        if client_tx.send(client.clone()).is_err() {
            warn!("Failed to send client to main loop (shutting down?)");
            break Ok(());
        }

        info!("GUI connected to preview process");

        // Handle messages from this client
        loop {
            let recv_result = {
                let mut client_lock = client.lock().unwrap();
                client_lock.recv_request()
            };
            
            match recv_result {
                Ok(PreviewRequest::UpdateProfile(profile)) => {
                    debug!(profile = %profile.name, "Received profile update");
                    let mut state = state.lock().unwrap();
                    state.profile = profile;
                    // TODO: Trigger thumbnail re-render with new settings
                    client.lock().unwrap().send_response(&PreviewResponse::Ready)?;
                }

                Ok(PreviewRequest::UpdateGlobalSettings(global)) => {
                    debug!("Received global settings update");
                    let mut state = state.lock().unwrap();
                    state.global = global;
                    client.lock().unwrap().send_response(&PreviewResponse::Ready)?;
                }

                Ok(PreviewRequest::GetPositions) => {
                    debug!("GUI requested character positions");
                    let state = state.lock().unwrap();
                    let positions = state.character_positions.clone();
                    client.lock().unwrap().send_response(&PreviewResponse::Positions(positions))?;
                }

                Ok(PreviewRequest::Ping) => {
                    client.lock().unwrap().send_response(&PreviewResponse::Pong)?;
                }

                Ok(PreviewRequest::Shutdown) => {
                    info!("Received shutdown request via IPC");
                    shutdown_tx.send(()).ok();
                    break;  // Break inner loop, outer loop continues (but shutdown will stop it)
                }

                Err(e) => {
                    warn!(error = ?e, "IPC connection closed or error");
                    break;  // Break inner loop, continue accepting new connections
                }
            }
        }

        info!("GUI disconnected from preview process");
    }
}
