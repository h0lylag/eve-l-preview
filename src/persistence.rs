use anyhow::Result;
use std::collections::HashMap;
use tracing::info;
use x11rb::protocol::xproto::Window;

use crate::types::{CharacterSettings, Position};

/// Runtime state for position tracking
/// Window positions are session-only (not persisted to disk)
pub struct SavedState {
    /// Window ID → position (session-only, not persisted)
    /// Used for logged-out windows that show "EVE" without character name
    /// Window IDs are ephemeral and don't survive X11 server restarts
    pub window_positions: HashMap<Window, Position>,
    
    /// TODO: Move to PersistentState - behavior for new characters on existing windows
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
    /// Priority: character position (from persistent state) > window position (if enabled) > None (use center)
    /// Window position only used for logged-out windows or if inherit_window_position is enabled
    pub fn get_position(
        &self,
        character_name: &str,
        window: Window,
        character_positions: &HashMap<String, CharacterSettings>,
    ) -> Option<Position> {
        // If character has a name (not just "EVE"), check character position from config
        if !character_name.is_empty() {
            if let Some(settings) = character_positions.get(character_name) {
                info!(character = %character_name, x = settings.x, y = settings.y, "Using saved position for character");
                return Some(settings.position());
            }
            
            // TODO: When config option is added, check inherit_window_position here
            // For now, new character always spawns centered
            if self.inherit_window_position {
                if let Some(&pos) = self.window_positions.get(&window) {
                    info!(character = %character_name, position = ?pos, "Inheriting window position for new character");
                    return Some(pos);
                }
            }
            
            // New character with no saved position → return None (will center)
            return None;
        }
        
        // Logged-out window ("EVE" title) → use window position from this session
        if let Some(&pos) = self.window_positions.get(&window) {
            info!(window = window, position = ?pos, "Using session position for logged-out window");
            Some(pos)
        } else {
            None
        }
    }

    /// Update session position (window tracking)
    pub fn update_window_position(&mut self, window: Window, x: i16, y: i16) {
        self.window_positions.insert(window, Position::new(x, y));
        info!(window = window, x = x, y = y, "Saved session position for window");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_position_character_from_config() {
        let state = SavedState::new();
        let mut char_positions = HashMap::new();
        char_positions.insert("Alice".to_string(), CharacterSettings::new(100, 200, 240, 135));
        
        let pos = state.get_position("Alice", 123, &char_positions);
        assert_eq!(pos, Some(Position::new(100, 200)));
    }

    #[test]
    fn test_get_position_new_character_no_inherit() {
        let state = SavedState {
            window_positions: HashMap::from([(456, Position::new(300, 400))]),
            inherit_window_position: false,
        };
        let char_positions = HashMap::new();
        
        // New character "Bob" with window 456 that has position → should return None (center)
        let pos = state.get_position("Bob", 456, &char_positions);
        assert_eq!(pos, None);
    }

    #[test]
    fn test_get_position_new_character_with_inherit() {
        let state = SavedState {
            window_positions: HashMap::from([(789, Position::new(500, 600))]),
            inherit_window_position: true,
        };
        let char_positions = HashMap::new();
        
        // New character "Charlie" with inherit enabled → should use window position
        let pos = state.get_position("Charlie", 789, &char_positions);
        assert_eq!(pos, Some(Position::new(500, 600)));
    }

    #[test]
    fn test_get_position_new_character_inherit_but_no_window_position() {
        let state = SavedState {
            window_positions: HashMap::new(),
            inherit_window_position: true,
        };
        let char_positions = HashMap::new();
        
        // inherit enabled but window 999 has no saved position → None (center)
        let pos = state.get_position("Diana", 999, &char_positions);
        assert_eq!(pos, None);
    }

    #[test]
    fn test_get_position_logged_out_window() {
        let state = SavedState {
            window_positions: HashMap::from([(111, Position::new(700, 800))]),
            inherit_window_position: false,
        };
        let char_positions = HashMap::new();
        
        // Empty character name (logged-out "EVE" window) → use window position
        let pos = state.get_position("", 111, &char_positions);
        assert_eq!(pos, Some(Position::new(700, 800)));
    }

    #[test]
    fn test_get_position_logged_out_window_no_saved_position() {
        let state = SavedState::new();
        let char_positions = HashMap::new();
        
        // Logged-out window with no saved position → None (center)
        let pos = state.get_position("", 222, &char_positions);
        assert_eq!(pos, None);
    }

    #[test]
    fn test_get_position_character_priority_over_window() {
        let mut state = SavedState::new();
        state.window_positions.insert(333, Position::new(900, 1000));
        state.inherit_window_position = true;
        
        let mut char_positions = HashMap::new();
        char_positions.insert("Eve".to_string(), CharacterSettings::new(1100, 1200, 240, 135));
        
        // Character position should take priority even with inherit enabled
        let pos = state.get_position("Eve", 333, &char_positions);
        assert_eq!(pos, Some(Position::new(1100, 1200)));
    }

    #[test]
    fn test_update_window_position() {
        let mut state = SavedState::new();
        
        state.update_window_position(444, 1300, 1400);
        assert_eq!(state.window_positions.get(&444), Some(&Position::new(1300, 1400)));
        
        // Update existing position
        state.update_window_position(444, 1500, 1600);
        assert_eq!(state.window_positions.get(&444), Some(&Position::new(1500, 1600)));
    }

    #[test]
    fn test_update_window_position_multiple_windows() {
        let mut state = SavedState::new();
        
        state.update_window_position(555, 100, 200);
        state.update_window_position(666, 300, 400);
        
        assert_eq!(state.window_positions.get(&555), Some(&Position::new(100, 200)));
        assert_eq!(state.window_positions.get(&666), Some(&Position::new(300, 400)));
    }
}
