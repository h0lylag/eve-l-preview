use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::PathBuf;
use tracing::{error, info, warn};
use x11rb::protocol::render::Color;

use crate::color::{HexColor, Opacity};
use crate::types::{CharacterSettings, Position, TextOffset};

/// Immutable display settings (loaded once at startup)
/// Can be borrowed by Thumbnails without RefCell
#[derive(Debug, Clone)]
/// Shared display configuration for all thumbnails
/// Per-character dimensions are stored in CharacterSettings, not here
pub struct DisplayConfig {
    pub opacity: u32,
    pub border_size: u16,
    pub border_color: Color,
    pub text_offset: TextOffset,
    pub text_foreground: u32,
    pub hide_when_no_focus: bool,
}

/// Persistent state that gets saved to TOML file
/// Contains both immutable display config and mutable runtime data
#[derive(Debug, Serialize, Deserialize)]
pub struct PersistentState {
    // Global settings section (flattened in TOML)
    #[serde(flatten)]
    pub global: GlobalSettings,
    
    // Per-character settings section
    #[serde(rename = "characters", default)]
    pub character_positions: HashMap<String, CharacterSettings>,
}

/// Global/default settings that apply to all thumbnails
#[derive(Debug, Serialize, Deserialize)]
pub struct GlobalSettings {
    #[serde(rename = "opacity_percent")]
    opacity_percent: u8,
    pub border_size: u16,
    #[serde(rename = "border_color")]
    border_color_hex: String,
    pub text_x: i16,
    pub text_y: i16,
    #[serde(rename = "text_color")]
    text_color_hex: String,
    
    /// Text size in pixels (accepts integer or float)
    #[serde(rename = "text_size", default = "default_text_size", deserialize_with = "deserialize_text_size", serialize_with = "serialize_text_size")]
    pub text_size: f32,
    
    pub hide_when_no_focus: bool,
    
    /// Snap threshold in pixels (0 = disabled)
    #[serde(default = "default_snap_threshold")]
    pub snap_threshold: u16,
    
    /// Default thumbnail width for new characters
    #[serde(default = "default_width")]
    pub default_width: u16,
    
    /// Default thumbnail height for new characters
    #[serde(default = "default_height")]
    pub default_height: u16,
    
    /// Character order for hotkey cycling (Tab/Shift+Tab)
    /// Characters are auto-added when first seen, but can be manually ordered
    #[serde(default)]
    pub hotkey_order: Vec<String>,
    
    /// Only allow hotkey cycling when an EVE window is focused
    #[serde(default = "default_hotkey_require_eve_focus")]
    pub hotkey_require_eve_focus: bool,
}

fn default_text_size() -> f32 {
    18.0
}

/// Custom deserializer that accepts both integer and float for text_size
fn deserialize_text_size<'de, D>(deserializer: D) -> Result<f32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;
    
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum IntOrFloat {
        Int(i64),
        Float(f32),
    }
    
    match IntOrFloat::deserialize(deserializer)? {
        IntOrFloat::Int(i) => Ok(i as f32),
        IntOrFloat::Float(f) => Ok(f),
    }
}

/// Custom serializer that writes whole numbers without decimal point
fn serialize_text_size<S>(value: &f32, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    if value.fract() == 0.0 {
        // Whole number - serialize as integer
        serializer.serialize_i64(*value as i64)
    } else {
        // Has decimal - serialize as float
        serializer.serialize_f32(*value)
    }
}

fn default_snap_threshold() -> u16 {
    15
}

fn default_width() -> u16 {
    250
}

fn default_height() -> u16 {
    141
}

fn default_hotkey_require_eve_focus() -> bool {
    true
}

fn serialize_color<S>(hex: &String, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(hex)
}

fn deserialize_color<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    String::deserialize(deserializer)
}

impl PersistentState {
    fn config_path() -> PathBuf {
        let mut path = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
        path.push(crate::constants::config::APP_DIR);
        path.push(crate::constants::config::FILENAME);
        path
    }

    /// Get default thumbnail dimensions for screen size
    pub fn default_thumbnail_size(&self, _screen_width: u16, _screen_height: u16) -> (u16, u16) {
        // Use configured defaults from TOML
        (self.global.default_width, self.global.default_height)
    }
    
