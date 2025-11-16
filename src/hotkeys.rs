use anyhow::{Context, Result};
use evdev::{Device, EventType, InputEventKind, Key};
use std::sync::mpsc::Sender;
use std::thread;
use tracing::{debug, error, info, warn};

use crate::constants::{input, paths, permissions};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CycleCommand {
    Forward,
    Backward,
}

/// Find all keyboard devices that support Tab key
fn find_all_keyboard_devices() -> Result<Vec<Device>> {
    info!(path = %paths::DEV_INPUT, "Scanning for keyboard devices...");
    
    let mut devices = Vec::new();
    
    for entry in std::fs::read_dir(paths::DEV_INPUT)
        .context(format!("Failed to read {} - are you in the '{}' group?", paths::DEV_INPUT, permissions::INPUT_GROUP))?
    {
        let entry = entry?;
        let path = entry.path();

        // Try to open device
        if let Ok(device) = Device::open(&path) {
            // Check if it has Tab key (indicates keyboard)
            if let Some(keys) = device.supported_keys() {
                    if keys.contains(Key::KEY_TAB) {
                    let key_count = keys.iter().count();
                    info!(device_path = %path.display(), name = ?device.name(), key_count = key_count, "Found keyboard device");
                    devices.push(device);
                }
            }
        }
    }

    if devices.is_empty() {
        anyhow::bail!(
            "No keyboard device found. Ensure you're in '{}' group:\n\
             {}\n\
             Then log out and back in.",
            permissions::INPUT_GROUP,
            permissions::ADD_TO_INPUT_GROUP
        )
    }

    info!(count = devices.len(), "Listening on keyboard device(s)");
    
    Ok(devices)
}

/// Spawn background threads to listen for Tab/Shift+Tab on all keyboard devices
pub fn spawn_listener(sender: Sender<CycleCommand>) -> Result<Vec<thread::JoinHandle<()>>> {
    let devices = find_all_keyboard_devices()?;
    let mut handles = Vec::new();

    for device in devices {
        let sender = sender.clone();
        let handle = thread::spawn(move || {
            info!(device = ?device.name(), "Hotkey listener started");
            if let Err(e) = listen_for_hotkeys(device, sender) {
                error!(error = %e, "Hotkey listener error");
            }
        });
        handles.push(handle);
    }

    Ok(handles)
}

/// Listen for Tab/Shift+Tab events on a single device
fn listen_for_hotkeys(mut device: Device, sender: Sender<CycleCommand>) -> Result<()> {
    loop {
        // Fetch events (blocks until available)
        let events = device.fetch_events()
            .context("Failed to fetch events")?;

        // Collect Tab press events that need processing
        // We need to finish with the events iterator before querying key state
        let mut tab_presses = Vec::new();

        for event in events {
            // Log all key events for debugging
            if event.event_type() == EventType::KEY {
                if let InputEventKind::Key(key) = event.kind() {
                    debug!(key = ?key, value = event.value(), "Key event");
                }
            }

            // Only care about key events
            if event.event_type() != EventType::KEY {
                continue;
            }

            if let InputEventKind::Key(key) = event.kind() {
                let pressed = event.value() == input::KEY_PRESS;

                if key == Key::KEY_TAB && pressed {
                    tab_presses.push(());
                }
            }
        }

        // Now process Tab presses with current keyboard state
        for _ in tab_presses {
            // Check real-time state of shift keys when Tab was pressed
            // This avoids race conditions from batched events
            let key_state = device.get_key_state()
                .context("Failed to get keyboard state")?;
            
            let shift_pressed = key_state.contains(Key::KEY_LEFTSHIFT) 
                || key_state.contains(Key::KEY_RIGHTSHIFT);

            let command = if shift_pressed {
                CycleCommand::Backward
            } else {
                CycleCommand::Forward
            };

            info!(shift = shift_pressed, command = ?command, "Tab hotkey pressed, sending command");

            sender.send(command)
                .context("Failed to send cycle command")?;
        }
    }
}

/// Check if hotkeys are available (user has input group permissions)
pub fn check_permissions() -> bool {
    std::fs::read_dir(paths::DEV_INPUT).is_ok()
}

/// Print helpful error message if permissions missing
pub fn print_permission_error() {
    error!(path = %paths::DEV_INPUT, "Cannot access input devices");
    error!(group = %permissions::INPUT_GROUP, "Hotkeys require group membership");
    error!(command = %permissions::ADD_TO_INPUT_GROUP, "Add user to input group");
    error!("  Then log out and back in");
    warn!(continuing = true, "Continuing without hotkey support...");
}
