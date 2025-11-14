use anyhow::Result;
use std::collections::HashMap;
use x11rb::connection::Connection;
use x11rb::protocol::damage::ConnectionExt as DamageExt;
use x11rb::protocol::Event::{self, CreateNotify, DamageNotify, DestroyNotify, PropertyNotify};
use x11rb::protocol::xproto::*;
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as WrapperExt;

use crate::config::{DisplayConfig, PersistentState};
use crate::persistence::SavedState;
use crate::snapping::{self, Rect};
use crate::thumbnail::Thumbnail;
use crate::x11_utils::{is_window_eve, AppContext, CachedAtoms};

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

    let dx = event.root_x - thumbnail.input_state.drag_start.0;
    let dy = event.root_y - thumbnail.input_state.drag_start.1;
    let new_x = thumbnail.input_state.win_start.0 + dx;
    let new_y = thumbnail.input_state.win_start.1 + dy;

    let dragged_rect = Rect {
        x: new_x,
        y: new_y,
        width: config_width,
        height: config_height,
    };

    let (final_x, final_y) = snapping::find_snap_position(
        dragged_rect,
        others,
        snap_threshold,
    ).unwrap_or((new_x, new_y));

    if final_x != thumbnail.x || final_y != thumbnail.y {
        thumbnail.reposition(final_x, final_y)?;
    }

    Ok(())
}

pub fn handle_event<'a>(
    ctx: &AppContext<'a>,
    persistent_state: &mut PersistentState,
    eves: &mut HashMap<Window, Thumbnail<'a>>,
    event: Event,
    session_state: &mut SavedState,
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
                eves.insert(event.window, thumbnail);
            }
        }
        DestroyNotify(event) => {
            eves.remove(&event.window);
        }
        PropertyNotify(event) => {
            if event.atom == ctx.atoms.wm_name
                && let Some(thumbnail) = eves.get_mut(&event.window)
                && let Some(new_character_name) = is_window_eve(ctx.conn, event.window, ctx.atoms)?
            {
                // Character name changed (login/logout/character switch)
                let old_name = thumbnail.character_name.clone();
                let current_pos = (thumbnail.x, thumbnail.y);
                
                // Ask persistent state what to do
                let new_position = persistent_state.handle_character_change(
                    &old_name,
                    &new_character_name,
                    current_pos,
                )?;
                
                // Update session state
                session_state.update_window_position(event.window, current_pos.0, current_pos.1);
                
                // Update thumbnail (may move to new position)
                thumbnail.set_character_name(new_character_name, new_position)?;
                
            } else if event.atom == ctx.atoms.wm_name
                && let Some(thumbnail) = check_and_create_window(ctx, persistent_state, event.window, session_state)?
            {
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
                    // Update session state
                    session_state.update_window_position(thumbnail.window, thumbnail.x, thumbnail.y);
                    // Persist character position
                    persistent_state.update_position(
                        &thumbnail.character_name,
                        thumbnail.x,
                        thumbnail.y,
                    )?;
                }
                
                thumbnail.input_state.dragging = false;
            }
        }
        Event::MotionNotify(event) => {
            // Build list of other thumbnails for snapping (immutable pass)
            let others: Vec<_> = eves
                .iter()
                .filter(|(_, t)| !t.input_state.dragging && t.visible)
                .map(|(win, t)| (*win, Rect {
                    x: t.x,
                    y: t.y,
                    width: ctx.config.width,
                    height: ctx.config.height,
                }))
                .collect();
            
            let snap_threshold = persistent_state.snap_threshold;
            
            // Handle drag for all thumbnails (mutable pass)
            for thumbnail in eves.values_mut() {
                handle_drag_motion(
                    thumbnail,
                    &event,
                    &others,
                    ctx.config.width,
                    ctx.config.height,
                    snap_threshold,
                )?;
            }
        }
        _ => (),
    }
    Ok(())
}