    /// Update hotkey order and save
    pub fn update_hotkey_order(&mut self, order: Vec<String>) -> Result<()> {
        self.global.hotkey_order = order;
        self.save()
    }

    /// Validate and clamp config values to safe ranges
    /// Called after loading TOML or creating from env vars
    fn validate_and_clamp(&mut self) {
        use crate::constants::validation::*;
        
        // Opacity already limited by u8 type (0-255), but clamp to 0-100 for percentage
        if self.global.opacity_percent > 100 {
            warn!(opacity_percent = self.global.opacity_percent, "opacity_percent exceeds 100, clamping to 100");
            self.global.opacity_percent = 100;
        }
        
        // Border size should be reasonable (0-100 pixels)
        if self.global.border_size > MAX_BORDER_SIZE {
            warn!(border_size = self.global.border_size, max = MAX_BORDER_SIZE, "border_size exceeds maximum, clamping");
            self.global.border_size = MAX_BORDER_SIZE;
        }
        
        // Text size should be reasonable (1.0-200.0 pixels)
        if self.global.text_size < MIN_TEXT_SIZE {
            warn!(text_size = self.global.text_size, min = MIN_TEXT_SIZE, "text_size below minimum, clamping");
            self.global.text_size = MIN_TEXT_SIZE;
        } else if self.global.text_size > MAX_TEXT_SIZE {
            warn!(text_size = self.global.text_size, max = MAX_TEXT_SIZE, "text_size exceeds maximum, clamping");
            self.global.text_size = MAX_TEXT_SIZE;
        }
        
        // Default dimensions should be non-zero (1-4096 pixels)
        if self.global.default_width < MIN_DIMENSION {
            warn!(default_width = self.global.default_width, min = MIN_DIMENSION, using = default_width(), "default_width below minimum, using default");
            self.global.default_width = default_width();
        } else if self.global.default_width > MAX_DIMENSION {
            warn!(default_width = self.global.default_width, max = MAX_DIMENSION, "default_width exceeds maximum, clamping");
            self.global.default_width = MAX_DIMENSION;
        }
        
        if self.global.default_height < MIN_DIMENSION {
            warn!(default_height = self.global.default_height, min = MIN_DIMENSION, using = default_height(), "default_height below minimum, using default");
            self.global.default_height = default_height();
        } else if self.global.default_height > MAX_DIMENSION {
            warn!(default_height = self.global.default_height, max = MAX_DIMENSION, "default_height exceeds maximum, clamping");
            self.global.default_height = MAX_DIMENSION;
        }
        
        // Snap threshold should be reasonable (0-1000 pixels, 0 = disabled)
        if self.global.snap_threshold > MAX_SNAP_THRESHOLD {
            warn!(snap_threshold = self.global.snap_threshold, max = MAX_SNAP_THRESHOLD, "snap_threshold exceeds maximum, clamping");
            self.global.snap_threshold = MAX_SNAP_THRESHOLD;
        }
        
        // Validate per-character dimensions
        for (character, settings) in &mut self.character_positions {
            let mut changed = false;
            
            if settings.dimensions.width > 0 && settings.dimensions.width < MIN_DIMENSION {
                warn!(character = %character, width = settings.dimensions.width, using = self.global.default_width, "character width below minimum, using default");
                settings.dimensions.width = self.global.default_width;
                changed = true;
            } else if settings.dimensions.width > MAX_DIMENSION {
                warn!(character = %character, width = settings.dimensions.width, max = MAX_DIMENSION, "character width exceeds maximum, clamping");
                settings.dimensions.width = MAX_DIMENSION;
                changed = true;
            }
            
            if settings.dimensions.height > 0 && settings.dimensions.height < MIN_DIMENSION {
                warn!(character = %character, height = settings.dimensions.height, using = self.global.default_height, "character height below minimum, using default");
                settings.dimensions.height = self.global.default_height;
                changed = true;
            } else if settings.dimensions.height > MAX_DIMENSION {
                warn!(character = %character, height = settings.dimensions.height, max = MAX_DIMENSION, "character height exceeds maximum, clamping");
                settings.dimensions.height = MAX_DIMENSION;
                changed = true;
            }
            
            if changed {
                info!(character = %character, width = settings.dimensions.width, height = settings.dimensions.height, "Corrected dimensions for character");
            }
        }
    }

