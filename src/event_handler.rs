use anyhow::Result;
use std::cell::RefCell;
use std::collections::HashMap;
use x11rb::connection::Connection;
use x11rb::protocol::damage::ConnectionExt as DamageExt;
use x11rb::protocol::Event::{self, CreateNotify, DamageNotify, DestroyNotify, PropertyNotify};
use x11rb::protocol::xproto::*;
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as WrapperExt;

use crate::config::Config;
use crate::persistence::SavedState;
use crate::snapping;
use crate::thumbnail::Thumbnail;
use crate::x11_utils::{is_window_eve, CachedAtoms};

pub fn handle_event<'a>(
    conn: &'a RustConnection,
    screen: &Screen,
    config: &'a RefCell<Config>,
    eves: &mut HashMap<Window, Thumbnail<'a>>,
    event: Event,
    atoms: &CachedAtoms,
    state: &mut SavedState,
    check_and_create_window: impl Fn(&'a RustConnection, &Screen, &'a RefCell<Config>, Window, &CachedAtoms, &SavedState) -> Result<Option<Thumbnail<'a>>>,
) -> Result<()> {
    match event {
        DamageNotify(event) => {
            if let Some(thumbnail) = eves
                .values()
                .find(|thumbnail| thumbnail.damage == event.damage)
            {
                thumbnail.update()?; // TODO: add fps limiter?
                conn.damage_subtract(event.damage, 0u32, 0u32)?;
                conn.flush()?;
            }
        }
        CreateNotify(event) => {
            if let Some(thumbnail) = check_and_create_window(conn, screen, config, event.window, atoms, state)? {
                eves.insert(event.window, thumbnail);
            }
        }
        DestroyNotify(event) => {
            eves.remove(&event.window);
        }
        PropertyNotify(event) => {
            if event.atom == atoms.wm_name
                && let Some(thumbnail) = eves.get_mut(&event.window)
                && let Some(new_character_name) = is_window_eve(conn, event.window, atoms)?
            {
                // Character name changed (login/logout/character switch)
                let old_name = thumbnail.character_name.clone();
                let current_pos = (thumbnail.x, thumbnail.y);
                
                // Ask state manager what to do
                let new_position = state.handle_character_change(
                    event.window,
                    &old_name,
                    &new_character_name,
                    current_pos,
                    &mut config.borrow_mut(),
                )?;
                
                // Update thumbnail (may move to new position)
                thumbnail.set_character_name(new_character_name, new_position)?;
                
            } else if event.atom == atoms.wm_name
                && let Some(thumbnail) = check_and_create_window(conn, screen, config, event.window, atoms, state)?
            {
                eves.insert(event.window, thumbnail);
            } else if event.atom == atoms.net_wm_state
                && let Some(thumbnail) = eves.get_mut(&event.window)
                && let Some(state) = conn
                    .get_property(false, event.window, event.atom, AtomEnum::ATOM, 0, 1024)?
                    .reply()?
                    .value32()
                && state.collect::<Vec<_>>().contains(&atoms.net_wm_state_hidden)
            {
                thumbnail.minimized()?;
            }
        }
        Event::FocusIn(event) => {
            if let Some(thumbnail) = eves.get_mut(&event.event) {
                thumbnail.minimized = false;
                thumbnail.focused = true;
                thumbnail.border(true)?;
                if config.borrow().hide_when_no_focus && eves.values().any(|x| !x.visible) {
                    for thumbnail in eves.values_mut() {
                        thumbnail.visibility(true)?;
                    }
                }
            }
        }
        Event::FocusOut(event) => {
            if let Some(thumbnail) = eves.get_mut(&event.event) {
                thumbnail.focused = false;
                thumbnail.border(false)?;
                if config.borrow().hide_when_no_focus && eves.values().all(|x| !x.focused && !x.minimized) {
                    for thumbnail in eves.values_mut() {
                        thumbnail.visibility(false)?;
                    }
                }
            }
        }
        Event::ButtonPress(event) => {
            if let Some((_, thumbnail)) = eves
                .iter_mut()
                .find(|(_, thumb)| thumb.is_hovered(event.root_x, event.root_y) && thumb.visible)
            {
                let geom = conn.get_geometry(thumbnail.window)?.reply()?;
                thumbnail.input_state.drag_start = (event.root_x, event.root_y);
                thumbnail.input_state.win_start = (geom.x, geom.y);
                // Only allow dragging with right-click (button 3)
                if event.detail == 3 {
                    thumbnail.input_state.dragging = true;
                }
            }
        }
        Event::ButtonRelease(event) => {
            if let Some((_, thumbnail)) = eves
                .iter_mut()
                .find(|(_, thumb)| thumb.is_hovered(event.root_x, event.root_y))
            {
                // Left-click focuses the window (only if it wasn't dragged)
                if event.detail == 1
                    && thumbnail.input_state.drag_start == (event.root_x, event.root_y)
                {
                    thumbnail.focus()?;
                }
                
                // Save position after drag ends (right-click release)
                if thumbnail.input_state.dragging {
                    state.update_position(
                        &thumbnail.character_name,
                        thumbnail.window,
                        thumbnail.x,
                        thumbnail.y,
                        &mut config.borrow_mut(),
                    )?;
                }
                
                thumbnail.input_state.dragging = false;
            }
        }
        Event::MotionNotify(event) => {
            // First, find which window is being dragged and calculate new position
            let drag_info = eves.iter().find_map(|(win, thumb)| {
                if thumb.input_state.dragging && thumb.is_hovered(event.root_x, event.root_y) {
                    let dx = event.root_x - thumb.input_state.drag_start.0;
                    let dy = event.root_y - thumb.input_state.drag_start.1;
                    let new_x = thumb.input_state.win_start.0 + dx;
                    let new_y = thumb.input_state.win_start.1 + dy;
                    Some((*win, new_x, new_y))
                } else {
                    None
                }
            });
            
            if let Some((dragged_window, new_x, new_y)) = drag_info {
                // Build rect for dragged thumbnail
                let cfg = config.borrow();
                let dragged_rect = snapping::Rect {
                    x: new_x,
                    y: new_y,
                    width: cfg.width,
                    height: cfg.height,
                };
                
                // Build list of other thumbnails for snapping
                let others: Vec<_> = eves
                    .iter()
                    .filter(|(win, t)| **win != dragged_window && t.visible)
                    .map(|(win, t)| (*win, snapping::Rect {
                        x: t.x,
                        y: t.y,
                        width: cfg.width,
                        height: cfg.height,
                    }))
                    .collect();
                
                let snap_threshold = cfg.snap_threshold;
                drop(cfg); // Release borrow before repositioning
                
                // Find snap position
                let (final_x, final_y) = snapping::find_snap_position(
                    dragged_rect,
                    &others,
                    snap_threshold,
                ).unwrap_or((new_x, new_y));
                
                // Now reposition the dragged thumbnail
                if let Some(thumbnail) = eves.get_mut(&dragged_window) {
                    thumbnail.reposition(final_x, final_y)?;
                }
            }
        }
        _ => (),
    }
    Ok(())
}
