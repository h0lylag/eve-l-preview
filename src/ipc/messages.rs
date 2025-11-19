//! IPC message types for GUI ↔ Preview process communication

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::config::profile::{GlobalSettings, Profile};
use crate::types::CharacterSettings;

/// Requests sent from GUI to Preview process
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum PreviewRequest {
    /// Update the active profile (visual settings + cycle group)
    UpdateProfile(Profile),
    
    /// Update global settings (snap threshold, hide behavior, etc.)
    UpdateGlobalSettings(GlobalSettings),
    
    /// Query current character positions
    GetPositions,
    
    /// Health check
    Ping,
    
    /// Request graceful shutdown
    Shutdown,
}

/// Responses sent from Preview process to GUI
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum PreviewResponse {
    /// Return current character positions (response to GetPositions)
    Positions(HashMap<String, CharacterSettings>),
    
    /// Health check response
    Pong,
    
    /// Character position changed (user dragged thumbnail)
    PositionChanged {
        character: String,
        x: i16,
        y: i16,
        width: u16,
        height: u16,
    },
    
    /// New character window detected
    CharacterAdded {
        character: String,
        x: i16,
        y: i16,
        width: u16,
        height: u16,
    },
    
    /// Character window closed/logged out
    CharacterRemoved(String),
    
    /// Acknowledgment that request was processed
    Ready,
    
    /// Error occurred
    Error(String),
}