    /// Build DisplayConfig from current settings
    /// Returns a new DisplayConfig that can be used independently
    /// Note: Per-character dimensions are not included here - they're in CharacterSettings
    pub fn build_display_config(&self) -> DisplayConfig {
        // Parse colors from hex strings using color module
        // Supports both 6-digit (RRGGBB) and 8-digit (AARRGGBB) formats
        // 6-digit format automatically gets full opacity (FF) prepended
        // Optional '#' prefix is supported but not required
        let border_color = HexColor::parse(&self.global.border_color_hex)
            .map(|c| c.to_x11_color())
            .unwrap_or_else(|| {
                error!(border_color = %self.global.border_color_hex, "Invalid border_color hex, using default");
                HexColor::from_argb32(0xFFFF0000).to_x11_color()
            });
        
        let text_foreground = HexColor::parse(&self.global.text_color_hex)
            .map(|c| c.argb32())  // Use raw ARGB, not premultiplied
            .unwrap_or_else(|| {
                error!(text_color = %self.global.text_color_hex, "Invalid text_color hex, using default");
                HexColor::from_argb32(0xFF_FF_FF_FF).argb32()
            });
        
        let opacity = Opacity::from_percent(self.global.opacity_percent).to_argb32();
        
        DisplayConfig {
            opacity,
            border_size: self.global.border_size,
            border_color,
            text_offset: TextOffset::from_border_edge(self.global.text_x, self.global.text_y),
            text_foreground,
            hide_when_no_focus: self.global.hide_when_no_focus,
        }
    }
    pub fn load() -> Self {
        // Try to load existing config file
        let config_path = Self::config_path();
        if let Ok(contents) = fs::read_to_string(&config_path) {
            match toml::from_str::<PersistentState>(&contents) {
                Ok(mut state) => {
                    // Apply env var overrides
                    state.apply_env_overrides();
                    
                    // Validate and clamp all values to safe ranges
                    state.validate_and_clamp();
                    
                    // Auto-save if config is missing new fields (e.g., default_width/default_height)
                    // This ensures existing configs get updated with new options
                    if !contents.contains("default_width") || !contents.contains("default_height") {
                        info!("Updating config with new fields (default_width, default_height)");
                        if let Err(e) = state.save()
                            .context("Failed to save config after adding new fields") {
                                error!(error = ?e, "Failed to update config");
                            }
                    }
                    
                    return state;
                }
                Err(e) => {
                    error!(path = %config_path.display(), error = %e, "Failed to parse config file");
                    error!(path = %config_path.display(), "Please fix the syntax errors in your config file.");
                    error!(path = %config_path.display(), "The file has been preserved - check for missing quotes around color values.");
                    // Don't overwrite the broken config - user needs to fix it
                    std::process::exit(1);
                }
            }
        }

        // Generate new config from env vars (with fallback defaults)
        let mut state = Self::from_env(None);
        
        // Validate and clamp all values to safe ranges
        state.validate_and_clamp();
        
        // Save for next time
        if let Err(e) = state.save()
            .context(format!("Failed to save new config to {}", config_path.display())) {
            error!(error = ?e, "Failed to save config");
        } else {
            info!(path = %config_path.display(), "Generated config file for user to edit (env vars still override)");
        }
        
        state
    }

