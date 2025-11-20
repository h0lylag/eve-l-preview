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
        crate::ipc::write_message(&mut self.stream, resp)
    }

    /// Receive request from GUI (blocking)
    fn recv_request(&mut self) -> Result<PreviewRequest> {
        crate::ipc::read_message(&mut self.stream)
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
                Ok(PreviewRequest::SetProfile { profile, global }) => {
                    info!(profile = %profile.name, "Received profile configuration via IPC");
                    let mut state = state.lock().unwrap();
                    state.profile = profile;
                    state.global = global;
                    // TODO: Trigger thumbnail re-render with new settings
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
