use anyhow::Result;
use std::collections::HashMap;
use tracing::info;
use x11rb::protocol::xproto::Window;

use crate::config::Config;

/// Runtime state for position tracking
/// Window positions are session-only (not persisted to disk)
pub struct SavedState {
    /// Window ID → (x, y) position (session-only, not persisted)
    /// Used for logged-out windows that show "EVE" without character name
    /// Window IDs are ephemeral and don't survive X11 server restarts
    pub window_positions: HashMap<Window, (i16, i16)>,
    
    /// TODO: Move to Config - behavior for new characters on existing windows
    /// - false: New character spawns centered (current behavior)
    /// - true: New character inherits window's last position
    pub inherit_window_position: bool,
}

impl Default for SavedState {
    fn default() -> Self {
        Self {
            window_positions: HashMap::new(),
            inherit_window_position: false,
        }
    }
}

impl SavedState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Get initial position for a thumbnail
    /// Priority: character position (from config) > window position (if enabled) > None (use center)
    /// Window position only used for logged-out windows or if inherit_window_position is enabled
    pub fn get_position(&self, character_name: &str, window: Window, config: &Config) -> Option<(i16, i16)> {
        // If character has a name (not just "EVE"), check character position from config
        if !character_name.is_empty() {
            if let Some(&pos) = config.character_positions.get(character_name) {
                info!("Using saved position for character '{}': {:?}", character_name, pos);
                return Some(pos);
            }
            
            // TODO: When config option is added, check inherit_window_position here
            // For now, new character always spawns centered
            if self.inherit_window_position {
                if let Some(&pos) = self.window_positions.get(&window) {
                    info!("Inheriting window position for new character '{}': {:?}", character_name, pos);
                    return Some(pos);
                }
            }
            
            // New character with no saved position → return None (will center)
            return None;
        }
        
        // Logged-out window ("EVE" title) → use window position from this session
        if let Some(&pos) = self.window_positions.get(&window) {
            info!("Using session position for logged-out window {}: {:?}", window, pos);
            Some(pos)
        } else {
            None
        }
    }

    /// Update position after drag
    pub fn update_position(&mut self, character_name: &str, window: Window, x: i16, y: i16, config: &mut Config) -> Result<()> {
        // Always save to window_positions (session memory)
        self.window_positions.insert(window, (x, y));
        
        // Only save to character_positions if we have a character name
        if !character_name.is_empty() {
            info!("Saving position for character '{}': ({}, {})", character_name, x, y);
            config.character_positions.insert(character_name.to_string(), (x, y));
            // Save config to disk
            config.save()?;
        } else {
            info!("Saving session position for window {} (logged out): ({}, {})", window, x, y);
        }
        
        Ok(())
    }

    /// Handle character name change (login/logout)
    /// Returns new position if the new character has a saved position
    pub fn handle_character_change(
        &mut self,
        window: Window,
        old_name: &str,
        new_name: &str,
        current_position: (i16, i16),
        config: &mut Config,
    ) -> Result<Option<(i16, i16)>> {
        info!("Character change on window {}: '{}' → '{}'", window, old_name, new_name);
        
        // Save old position to config
        if !old_name.is_empty() {
            config.character_positions.insert(old_name.to_string(), current_position);
        }
        self.window_positions.insert(window, current_position);
        
        // Save config to disk
        config.save()?;
        
        // Return new position if we have one saved for the new character
        if !new_name.is_empty() {
            if let Some(&new_pos) = config.character_positions.get(new_name) {
                info!("Moving to saved position for '{}': {:?}", new_name, new_pos);
                return Ok(Some(new_pos));
            }
        }
        
        // Character logged out OR new character with no saved position → keep current position
        Ok(None)
    }
}
