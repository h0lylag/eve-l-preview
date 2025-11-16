//! GUI manager implemented with egui/eframe and tray-icon system tray support

use std::io::Cursor;
use std::process::{Child, Command};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use eframe::{egui, NativeOptions};
use tracing::{error, info, warn};

#[cfg(target_os = "linux")]
use tray_icon::{
    menu::{Menu, MenuEvent, MenuItem, MenuId, PredefinedMenuItem},
    Icon, TrayIconBuilder,
};

#[cfg(target_os = "linux")]
use gtk::glib::ControlFlow;

use super::{components, constants::*};
use crate::config::profile::{Config, Profile};
use crate::gui::components::profile_selector::{ProfileSelector, ProfileAction};

#[cfg(target_os = "linux")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TrayCommand {
    Reload,
    Quit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DaemonStatus {
    Starting,
    Running,
    Stopped,
    Crashed(Option<i32>),
}

impl DaemonStatus {
    fn color(&self) -> egui::Color32 {
        match self {
            DaemonStatus::Running => STATUS_RUNNING,
            DaemonStatus::Starting => STATUS_STARTING,
            _ => STATUS_STOPPED,
        }
    }

    fn label(&self) -> String {
        match self {
            DaemonStatus::Running => "\u{25CF}  Running".to_string(),
            DaemonStatus::Starting => "\u{25CF}  Starting...".to_string(),
            DaemonStatus::Stopped => "\u{25CF}  Stopped".to_string(),
            DaemonStatus::Crashed(code) => match code {
                Some(code) => format!("\u{25CF}  Crashed (exit {code})"),
                None => "\u{25CF}  Crashed".to_string(),
            },
        }
    }
}

struct StatusMessage {
    text: String,
    color: egui::Color32,
}

struct ManagerApp {
    daemon: Option<Child>,
    daemon_status: DaemonStatus,
    last_health_check: Instant,
    status_message: Option<StatusMessage>,
    #[cfg(target_os = "linux")]
    tray_rx: Receiver<TrayCommand>,
    should_quit: bool,
    
    // Configuration state with profiles
    config: Config,
    selected_profile_idx: usize,
    profile_selector: ProfileSelector,
    settings_changed: bool,
}

impl ManagerApp {
    fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        info!("Initializing egui manager");

        // Create channel for tray icon commands
        #[cfg(target_os = "linux")]
        let (tray_tx, tray_rx) = mpsc::channel();

        // Spawn GTK thread for tray icon on Linux
        // GTK must run in its own thread because it conflicts with eframe's event loop
        #[cfg(target_os = "linux")]
        std::thread::spawn(move || {
            if let Err(e) = gtk::init() {
                error!(error = ?e, "Failed to initialize GTK for tray icon");
                return;
            }
            
            match create_tray_icon(tray_tx.clone()) {
                Ok(tray_icon) => {
                    info!("Tray icon created in GTK thread");
                    
                    // Set up menu event listener
                    let menu_channel = MenuEvent::receiver();
                    let tx = tray_tx;
                    
                    // Poll for menu events in GTK thread
                    gtk::glib::timeout_add_local(Duration::from_millis(100), move || {
                        if let Ok(event) = menu_channel.try_recv() {
                            let id = event.id.0;
                            info!(menu_id = %id, "Tray menu event received");
                            
                            if id == "reload" {
                                let _ = tx.send(TrayCommand::Reload);
                            } else if id == "quit" {
                                let _ = tx.send(TrayCommand::Quit);
                            }
                        }
                        ControlFlow::Continue
                    });
                    
                    // Keep tray_icon alive by moving it into a leaked Box
                    // This prevents it from being dropped when the thread continues
                    Box::leak(Box::new(tray_icon));
                    gtk::main();
                }
                Err(e) => {
                    error!(error = ?e, "Failed to create tray icon");
                }
            }
        });

