use anyhow::{Context, Result};
use tracing::debug;
use x11rb::connection::Connection;
use x11rb::protocol::render::{ConnectionExt as RenderExt, Fixed, Pictformat};
use x11rb::protocol::xproto::*;
use x11rb::rust_connection::RustConnection;

use crate::config::DisplayConfig;
use crate::constants::{eve, fixed_point, x11};
use crate::font::FontRenderer;
use crate::types::EveWindowType;

/// Application context holding immutable shared state
pub struct AppContext<'a> {
    pub conn: &'a RustConnection,
    pub screen: &'a Screen,
    pub config: &'a DisplayConfig,
    pub atoms: &'a CachedAtoms,
    pub font_renderer: &'a FontRenderer,
}

/// Pre-cached X11 atoms to avoid repeated roundtrips
pub struct CachedAtoms {
    pub wm_name: Atom,
    pub net_wm_pid: Atom,
    pub net_wm_state: Atom,
    pub net_wm_state_hidden: Atom,
    pub net_active_window: Atom,
}

impl CachedAtoms {
    pub fn new(conn: &RustConnection) -> Result<Self> {
        // Do all intern_atom roundtrips once at startup
        Ok(Self {
            wm_name: conn.intern_atom(false, b"WM_NAME")
                .context("Failed to intern WM_NAME atom")?
                .reply()
                .context("Failed to get reply for WM_NAME atom")?
                .atom,
            net_wm_pid: conn.intern_atom(false, b"_NET_WM_PID")
                .context("Failed to intern _NET_WM_PID atom")?
                .reply()
                .context("Failed to get reply for _NET_WM_PID atom")?
                .atom,
            net_wm_state: conn.intern_atom(false, b"_NET_WM_STATE")
                .context("Failed to intern _NET_WM_STATE atom")?
                .reply()
                .context("Failed to get reply for _NET_WM_STATE atom")?
                .atom,
            net_wm_state_hidden: conn.intern_atom(false, b"_NET_WM_STATE_HIDDEN")
                .context("Failed to intern _NET_WM_STATE_HIDDEN atom")?
                .reply()
                .context("Failed to get reply for _NET_WM_STATE_HIDDEN atom")?
                .atom,
            net_active_window: conn.intern_atom(false, b"_NET_ACTIVE_WINDOW")
                .context("Failed to intern _NET_ACTIVE_WINDOW atom")?
                .reply()
                .context("Failed to get reply for _NET_ACTIVE_WINDOW atom")?
                .atom,
        })
    }
}

pub fn to_fixed(v: f32) -> Fixed {
    (v * fixed_point::MULTIPLIER).round() as Fixed
}

#[tracing::instrument]
pub fn get_pictformat(conn: &RustConnection, depth: u8, alpha: bool) -> Result<Pictformat> {
    if let Some(format) = conn
        .render_query_pict_formats()
        .context("Failed to query RENDER picture formats")?
        .reply()
        .context("Failed to get reply for RENDER picture formats query")?
        .formats
        .iter()
        .find(|format| {
            debug!(
                "discovered Pictformat: {}, {}",
                format.depth, format.direct.alpha_mask
            );
            format.depth == depth
                && if alpha {
                    format.direct.alpha_mask != 0
                } else {
                    format.direct.alpha_mask == 0
                }
        })
    {
        debug!(
            "using Pictformat: {}, {}",
            format.depth, format.direct.alpha_mask
        );
        Ok(format.id)
    } else {
        anyhow::bail!("Could not find suitable picture format (depth={}, alpha={}). Check RENDER extension support.", depth, alpha)
    }
}

pub fn is_window_eve(conn: &RustConnection, window: Window, atoms: &CachedAtoms) -> Result<Option<EveWindowType>> {
    let name_prop = conn
        .get_property(false, window, atoms.wm_name, AtomEnum::STRING, 0, 1024)
        .context(format!("Failed to query WM_NAME property for window {}", window))?
        .reply()
        .context(format!("Failed to get WM_NAME reply for window {}", window))?;
    let title = String::from_utf8_lossy(&name_prop.value).into_owned();
    Ok(if let Some(name) = title.strip_prefix(eve::WINDOW_TITLE_PREFIX) {
        Some(EveWindowType::LoggedIn(name.to_string()))
    } else if title == eve::LOGGED_OUT_TITLE {
        Some(EveWindowType::LoggedOut)
    } else {
        None
    })
}

/// Check if the currently focused window is an EVE client
pub fn is_eve_window_focused(conn: &RustConnection, screen: &Screen, atoms: &CachedAtoms) -> Result<bool> {
    // Get the currently active window
    let active_window_prop = conn
        .get_property(
            false,
            screen.root,
            atoms.net_active_window,
            AtomEnum::WINDOW,
            0,
            1,
        )
        .context("Failed to query _NET_ACTIVE_WINDOW property")?
        .reply()
        .context("Failed to get reply for _NET_ACTIVE_WINDOW query")?;
    
    if active_window_prop.value.len() >= 4 {
        let active_window = u32::from_ne_bytes(active_window_prop.value[0..4].try_into()
            .context("Invalid _NET_ACTIVE_WINDOW property format")?);
        // Check if this window is an EVE client
        Ok(is_window_eve(conn, active_window, atoms)
            .context(format!("Failed to check if active window {} is EVE client", active_window))?.is_some())
    } else {
        Ok(false)
    }
}

/// Activate (focus) an X11 window using _NET_ACTIVE_WINDOW
pub fn activate_window(
    conn: &RustConnection,
    screen: &Screen,
    atoms: &CachedAtoms,
    window: Window,
) -> Result<()> {
    use x11rb::protocol::xproto::*;

    // First, raise the window to top of stack
    conn.configure_window(
        window,
        &ConfigureWindowAux::new().stack_mode(StackMode::ABOVE),
    )
    .context(format!("Failed to raise window {} to top of stack", window))?;

    // Send _NET_ACTIVE_WINDOW client message to root window
    let event = ClientMessageEvent {
        response_type: CLIENT_MESSAGE_EVENT,
        format: 32,
        sequence: 0,
        window,
        type_: atoms.net_active_window,
        data: ClientMessageData::from([
            x11::ACTIVE_WINDOW_SOURCE_PAGER, // Source indication: 2 = pager/direct user action
            x11rb::CURRENT_TIME, // Timestamp (current time)
            0, // Requestor's currently active window (0 = none)
            0,
            0,
        ]),
    };

    conn.send_event(
        false,
        screen.root,
        EventMask::SUBSTRUCTURE_NOTIFY | EventMask::SUBSTRUCTURE_REDIRECT,
        &event,
    )
    .context(format!("Failed to send _NET_ACTIVE_WINDOW event for window {}", window))?;

    conn.flush()
        .context("Failed to flush X11 connection after window activation")?;
    Ok(())
}
