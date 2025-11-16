//! Profile-based configuration for GUI manager
//!
//! New config architecture with support for multiple profiles,
//! each containing a complete set of visual and behavioral settings.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use tracing::info;

use crate::types::CharacterSettings;

/// Strategy for saving configuration files
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveStrategy {
    /// Preserve character_positions entries already on disk (GUI edits)
    PreserveCharacterPositions,
    /// Overwrite character_positions with in-memory data (daemon updates)
    OverwriteCharacterPositions,
}

/// Top-level configuration with profile support
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub manager: ManagerSettings,
    #[serde(default)]
    pub global: GlobalSettingsPhase2,
    #[serde(default = "default_profiles")]
    pub profiles: Vec<Profile>,
}

/// Manager-specific settings (window state, selected profile)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagerSettings {
    #[serde(default = "default_profile_name")]
    pub selected_profile: String,
    #[serde(default = "default_window_width")]
    pub window_width: u16,
    #[serde(default = "default_window_height")]
    pub window_height: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_x: Option<i16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_y: Option<i16>,
}

/// Global daemon behavior (applies to all profiles)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalSettingsPhase2 {
    #[serde(default = "default_log_level")]
    pub log_level: String,
    #[serde(default)]
    pub minimize_clients_on_switch: bool,
}

/// Profile - A complete set of visual and behavioral settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    #[serde(default)]
    pub description: String,
    
    // Visual settings
    #[serde(rename = "opacity_percent")]
    pub opacity_percent: u8,
    #[serde(default = "default_border_enabled")]
    pub border_enabled: bool,
    pub border_size: u16,
    #[serde(rename = "border_color")]
    pub border_color: String,
    pub text_size: u16,
    pub text_x: i16,
    pub text_y: i16,
    #[serde(rename = "text_foreground")]
    pub text_foreground: String,
    #[serde(rename = "text_background")]
    pub text_background: String,
    
    // Behavior settings
    pub hide_when_no_focus: bool,
    pub snap_threshold: u16,
    
    // Hotkey settings
    pub hotkey_require_eve_focus: bool,
    #[serde(default)]
    pub cycle_group: Vec<String>,
    
    // Per-profile character positions and dimensions
    // Skip serializing if empty to avoid creating empty [profiles.characters] table
    #[serde(rename = "characters", default, skip_serializing_if = "HashMap::is_empty")]
    pub character_positions: HashMap<String, CharacterSettings>,
}

// Default value functions
fn default_profile_name() -> String {
    "default".to_string()
}

fn default_window_width() -> u16 {
    600
}

fn default_window_height() -> u16 {
    800
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_border_enabled() -> bool {
    true
}

fn default_profiles() -> Vec<Profile> {
    vec![Profile {
        name: "default".to_string(),
        description: "Default profile".to_string(),
        opacity_percent: 75,
        border_enabled: true,
        border_size: 3,
        border_color: "#7FFF0000".to_string(),
        text_size: 22,
        text_x: 10,
        text_y: 20,
        text_foreground: "#FFFFFFFF".to_string(),
        text_background: "#7F000000".to_string(),
        hide_when_no_focus: false,
        snap_threshold: 15,
        hotkey_require_eve_focus: false,
        cycle_group: Vec::new(),
        character_positions: HashMap::new(),
    }]
}

impl Default for ManagerSettings {
    fn default() -> Self {
        Self {
            selected_profile: default_profile_name(),
            window_width: default_window_width(),
            window_height: default_window_height(),
            window_x: None,
            window_y: None,
        }
    }
}

impl Default for GlobalSettingsPhase2 {
    fn default() -> Self {
        Self {
            log_level: default_log_level(),
            minimize_clients_on_switch: false,
        }
    }
}

impl Profile {
    /// Create a new profile with default values and the given name
    pub fn default_with_name(name: String, description: String) -> Self {
        let mut profile = default_profiles().into_iter().next().unwrap();
        profile.name = name;
        profile.description = description;
        profile
    }
}

impl Config {
    pub fn path() -> PathBuf {
        let mut path = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
        path.push(crate::constants::config::APP_DIR);
        path.push(crate::constants::config::FILENAME);
        path
    }
    
    /// Load configuration from TOML file or create default
    pub fn load() -> Result<Self> {
        let config_path = Self::path();
        
        if !config_path.exists() {
            info!("Config file not found, creating default config at {:?}", config_path);
            let config = Config::default();
            config.save()?;
            return Ok(config);
        }
        
        let contents = fs::read_to_string(&config_path)
            .with_context(|| format!("Failed to read config from {:?}", config_path))?;
        
        let config: Config = toml::from_str(&contents)
            .with_context(|| format!("Failed to parse TOML from {:?}", config_path))?;
        
        info!("Loaded config with {} profile(s)", config.profiles.len());
        Ok(config)
    }
    
    /// Save configuration to TOML file using chosen strategy
    pub fn save_with_strategy(&self, strategy: SaveStrategy) -> Result<()> {
        let config_path = Self::path();
        
        // Ensure config directory exists
        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create config directory {:?}", parent))?;
        }
        
        let config_to_save = match strategy {
            SaveStrategy::PreserveCharacterPositions => {
                let mut clone = self.clone();
                if config_path.exists() {
                    if let Ok(contents) = fs::read_to_string(&config_path) {
                        if let Ok(existing_config) = toml::from_str::<Config>(&contents) {
                            for profile_to_save in clone.profiles.iter_mut() {
                                if let Some(existing_profile) = existing_config.profiles.iter()
                                    .find(|p| p.name == profile_to_save.name)
                                {
                                    profile_to_save.character_positions = existing_profile.character_positions.clone();
                                }
                            }
                        }
                    }
                }
                clone
            }
            SaveStrategy::OverwriteCharacterPositions => self.clone(),
        };
        
        let toml_string = toml::to_string_pretty(&config_to_save)
            .context("Failed to serialize config to TOML")?;
        
        fs::write(&config_path, toml_string)
            .with_context(|| format!("Failed to write config to {:?}", config_path))?;
        
        info!("Saved config to {:?}", config_path);
        Ok(())
    }

    /// Convenience helper: save preserving character positions (GUI default)
    pub fn save(&self) -> Result<()> {
        self.save_with_strategy(SaveStrategy::PreserveCharacterPositions)
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            manager: ManagerSettings::default(),
            global: GlobalSettingsPhase2::default(),
            profiles: default_profiles(),
        }
    }
}