        // Load configuration
        let config = Config::load().unwrap_or_default();
        
        // Find selected profile index
        let selected_profile_idx = config.profiles
            .iter()
            .position(|p| p.name == config.manager.selected_profile)
            .unwrap_or(0);

        #[cfg(target_os = "linux")]
        let mut app = Self {
            daemon: None,
            daemon_status: DaemonStatus::Stopped,
            last_health_check: Instant::now(),
            status_message: None,
            tray_rx,
            should_quit: false,
            config,
            selected_profile_idx,
            profile_selector: ProfileSelector::new(),
            settings_changed: false,
        };

        #[cfg(not(target_os = "linux"))]
        let mut app = Self {
            daemon: None,
            daemon_status: DaemonStatus::Stopped,
            last_health_check: Instant::now(),
            status_message: None,
            should_quit: false,
            config,
            selected_profile_idx,
            profile_selector: ProfileSelector::new(),
            settings_changed: false,
        };

        if let Err(err) = app.start_daemon() {
            error!(error = ?err, "Failed to start preview daemon");
            app.status_message = Some(StatusMessage {
                text: format!("Failed to start daemon: {err}"),
                color: STATUS_STOPPED,
            });
        }

        app
    }

    fn start_daemon(&mut self) -> Result<()> {
        if self.daemon.is_some() {
            return Ok(());
        }

        let child = spawn_preview_daemon()?;
        let pid = child.id();
        info!(pid, "Started preview daemon");

        self.daemon = Some(child);
        self.daemon_status = DaemonStatus::Starting;
        self.status_message = Some(StatusMessage {
            text: format!("Preview daemon starting (PID: {pid})"),
            color: STATUS_STARTING,
        });
        Ok(())
    }

    fn stop_daemon(&mut self) -> Result<()> {
        if let Some(mut child) = self.daemon.take() {
            info!(pid = child.id(), "Stopping preview daemon");
            let _ = child.kill();
            let status = child
                .wait()
                .context("Failed to wait for preview daemon exit")?;
            self.daemon_status = if status.success() {
                DaemonStatus::Stopped
            } else {
                DaemonStatus::Crashed(status.code())
            };
            self.status_message = Some(StatusMessage {
                text: "Preview daemon stopped".to_string(),
                color: STATUS_STOPPED,
            });
        }
        Ok(())
    }

    fn restart_daemon(&mut self) {
        info!("Restart requested from UI");
        if let Err(err) = self.stop_daemon().and_then(|_| self.start_daemon()) {
            error!(error = ?err, "Failed to restart daemon");
            self.status_message = Some(StatusMessage {
                text: format!("Restart failed: {err}"),
                color: STATUS_STOPPED,
            });
        }
    }

    fn reload_daemon_config(&mut self) {
        info!("Config reload requested - restarting daemon");
        self.restart_daemon();
    }

    fn save_config(&mut self) -> Result<()> {
        self.config.save().context("Failed to save configuration")?;
        self.settings_changed = false;
        self.status_message = Some(StatusMessage {
            text: "Configuration saved successfully".to_string(),
            color: STATUS_RUNNING,
        });
        info!("Configuration saved to disk");
        Ok(())
    }

    fn discard_changes(&mut self) {
        self.config = Config::load().unwrap_or_default();
        
        // Re-find selected profile index after reload
        self.selected_profile_idx = self.config.profiles
            .iter()
            .position(|p| p.name == self.config.manager.selected_profile)
            .unwrap_or(0);
        
        self.settings_changed = false;
        self.status_message = Some(StatusMessage {
            text: "Changes discarded".to_string(),
            color: STATUS_STOPPED,
        });
        info!("Configuration changes discarded");
    }

    fn poll_daemon(&mut self) {
        if self.last_health_check.elapsed() < Duration::from_millis(DAEMON_CHECK_INTERVAL_MS) {
            return;
        }
        self.last_health_check = Instant::now();

        if let Some(child) = self.daemon.as_mut() {
            match child.try_wait() {
                Ok(Some(status)) => {
                    warn!(pid = child.id(), exit = ?status.code(), "Preview daemon exited");
                    self.daemon = None;
                    self.daemon_status = if status.success() {
                        DaemonStatus::Stopped
                    } else {
                        DaemonStatus::Crashed(status.code())
                    };
                    self.status_message = Some(StatusMessage {
                        text: "Preview daemon exited".to_string(),
                        color: STATUS_STOPPED,
                    });
                }
                Ok(None) => {
                    if matches!(self.daemon_status, DaemonStatus::Starting) {
                        self.daemon_status = DaemonStatus::Running;
                        self.status_message = Some(StatusMessage {
                            text: "Preview daemon running".to_string(),
                            color: STATUS_RUNNING,
                        });
                    }
                }
                Err(err) => {
                    error!(error = ?err, "Failed to query daemon status");
                }
            }
        }
    }

    fn poll_tray_events(&mut self) {
        #[cfg(target_os = "linux")]
        while let Ok(cmd) = self.tray_rx.try_recv() {
            match cmd {
                TrayCommand::Reload => {
                    info!("Reload requested from tray menu");
                    self.reload_daemon_config();
                }
                TrayCommand::Quit => {
                    info!("Quit requested from tray menu");
                    self.should_quit = true;
                }
            }
        }
    }
}

