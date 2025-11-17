//! Application-wide constants
//!
//! This module contains all magic numbers and string literals used throughout
//! the application, providing a single source of truth for constant values.

/// X11 protocol and rendering constants
pub mod x11 {
    /// ARGB color depth (32-bit: 8 bits each for Alpha, Red, Green, Blue)
    pub const ARGB_DEPTH: u8 = 32;
    
    /// Size of PID property value in bytes
    pub const PID_PROPERTY_SIZE: usize = 4;
    
    /// Override redirect flag for unmanaged windows
    pub const OVERRIDE_REDIRECT: u32 = 1;
    
    /// Source indication for _NET_ACTIVE_WINDOW (2 = pager/direct user action)
    pub const ACTIVE_WINDOW_SOURCE_PAGER: u32 = 2;
    
    /// _NET_WM_STATE action: add/set property (1)
    pub const NET_WM_STATE_ADD: u32 = 1;

    /// WM_CHANGE_STATE iconic value (requests the WM to minimize)
    pub const ICONIC_STATE: u32 = 3;
}

/// Input event constants (from evdev)
pub mod input {
    /// Key press event value
    pub const KEY_PRESS: i32 = 1;
}

/// Mouse button constants
pub mod mouse {
    /// Left mouse button number
    pub const BUTTON_LEFT: u8 = 1;
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
    
    /// Display name for logged-out character (shown in logs)
    pub const LOGGED_OUT_DISPLAY_NAME: &str = "login_screen";
}

/// Default window positioning constants
pub mod positioning {
    /// Padding offset from source window when spawning thumbnails
    pub const DEFAULT_SPAWN_OFFSET: i16 = 20;
}

/// Fixed-point arithmetic constants (X11 render transforms)
pub mod fixed_point {
    /// Fixed-point multiplier for conversion (2^16)
    pub const MULTIPLIER: f32 = 65536.0;
}

/// System paths
pub mod paths {
    /// Linux proc filesystem path format for process executables
    pub const PROC_EXE_FORMAT: &str = "/proc/{}/exe";
    
    /// Input device directory
    pub const DEV_INPUT: &str = "/dev/input";
}

/// User group permissions
pub mod permissions {
    /// Linux group name for input device access
    pub const INPUT_GROUP: &str = "input";
    
    /// Command to add user to input group
    pub const ADD_TO_INPUT_GROUP: &str = "sudo usermod -a -G input $USER";
}

/// Configuration paths and filenames
pub mod config {
    /// Application directory name under XDG config
    pub const APP_DIR: &str = "eve-l-preview";
    
    /// Configuration filename
    pub const FILENAME: &str = "eve-l-preview.json";
}
