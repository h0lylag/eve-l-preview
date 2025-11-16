//! Configuration management for EVE-L-Preview
//!
//! This module provides two config systems:
//! - **persistent**: PersistentState used by the preview daemon (flattened TOML)
//! - **profile**: Config with profile support used by the GUI manager (structured TOML)

pub mod persistent;
pub mod profile;

// Re-export commonly used types
pub use persistent::{DisplayConfig, GlobalSettings, PersistentState};
pub use profile::{Config, GlobalSettingsPhase2, ManagerSettings, Profile};