impl eframe::App for ManagerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_daemon();
        self.poll_tray_events();

        // Handle quit request from tray menu
        if self.should_quit {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(PADDING);
            ui.heading("EVE-L-Preview Manager");
            ui.add_space(SECTION_SPACING);

            ui.group(|ui| {
                ui.label(egui::RichText::new("Daemon Status").strong());
                ui.colored_label(self.daemon_status.color(), self.daemon_status.label());
                if let Some(child) = &self.daemon {
                    ui.label(format!("PID: {}", child.id()));
                }
                if let Some(message) = &self.status_message {
                    ui.colored_label(message.color, &message.text);
                }
            });

            ui.add_space(SECTION_SPACING);
            ui.separator();
            ui.add_space(SECTION_SPACING);

            // Profile Selector
            let action = self.profile_selector.ui(
                ui,
                &mut self.config,
                &mut self.selected_profile_idx
            );
            
            match action {
                ProfileAction::SwitchProfile | ProfileAction::ProfileCreated | ProfileAction::ProfileDeleted => {
                    // Save config and reload daemon
                    if let Err(err) = self.save_config() {
                        error!(error = ?err, "Failed to save config after profile action");
                        self.status_message = Some(StatusMessage {
                            text: format!("Save failed: {err}"),
                            color: STATUS_STOPPED,
                        });
                    } else {
                        self.reload_daemon_config();
                    }
                }
                ProfileAction::None => {}
            }

            ui.add_space(SECTION_SPACING);
            ui.separator();
            ui.add_space(SECTION_SPACING);

            // Visual Settings for selected profile
            let current_profile = &mut self.config.profiles[self.selected_profile_idx];
            if components::visual_settings::ui(ui, current_profile) {
                self.settings_changed = true;
            }

            ui.add_space(SECTION_SPACING);
            ui.separator();
            ui.add_space(SECTION_SPACING);

            // Save/Discard buttons
            ui.horizontal(|ui| {
                if ui.button("💾 Save & Apply").clicked() {
                    if let Err(err) = self.save_config() {
                        error!(error = ?err, "Failed to save config");
                        self.status_message = Some(StatusMessage {
                            text: format!("Save failed: {err}"),
                            color: STATUS_STOPPED,
                        });
                    } else {
                        // Reload daemon to apply changes
                        self.reload_daemon_config();
                    }
                }
                
                if ui.button("↶ Discard Changes").clicked() {
                    self.discard_changes();
                }
                
                if self.settings_changed {
                    ui.colored_label(
                        egui::Color32::from_rgb(255, 200, 0),
                        "● Unsaved changes"
                    );
                }
            });

            ui.add_space(SECTION_SPACING);
            ui.separator();
            ui.add_space(SECTION_SPACING / 2.0);

            ui.group(|ui| {
                ui.label(egui::RichText::new("Tips").strong());
                ui.label("• Tab/Shift+Tab: Cycle characters");
                ui.label("• Right-click drag: Move thumbnails");
                ui.label("• Left-click: Focus EVE window");
            });
        });

        ctx.request_repaint_after(Duration::from_millis(DAEMON_CHECK_INTERVAL_MS));
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if let Err(err) = self.stop_daemon() {
            error!(error = ?err, "Failed to stop daemon during shutdown");
        }
        info!("Manager exiting");
    }
}