    /// Load config with screen size for smart defaults
    /// Used when X11 connection is available before config load
    /// Note: Dimensions are now per-character, auto-detected at runtime, not during config load
    pub fn load_with_screen(_screen_width: u16, _screen_height: u16) -> Self {
        // Just load normally - dimensions are handled per-character at runtime
        Self::load()
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .context(format!("Failed to create config directory: {}", parent.display()))?;
        }
        let contents = toml::to_string_pretty(self)
            .context("Failed to serialize config to TOML")?;
        fs::write(&path, contents)
            .context(format!("Failed to write config file to {}", path.display()))?;
        Ok(())
    }

    /// Update position and dimensions after drag - saves to character_positions and persists
    /// Update character position and dimensions
    /// This is called when a thumbnail is dragged or when dimensions change
    pub fn update_position(&mut self, character_name: &str, x: i16, y: i16, width: u16, height: u16) -> Result<()> {
        if !character_name.is_empty() {
        info!(character = %character_name, x = x, y = y, width = width, height = height, "Saving position and dimensions for character");
            let settings = CharacterSettings::new(x, y, width, height);
            self.character_positions.insert(character_name.to_string(), settings);
            self.save()
                .context(format!("Failed to save config after updating position for '{}'", character_name))?;
        }
        Ok(())
    }

    /// Handle character name change (login/logout)
    /// Returns new position if the new character has a saved position
    /// Accepts current thumbnail dimensions to ensure they're saved correctly
    pub fn handle_character_change(
        &mut self,
        old_name: &str,
        new_name: &str,
        current_position: Position,
        current_width: u16,
        current_height: u16,
    ) -> Result<Option<Position>> {
    info!(old = %old_name, new = %new_name, "Character change");
        
        // Save old character's position and current dimensions
        if !old_name.is_empty() {
            let settings = CharacterSettings::new(
                current_position.x, 
                current_position.y, 
                current_width, 
                current_height
            );
            self.character_positions.insert(old_name.to_string(), settings);
        }
        
        // Save to disk
        self.save()
            .context(format!("Failed to save config after character change from '{}' to '{}'", old_name, new_name))?;
        
        // Return new position if we have one saved for the new character
        if !new_name.is_empty() {
            if let Some(settings) = self.character_positions.get(new_name) {
                info!(character = %new_name, x = settings.x, y = settings.y, "Moving to saved position for character");
                return Ok(Some(settings.position()));
            }
        }
        
        // Character logged out OR new character with no saved position → keep current position
        Ok(None)
    }

    fn parse_num<T: std::str::FromStr + TryFrom<u128>>(var: &str) -> Option<T> where <T as TryFrom<u128>>::Error: std::fmt::Debug, <T as std::str::FromStr>::Err: std::fmt::Debug {
        if let Ok(s) = env::var(var) {
            let s = s.trim();
            if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X"))
                && let Ok(n) = u128::from_str_radix(hex, 16)
            {
                return T::try_from(n).inspect_err(|e| error!(var = %var, error = ?e, "failed to parse hex env var")).ok();
            } else {
                return s.parse::<T>().inspect_err(|e| error!(var = %var, error = ?e, "failed to parse env var" )).ok();
            }
        }
        None
    }

    fn from_env(_screen_size: Option<(u16, u16)>) -> Self {
        let border_color_raw = Self::parse_num("BORDER_COLOR").unwrap_or(0xFFFF0000);
        let opacity = Self::parse_num("OPACITY").unwrap_or(0xCC000000);  // 80% opacity (0xCC = 204)
        let text_color_raw = Self::parse_num("TEXT_COLOR").unwrap_or(0xFF_FF_FF_FF);
        
        // No global width/height - dimensions are per-character now
        // Screen size is used only for auto-detecting new characters in runtime
        
        Self {
            global: GlobalSettings {
                opacity_percent: Opacity::from_argb32(opacity).percent(),
                border_size: Self::parse_num("BORDER_SIZE").unwrap_or(5),
                border_color_hex: HexColor::from_argb32(border_color_raw).to_hex_string(),
                text_x: Self::parse_num("TEXT_X").unwrap_or(10),
                text_y: Self::parse_num("TEXT_Y").unwrap_or(10),
                text_color_hex: HexColor::from_argb32(text_color_raw).to_hex_string(),
                hide_when_no_focus: env::var("HIDE_WHEN_NO_FOCUS")
                    .map(|x| x.parse().unwrap_or(false))
                    .unwrap_or(false),
                text_size: 18.0,
                snap_threshold: 15,
                default_width: 250,
                default_height: 141,
                // Example hotkey order - edit this with your character names!
                hotkey_order: vec![
                    "Main Character".to_string(),
                    "Alt 1".to_string(),
                    "Alt 2".to_string(),
                ],
                hotkey_require_eve_focus: true,
            },
            character_positions: HashMap::new(),
        }
    }

    fn apply_env_overrides(&mut self) {
        // Width/height are now per-character, no global env override
        if let Some(opacity) = Self::parse_num("OPACITY") {
            self.global.opacity_percent = Opacity::from_argb32(opacity).percent();
        }
        if let Some(border_size) = Self::parse_num("BORDER_SIZE") {
            self.global.border_size = border_size;
        }
        if let Some(border_color_raw) = Self::parse_num("BORDER_COLOR") {
            self.global.border_color_hex = HexColor::from_argb32(border_color_raw).to_hex_string();
        }
        if let Some(text_x) = Self::parse_num("TEXT_X") {
            self.global.text_x = text_x;
        }
        if let Some(text_y) = Self::parse_num("TEXT_Y") {
            self.global.text_y = text_y;
        }
        if let Some(text_color) = Self::parse_num("TEXT_COLOR") {
            self.global.text_color_hex = HexColor::from_argb32(text_color).to_hex_string();
        }
        if let Ok(hide) = env::var("HIDE_WHEN_NO_FOCUS") {
            self.global.hide_when_no_focus = hide.parse().unwrap_or(false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Helper function to create test GlobalSettings
    fn test_global_settings(
        opacity_percent: u8,
        border_size: u16,
        border_color_hex: &str,
        text_x: i16,
        text_y: i16,
        text_color_hex: &str,
        hide_when_no_focus: bool,
        snap_threshold: u16,
    ) -> GlobalSettings {
        GlobalSettings {
            opacity_percent,
            border_size,
            border_color_hex: border_color_hex.to_string(),
            text_x,
            text_y,
            text_color_hex: text_color_hex.to_string(),
            hide_when_no_focus,
            text_size: 18.0,
            snap_threshold,
            default_width: 250,
            default_height: 141,
            hotkey_order: Vec::new(),
            hotkey_require_eve_focus: true,
        }
    }

    #[test]
    fn test_build_display_config_valid_colors() {
        let state = PersistentState {
            global: test_global_settings(
                75,  // opacity_percent
                3,   // border_size
                "#FF00FF00",  // Green border
                15,  // text_x
                25,  // text_y
                "#FFFFFFFF",  // White text color
                true,  // hide_when_no_focus
                20,  // snap_threshold
            ),
            character_positions: HashMap::new(),
        };

        let config = state.build_display_config();
        assert_eq!(config.border_size, 3);
        assert_eq!(config.text_offset.x, 15);
        assert_eq!(config.text_offset.y, 25);
        assert_eq!(config.hide_when_no_focus, true);
        
        // Opacity: 75% → 0xBF
        assert_eq!(config.opacity, 0xBF000000);
        
        // Border color: #FF00FF00 → Color { red: 0, green: 65535, blue: 0, alpha: 65535 }
        assert_eq!(config.border_color.red, 0);
        assert_eq!(config.border_color.green, 65535);
        assert_eq!(config.border_color.blue, 0);
        assert_eq!(config.border_color.alpha, 65535);
    }

    #[test]
    fn test_build_display_config_invalid_colors_fallback() {
        let state = PersistentState {
            global: test_global_settings(
                100,  // opacity_percent
                5,    // border_size
                "invalid",  // invalid border color
                10,   // text_x
                20,   // text_y
                "also_invalid",  // invalid text color
                false,  // hide_when_no_focus
                15,  // snap_threshold
            ),
            character_positions: HashMap::new(),
        };

        let config = state.build_display_config();
        
        // Opacity: 100% → 0xFF
        assert_eq!(config.opacity, 0xFF000000);
        
        // Default border_color: 0xFFFF0000 (opaque red)
        // Alpha conversion: 0xFF (255) * 257 = 65535 in 16-bit
        assert_eq!(config.border_color.red, 65535);
        assert_eq!(config.border_color.blue, 0);
        assert_eq!(config.border_color.alpha, 65535);
    }

    #[test]
    fn test_update_position_with_character_name() {
        let mut state = PersistentState {
            global: test_global_settings(
                75, 3, "#FF00FF00", 10, 20, "#FFFFFFFF", false, 15,
            ),
            character_positions: HashMap::new(),
        };

        // Update position with dimensions
        let _ = state.update_position("TestChar", 100, 200, 480, 270);
        
        let settings = state.character_positions.get("TestChar").unwrap();
        assert_eq!(settings.x, 100);
        assert_eq!(settings.y, 200);
        assert_eq!(settings.dimensions.width, 480);
        assert_eq!(settings.dimensions.height, 270);
    }

    #[test]
    fn test_update_position_empty_name_ignored() {
        let mut state = PersistentState {
            global: test_global_settings(
                75, 3, "#FF00FF00", 10, 20, "#FFFFFFFF", false, 15,
            ),
            character_positions: HashMap::new(),
        };

        let _ = state.update_position("", 300, 400, 480, 270);
        
        // Empty name should not be inserted
        assert!(state.character_positions.is_empty());
    }

    #[test]
    fn test_handle_character_change_both_names() {
        let mut state = PersistentState {
            global: test_global_settings(
                75, 3, "#FF00FF00", 10, 20, "#FFFFFFFF", false, 15,
            ),
            character_positions: HashMap::from([("NewChar".to_string(), CharacterSettings::new(500, 600, 240, 135))]),
        };

        let current_pos = Position::new(100, 200);
        // This will fail to save (file I/O in test), but we check the logic
        let result = state.handle_character_change("OldChar", "NewChar", current_pos, 480, 270);
        
        // Should save old position AND dimensions (even if disk save fails)
        let old_settings = state.character_positions.get("OldChar").unwrap();
        assert_eq!(old_settings.x, 100);
        assert_eq!(old_settings.y, 200);
        assert_eq!(old_settings.dimensions.width, 480);
        assert_eq!(old_settings.dimensions.height, 270);
        
        // File save will fail in test, so we just verify the position was looked up
        // The function returns Err because save() fails, not because logic is wrong
        assert!(result.is_err());
        
        // Verify the new position exists in the map (function would return it if save succeeded)
        let new_settings = state.character_positions.get("NewChar").unwrap();
        assert_eq!(new_settings.x, 500);
        assert_eq!(new_settings.y, 600);
    }

    #[test]
    fn test_handle_character_change_logout() {
        let mut state = PersistentState {
            global: test_global_settings(
                75, 3, "#FF00FF00", 10, 20, "#FFFFFFFF", false, 15,
            ),
            character_positions: HashMap::new(),
        };

        let current_pos = Position::new(300, 400);
        let result = state.handle_character_change("LoggingOut", "", current_pos, 480, 270);
        
        // Should save old position AND dimensions (even if disk save fails)
        let settings = state.character_positions.get("LoggingOut").unwrap();
        assert_eq!(settings.x, 300);
        assert_eq!(settings.y, 400);
        assert_eq!(settings.dimensions.width, 480);
        assert_eq!(settings.dimensions.height, 270);
        
        // File save will fail in test environment
        assert!(result.is_err());
    }

    #[test]
    fn test_handle_character_change_new_character_no_saved_position() {
        let mut state = PersistentState {
            global: test_global_settings(
                75, 3, "#FF00FF00", 10, 20, "#FFFFFFFF", false, 15,
            ),
            character_positions: HashMap::new(),
        };

        let current_pos = Position::new(700, 800);
        let result = state.handle_character_change("", "BrandNewChar", current_pos, 480, 270);
        
        // Empty old name not saved
        assert!(state.character_positions.is_empty());
        
        // File save will fail in test environment
        assert!(result.is_err());
    }

    #[test]
    fn test_opacity_percent_roundtrip() {
        // Test that opacity_percent converts correctly through Opacity type
        let state = PersistentState {
            global: test_global_settings(
                50, 3, "#FF00FF00", 10, 20, "#FFFFFFFF", false, 15,
            ),
            character_positions: HashMap::new(),
        };

        let config = state.build_display_config();
        
        // 50% → 0x7F or 0x80 (due to rounding)
        assert!(config.opacity >= 0x7F000000 && config.opacity <= 0x80000000);
    }
}
