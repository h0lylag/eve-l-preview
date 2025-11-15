use anyhow::Result;
use tracing::debug;
use x11rb::connection::Connection;
use x11rb::protocol::render::{ConnectionExt as RenderExt, Fixed, Pictformat};
use x11rb::protocol::xproto::*;
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as WrapperExt;

use crate::config::DisplayConfig;
use crate::font::FontRenderer;

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
            wm_name: conn.intern_atom(false, b"WM_NAME")?.reply()?.atom,
            net_wm_pid: conn.intern_atom(false, b"_NET_WM_PID")?.reply()?.atom,
            net_wm_state: conn.intern_atom(false, b"_NET_WM_STATE")?.reply()?.atom,
            net_wm_state_hidden: conn.intern_atom(false, b"_NET_WM_STATE_HIDDEN")?.reply()?.atom,
            net_active_window: conn.intern_atom(false, b"_NET_ACTIVE_WINDOW")?.reply()?.atom,
        })
    }
}

pub fn to_fixed(v: f32) -> Fixed {
    (v * (1 << 16) as f32).round() as Fixed
}

#[tracing::instrument]
pub fn get_pictformat(conn: &RustConnection, depth: u8, alpha: bool) -> Result<Pictformat> {
    if let Some(format) = conn
        .render_query_pict_formats()?
        .reply()?
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
        anyhow::bail!("could not find suitable Pictformat")
    }
}

pub fn is_window_eve(conn: &RustConnection, window: Window, atoms: &CachedAtoms) -> Result<Option<String>> {
    let name_prop = conn
        .get_property(false, window, atoms.wm_name, AtomEnum::STRING, 0, 1024)?
        .reply()?;
    let title = String::from_utf8_lossy(&name_prop.value).into_owned();
    Ok(if let Some(name) = title.strip_prefix("EVE - ") {
        Some(name.to_string())
    } else if title == "EVE" {
        Some(String::new())
    } else {
        None
    })
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
    )?;

    // Send _NET_ACTIVE_WINDOW client message to root window
    let event = ClientMessageEvent {
        response_type: CLIENT_MESSAGE_EVENT,
        format: 32,
        sequence: 0,
        window,
        type_: atoms.net_active_window,
        data: ClientMessageData::from([
            2, // Source indication: 2 = pager/direct user action (stronger than 1=application)
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
    )?;

    conn.flush()?;
    Ok(())
}