fn spawn_preview_daemon() -> Result<Child> {
    let exe_path = std::env::current_exe().context("Failed to resolve executable path")?;
    Command::new(exe_path)
        .arg("--preview")
        .spawn()
        .context("Failed to spawn preview daemon")
}

#[cfg(target_os = "linux")]
fn create_tray_icon(_tray_tx: Sender<TrayCommand>) -> Result<tray_icon::TrayIcon> {
    let icon = load_tray_icon()?;

    // Build menu with Reload and Quit options
    let menu = Menu::new();
    let status_item = MenuItem::new("EVE-L Preview Running", false, None);
    let reload_item = MenuItem::with_id(MenuId::new("reload"), "Reload", true, None);
    let quit_item = MenuItem::with_id(MenuId::new("quit"), "Quit", true, None);
    
    menu.append(&status_item)
        .context("Failed to append status menu item")?;
    menu.append(&PredefinedMenuItem::separator())
        .context("Failed to append separator")?;
    menu.append(&reload_item)
        .context("Failed to append reload menu item")?;
    menu.append(&quit_item)
        .context("Failed to append quit menu item")?;

    // Build tray icon
    let tray_icon = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("EVE-L Preview")
        .with_icon(icon)
        .build()
        .context("Failed to build tray icon")?;

    info!("Tray icon created");

    Ok(tray_icon)
}

#[cfg(target_os = "linux")]
fn load_tray_icon() -> Result<Icon> {
    let icon_bytes = include_bytes!("../../assets/tray-icon.png");
    let decoder = png::Decoder::new(Cursor::new(icon_bytes));
    let mut reader = decoder.read_info()?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf)?;
    let rgba = &buf[..info.buffer_size()];

    // tray-icon expects RGBA format directly
    let rgba_vec = match info.color_type {
        png::ColorType::Rgba => rgba.to_vec(),
        png::ColorType::Rgb => {
            // Convert RGB to RGBA
            let mut rgba_data = Vec::with_capacity(rgba.len() / 3 * 4);
            for chunk in rgba.chunks_exact(3) {
                rgba_data.extend_from_slice(chunk);
                rgba_data.push(0xFF); // Add full alpha
            }
            rgba_data
        }
        other => {
            return Err(anyhow!(
                "Unsupported tray icon color type {:?} (expected RGB or RGBA)",
                other
            ))
        }
    };

    Icon::from_rgba(rgba_vec, info.width, info.height)
        .context("Failed to create icon from RGBA data")
}

pub fn run_gui() -> Result<()> {
    let options = NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([WINDOW_WIDTH, WINDOW_HEIGHT])
            .with_min_inner_size([WINDOW_MIN_WIDTH, WINDOW_MIN_HEIGHT])
            .with_title("EVE-L Preview Manager"),
        ..Default::default()
    };

    eframe::run_native(
        "EVE-L Preview Manager",
        options,
        Box::new(|cc| Ok(Box::new(ManagerApp::new(cc)))),
    )
    .map_err(|err| anyhow!("Failed to launch egui manager: {err}"))
}
