//! GUI-specific constants for layout, status colors and intervals

use egui;

/// Manager window dimensions
pub const WINDOW_WIDTH: f32 = 600.0;
pub const WINDOW_HEIGHT: f32 = 800.0;
pub const WINDOW_MIN_WIDTH: f32 = 500.0;
pub const WINDOW_MIN_HEIGHT: f32 = 600.0;

/// Layout spacing
pub const SECTION_SPACING: f32 = 15.0;
pub const ITEM_SPACING: f32 = 8.0;

/// Status colors
pub const STATUS_RUNNING: egui::Color32 = egui::Color32::from_rgb(0, 200, 0);
pub const STATUS_STOPPED: egui::Color32 = egui::Color32::from_rgb(200, 0, 0);
pub const STATUS_STARTING: egui::Color32 = egui::Color32::from_rgb(200, 200, 0);

/// Daemon monitoring
pub const DAEMON_CHECK_INTERVAL_MS: u64 = 500;
