//! Application-wide constants
//!
//! This module contains all magic numbers and string literals used throughout
//! the application, providing a single source of truth for constant values.

/// X11 protocol and rendering constants
pub mod x11 {
    /// ARGB color depth (32-bit: 8 bits each for Alpha, Red, Green, Blue)
    pub const ARGB_DEPTH: u8 = 32;
    
    /// RGB color depth (24-bit: 8 bits each for Red, Green, Blue, no alpha)
    pub const RGB_DEPTH: u8 = 24;
    
    /// Size of PID property value in bytes
    pub const PID_PROPERTY_SIZE: usize = 4;
    
    /// Override redirect flag for unmanaged windows
    pub const OVERRIDE_REDIRECT: u32 = 1;
    
    /// Source indication for _NET_ACTIVE_WINDOW (2 = pager/direct user action)
    pub const ACTIVE_WINDOW_SOURCE_PAGER: u32 = 2;
}

/// Input event constants (from evdev)
pub mod input {
    /// Key press event value
    pub const KEY_PRESS: i32 = 1;
    
    /// Key release event value
    pub const KEY_RELEASE: i32 = 0;
    
    /// Key repeat event value
    pub const KEY_REPEAT: i32 = 2;
}

/// Mouse button constants
pub mod mouse {
    /// Left mouse button number
    pub const BUTTON_LEFT: u8 = 1;
    
    /// Middle mouse button number
    pub const BUTTON_MIDDLE: u8 = 2;
    
    /// Right mouse button number
    pub const BUTTON_RIGHT: u8 = 3;
}

/// Wine process detection constants
pub mod wine {
    /// Wine 64-bit preloader process name
    pub const WINE64_PRELOADER: &str = "wine64-preloader";
    
    /// Wine 32-bit preloader process name
    pub const WINE_PRELOADER: &str = "wine-preloader";
}

/// EVE Online window detection constants
pub mod eve {
    /// Prefix for EVE client window titles (followed by character name)
    pub const WINDOW_TITLE_PREFIX: &str = "EVE - ";
    
    /// Window title for logged-out EVE clients
    pub const LOGGED_OUT_TITLE: &str = "EVE";
}

/// Default window positioning constants
pub mod positioning {
    /// Padding offset from source window when spawning thumbnails
    pub const DEFAULT_SPAWN_OFFSET: i16 = 20;
}

/// Fixed-point arithmetic constants (X11 render transforms)
pub mod fixed_point {
    /// Fixed-point shift amount (16.16 format)
    pub const SHIFT: u32 = 16;
    
    /// Fixed-point multiplier for conversion (2^16)
    pub const MULTIPLIER: f32 = 65536.0;
}
