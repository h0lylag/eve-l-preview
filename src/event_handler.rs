use anyhow::Result;
use std::collections::HashMap;
use x11rb::connection::Connection;
use x11rb::protocol::damage::ConnectionExt as DamageExt;
use x11rb::protocol::Event::{self, CreateNotify, DamageNotify, DestroyNotify, PropertyNotify};
use x11rb::protocol::xproto::*;

use crate::config::PersistentState;
use crate::constants::mouse;
use crate::cycle_state::CycleState;
use crate::persistence::SavedState;
use crate::snapping::{self, Rect};
use crate::thumbnail::Thumbnail;
use crate::types::Position;
use crate::x11_utils::{is_window_eve, AppContext};

/// Handle drag motion for a single thumbnail with snapping
fn handle_drag_motion(
    thumbnail: &mut Thumbnail,
    event: &MotionNotifyEvent,
    others: &[(Window, Rect)],
    config_width: u16,
    config_height: u16,
    snap_threshold: u16,
) -> Result<()> {
    if !thumbnail.input_state.dragging {
        return Ok(());
    }

    let dx = event.root_x - thumbnail.input_state.drag_start.x;
    let dy = event.root_y - thumbnail.input_state.drag_start.y;
    let new_x = thumbnail.input_state.win_start.x + dx;
    let new_y = thumbnail.input_state.win_start.y + dy;

    let dragged_rect = Rect {
        x: new_x,
        y: new_y,
        width: config_width,
        height: config_height,
    };

    let Position { x: final_x, y: final_y } = snapping::find_snap_position(
        dragged_rect,
        others,
        snap_threshold,
    ).unwrap_or_else(|| Position::new(new_x, new_y));

    // Always reposition (let X11 handle no-op if position unchanged)
    thumbnail.reposition(final_x, final_y)?;

    Ok(())
}

