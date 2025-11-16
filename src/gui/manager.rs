//! GUI manager implemented with egui/eframe and tray-icon system tray support

use std::io::Cursor;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use eframe::{egui, CreationContext, NativeOptions};
use tracing::{error, info, warn};
use tray_icon::{
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
    Icon, TrayIconBuilder,
};

use super::constants::*;



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
    quit_menu_id: tray_icon::menu::MenuId,
    daemon: Option<Child>,
    daemon_status: DaemonStatus,
    last_health_check: Instant,
    status_message: Option<StatusMessage>,
    allow_close: bool,
}

impl ManagerApp {
    fn new(_cc: &CreationContext<'_>) -> Self {
        info!("Initializing egui manager");

        // Spawn GTK thread for tray icon on Linux (egui uses winit, not GTK)
        // On Linux, we need a separate GTK event loop for the tray icon
        #[cfg(target_os = "linux")]
        let quit_menu_id = {
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                if let Err(err) = gtk::init() {
                    error!(error = ?err, "Failed to initialize GTK for tray icon");
                    return;
                }
                
                match create_tray_icon() {
                    Ok((tray_icon, quit_id)) => {
                        info!("Tray icon initialized in GTK thread");
                        let _ = tx.send(quit_id);
                        // Keep tray_icon alive by moving it into a leaked Box
                        // This prevents it from being dropped when the thread continues
                        Box::leak(Box::new(tray_icon));
                        gtk::main();
                    }
                    Err(err) => {
                        error!(error = ?err, "Failed to create tray icon in GTK thread");
                    }
                }
            });
            
            // Wait briefly for tray creation, or use dummy ID
            rx.recv_timeout(Duration::from_millis(500))
                .unwrap_or_else(|_| MenuItem::new("", true, None).id().clone())
        };

        #[cfg(not(target_os = "linux"))]
        let quit_menu_id = MenuItem::new("", true, None).id().clone();

        let mut app = Self {
            quit_menu_id,
            daemon: None,
            daemon_status: DaemonStatus::Stopped,
            last_health_check: Instant::now(),
            status_message: None,
            allow_close: false,
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

    fn process_tray_events(&mut self, ctx: &egui::Context) {
        if let Ok(MenuEvent { id }) = MenuEvent::receiver().try_recv() {
            if id == self.quit_menu_id {
                info!("Quit requested from tray menu");
                self.allow_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }
}

impl eframe::App for ManagerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.process_tray_events(ctx);
        self.poll_daemon();

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(PADDING);
            ui.heading("EVE-L Preview Manager");
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

            ui.horizontal(|ui| {
                if ui.button("\u{1F504} Restart Preview").clicked() {
                    self.restart_daemon();
                }
            });

            ui.add_space(SECTION_SPACING);
            ui.separator();
            ui.add_space(SECTION_SPACING);

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

fn create_tray_icon() -> Result<(tray_icon::TrayIcon, tray_icon::menu::MenuId)> {
    let icon = load_tray_icon()?;

    // Build simple menu with just Quit
    let menu = Menu::new();
    let quit_item = MenuItem::new("Quit", true, None);
    let quit_id = quit_item.id().clone();
    
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

    Ok((tray_icon, quit_id))
}

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
