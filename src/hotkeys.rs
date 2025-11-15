use anyhow::{Context, Result};
use evdev::{Device, EventType, InputEventKind, Key};
use std::sync::mpsc::Sender;
use std::thread;
use tracing::{debug, error, info, warn};

use crate::constants::input;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CycleCommand {
    Forward,
    Backward,
}

/// Find all keyboard devices that support Tab key
fn find_all_keyboard_devices() -> Result<Vec<Device>> {
    info!("Scanning /dev/input for keyboard devices...");
    
    let mut devices = Vec::new();
    
    for entry in std::fs::read_dir("/dev/input")
        .context("Failed to read /dev/input - are you in the 'input' group?")?
    {
        let entry = entry?;
        let path = entry.path();

        // Try to open device
        if let Ok(device) = Device::open(&path) {
            // Check if it has Tab key (indicates keyboard)
            if let Some(keys) = device.supported_keys() {
                if keys.contains(Key::KEY_TAB) {
                    let key_count = keys.iter().count();
                    info!("Found keyboard device: {} (name: {:?}, {} keys)", 
                          path.display(), device.name(), key_count);
                    devices.push(device);
                }
            }
        }
    }

    if devices.is_empty() {
        anyhow::bail!(
            "No keyboard device found. Ensure you're in 'input' group:\n\
             sudo usermod -a -G input $USER\n\
             Then log out and back in."
        )
    }

    info!("Listening on {} keyboard device(s)", devices.len());
    
    Ok(devices)
}

/// Spawn background threads to listen for Tab/Shift+Tab on all keyboard devices
pub fn spawn_listener(sender: Sender<CycleCommand>) -> Result<Vec<thread::JoinHandle<()>>> {
    let devices = find_all_keyboard_devices()?;
    let mut handles = Vec::new();

    for device in devices {
        let sender = sender.clone();
        let handle = thread::spawn(move || {
            info!("Hotkey listener started (device: {:?})", device.name());
            if let Err(e) = listen_for_hotkeys(device, sender) {
                error!("Hotkey listener error: {}", e);
            }
        });
        handles.push(handle);
    }

    Ok(handles)
}

/// Listen for Tab/Shift+Tab events on a single device
fn listen_for_hotkeys(mut device: Device, sender: Sender<CycleCommand>) -> Result<()> {
    let mut shift_pressed = false;

    loop {
        // Fetch events (blocks until available)
        let events = device.fetch_events()
            .context("Failed to fetch events")?;

        for event in events {
            // Log all key events for debugging
            if event.event_type() == EventType::KEY {
                if let InputEventKind::Key(key) = event.kind() {
                    debug!("Key event: {:?} value={}", key, event.value());
                }
            }

            // Only care about key events
            if event.event_type() != EventType::KEY {
                continue;
            }

            if let InputEventKind::Key(key) = event.kind() {
                let pressed = event.value() == input::KEY_PRESS;

                match key {
                    Key::KEY_LEFTSHIFT | Key::KEY_RIGHTSHIFT => {
                        shift_pressed = pressed;
                        debug!("Shift: {}", if pressed { "pressed" } else { "released" });
                    }
                    Key::KEY_TAB if pressed => {
                        // Only trigger on key press, not repeat or release
                        let command = if shift_pressed {
                            CycleCommand::Backward
                        } else {
                            CycleCommand::Forward
                        };

                        info!(
                            "Tab pressed (shift={}), sending {:?}",
                            shift_pressed, command
                        );

                        sender.send(command)
                            .context("Failed to send cycle command")?;
                    }
                    _ => {}
                }
            }
        }
    }
}

/// Check if hotkeys are available (user has input group permissions)
pub fn check_permissions() -> bool {
    std::fs::read_dir("/dev/input").is_ok()
}

/// Print helpful error message if permissions missing
pub fn print_permission_error() {
    error!("Cannot access /dev/input devices");
    error!("Hotkeys require 'input' group membership:");
    error!("  sudo usermod -a -G input $USER");
    error!("  Then log out and back in");
    warn!("Continuing without hotkey support...");
}
