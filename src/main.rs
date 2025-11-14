#![forbid(unsafe_code)]

mod config;
mod event_handler;
mod persistence;
mod thumbnail;
mod x11_utils;

use anyhow::Result;
use std::cell::RefCell;
use std::collections::HashMap;
use tracing::{error, info, warn, Level as TraceLevel};
use tracing_subscriber::FmtSubscriber;
use x11rb::connection::Connection;
use x11rb::protocol::damage::ConnectionExt as DamageExt;
use x11rb::protocol::xproto::*;
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as WrapperExt;

use config::Config;
use event_handler::handle_event;
use persistence::SavedState;
use thumbnail::Thumbnail;
use x11_utils::{is_window_eve, CachedAtoms};

fn check_and_create_window<'a>(
    conn: &'a RustConnection,
    screen: &Screen,
    config: &'a RefCell<Config>,
    window: Window,
    atoms: &CachedAtoms,
    state: &SavedState,
) -> Result<Option<Thumbnail<'a>>> {
    let pid_atom = conn.intern_atom(false, b"_NET_WM_PID")?.reply()?.atom;
    if let Ok(prop) = conn
        .get_property(false, window, pid_atom, AtomEnum::CARDINAL, 0, 1)?
        .reply()
    {
        if !prop.value.is_empty() {
            let pid = u32::from_ne_bytes(prop.value[0..4].try_into()?);
            if !std::fs::read_link(format!("/proc/{pid}/exe"))
                .map(|x| {
                    x.to_string_lossy().contains("wine64-preloader")
                        || x.to_string_lossy().contains("wine-preloader")
                })
                .inspect_err(|e| {
                    error!("cant read link '/proc/{pid}/exe' assuming its wine: err={e:?}")
                })
                .unwrap_or(true)
            {
                return Ok(None); // Return if we can determine that the window is not running through wine.
            }
        } else {
            warn!("_NET_WM_PID not set for window={window} assuming its wine");
        }
    }

    conn.change_window_attributes(
        window,
        &ChangeWindowAttributesAux::new().event_mask(EventMask::PROPERTY_CHANGE),
    )?;

    if let Some(character_name) = is_window_eve(conn, window, atoms)? {
        conn.change_window_attributes(
            window,
            &ChangeWindowAttributesAux::new()
                .event_mask(EventMask::PROPERTY_CHANGE | EventMask::FOCUS_CHANGE),
        )?;
        let font = conn.generate_id()?;
        conn.open_font(font, b"fixed")?;
        
        // Get saved position for this character/window
        let position = state.get_position(&character_name, window, &config.borrow());
        
        let thumbnail = Thumbnail::new(conn, screen, character_name, window, font, config, position)?;
        conn.close_font(font)?;
        info!("constructed Thumbnail for eve window: window={window}");
        Ok(Some(thumbnail))
    } else {
        Ok(None)
    }
}

fn get_eves<'a>(
    conn: &'a RustConnection,
    screen: &Screen,
    config: &'a RefCell<Config>,
    atoms: &CachedAtoms,
    state: &SavedState,
) -> Result<HashMap<Window, Thumbnail<'a>>> {
    let net_client_list = conn.intern_atom(false, b"_NET_CLIENT_LIST")?.reply()?.atom;
    let prop = conn
        .get_property(
            false,
            screen.root,
            net_client_list,
            AtomEnum::WINDOW,
            0,
            u32::MAX,
        )?
        .reply()?;
    let windows: Vec<u32> = prop
        .value32()
        .ok_or_else(|| anyhow::anyhow!("Invalid return from _NET_CLIENT_LIST"))?
        .collect();

    let mut eves = HashMap::new();
    for w in windows {
        if let Some(eve) = check_and_create_window(conn, screen, config, w, atoms, state)? {
            eves.insert(w, eve);
        }
    }
    conn.flush()?;
    Ok(eves)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let subscriber = FmtSubscriber::builder()
        .with_max_level(TraceLevel::INFO)
        .finish();

    tracing::subscriber::set_global_default(subscriber)?;

    let config = RefCell::new(Config::load());
    info!("config={:#?}", config.borrow());
    
    let mut state = SavedState::new();
    info!("loaded {} character positions from config", config.borrow().character_positions.len());

    let (conn, screen_num) = x11rb::connect(None)?;
    let screen = &conn.setup().roots[screen_num];
    
    // Pre-cache atoms once at startup (eliminates roundtrip overhead)
    let atoms = CachedAtoms::new(&conn)?;
    
    conn.damage_query_version(1, 1)?;
    conn.change_window_attributes(
        screen.root,
        &ChangeWindowAttributesAux::new().event_mask(
            EventMask::SUBSTRUCTURE_NOTIFY
                | EventMask::BUTTON_PRESS
                | EventMask::BUTTON_RELEASE
                | EventMask::POINTER_MOTION,
        ),
    )?;
    info!("successfully connected to x11: screen={screen_num}");

    let mut eves = get_eves(&conn, screen, &config, &atoms, &state)?;
    loop {
        let event = conn.wait_for_event()?;
        let _ = handle_event(&conn, screen, &config, &mut eves, event, &atoms, &mut state, check_and_create_window)
            .inspect_err(|err| error!("ecountered error in 'handle_event': err={err:#?}"));
    }
}