pub fn handle_event<'a>(
    ctx: &AppContext<'a>,
    persistent_state: &mut PersistentState,
    eves: &mut HashMap<Window, Thumbnail<'a>>,
    event: Event,
    session_state: &mut SavedState,
    cycle_state: &mut CycleState,
    check_and_create_window: impl Fn(&AppContext<'a>, &PersistentState, Window, &SavedState) -> Result<Option<Thumbnail<'a>>>,
) -> Result<()> {
    match event {
        DamageNotify(event) => {
            if let Some(thumbnail) = eves
                .values()
                .find(|thumbnail| thumbnail.damage == event.damage)
            {
                thumbnail.update()?; // TODO: add fps limiter?
                ctx.conn.damage_subtract(event.damage, 0u32, 0u32)?;
                ctx.conn.flush()?;
            }
        }
        CreateNotify(event) => {
            if let Some(thumbnail) = check_and_create_window(ctx, persistent_state, event.window, session_state)? {
                // Register with cycle state
                cycle_state.add_window(thumbnail.character_name.clone(), event.window);
                eves.insert(event.window, thumbnail);
            }
        }
        DestroyNotify(event) => {
            cycle_state.remove_window(event.window);
            eves.remove(&event.window);
        }
        PropertyNotify(event) => {
            if event.atom == ctx.atoms.wm_name
                && let Some(thumbnail) = eves.get_mut(&event.window)
                && let Some(new_character_name) = is_window_eve(ctx.conn, event.window, ctx.atoms)?
            {
                // Character name changed (login/logout/character switch)
                let old_name = thumbnail.character_name.clone();
                
                // Query actual position from X11
                let geom = ctx.conn.get_geometry(thumbnail.window)?.reply()?;
                let current_pos = Position::new(geom.x, geom.y);
                
                // Update cycle state with new character name
                cycle_state.update_character(event.window, new_character_name.clone());
                
                // Ask persistent state what to do - pass current dimensions to ensure they're saved
                let new_position = persistent_state.handle_character_change(
                    &old_name,
                    &new_character_name,
                    current_pos,
                    thumbnail.width,
                    thumbnail.height,
                )?;
                
                // Update session state
                session_state.update_window_position(event.window, current_pos.x, current_pos.y);
                
                // Update thumbnail (may move to new position)
                thumbnail.set_character_name(new_character_name, new_position)?;
                
            } else if event.atom == ctx.atoms.wm_name
                && let Some(thumbnail) = check_and_create_window(ctx, persistent_state, event.window, session_state)?
            {
                // New EVE window detected
                cycle_state.add_window(thumbnail.character_name.clone(), event.window);
                eves.insert(event.window, thumbnail);
            } else if event.atom == ctx.atoms.net_wm_state
                && let Some(thumbnail) = eves.get_mut(&event.window)
                && let Some(state) = ctx.conn
                    .get_property(false, event.window, event.atom, AtomEnum::ATOM, 0, 1024)?
                    .reply()?
                    .value32()
                && state.collect::<Vec<_>>().contains(&ctx.atoms.net_wm_state_hidden)
            {
                thumbnail.minimized()?;
            }
        }
        Event::FocusIn(event) => {
            if let Some(thumbnail) = eves.get_mut(&event.event) {
                thumbnail.minimized = false;
                thumbnail.focused = true;
                thumbnail.border(true)?;
                if ctx.config.hide_when_no_focus && eves.values().any(|x| !x.visible) {
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
                if ctx.config.hide_when_no_focus && eves.values().all(|x| !x.focused && !x.minimized) {
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
                let geom = ctx.conn.get_geometry(thumbnail.window)?.reply()?;
                thumbnail.input_state.drag_start = Position::new(event.root_x, event.root_y);
                thumbnail.input_state.win_start = Position::new(geom.x, geom.y);
                // Only allow dragging with right-click (button 3)
                if event.detail == mouse::BUTTON_RIGHT {
                    thumbnail.input_state.dragging = true;
                }
                // Left-click sets current character for cycling
                if event.detail == mouse::BUTTON_LEFT {
                    cycle_state.set_current(&thumbnail.character_name);
                }
            }
        }
        Event::ButtonRelease(event) => {
            if let Some((_, thumbnail)) = eves
                .iter_mut()
                .find(|(_, thumb)| thumb.is_hovered(event.root_x, event.root_y))
            {
                // Left-click focuses the window
                // (dragging is only enabled for right-click, so left-click never drags)
                if event.detail == mouse::BUTTON_LEFT {
                    thumbnail.focus()?;
                }
                
                // Save position after drag ends (right-click release)
                if thumbnail.input_state.dragging {
                    // Query actual position from X11
                    let geom = ctx.conn.get_geometry(thumbnail.window)?.reply()?;
                    
                    // Update session state
                    session_state.update_window_position(thumbnail.window, geom.x, geom.y);
                    // Persist character position AND dimensions
                    persistent_state.update_position(
                        &thumbnail.character_name,
                        geom.x,
                        geom.y,
                        thumbnail.width,
                        thumbnail.height,
                    )?;
                }
                
                thumbnail.input_state.dragging = false;
            }
        }
        Event::MotionNotify(event) => {
            // Build list of other thumbnails for snapping (query actual positions)
            let others: Vec<_> = eves
                .iter()
                .filter(|(_, t)| !t.input_state.dragging && t.visible)
                .filter_map(|(win, t)| {
                    ctx.conn.get_geometry(t.window).ok()
                        .and_then(|req| req.reply().ok())
                        .map(|geom| (*win, Rect {
                            x: geom.x,
                            y: geom.y,
                            width: t.width,
                            height: t.height,
                        }))
                })
                .collect();
            
            let snap_threshold = persistent_state.global.snap_threshold;
            
            // Handle drag for all thumbnails (mutable pass)
            for thumbnail in eves.values_mut() {
                handle_drag_motion(
                    thumbnail,
                    &event,
                    &others,
                    thumbnail.width,
                    thumbnail.height,
                    snap_threshold,
                )?;
            }
        }
        _ => (),
    }
    Ok(())
}
