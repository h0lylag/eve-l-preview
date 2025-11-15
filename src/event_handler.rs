use anyhow::{Context, Result};
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
use crate::types::{Position, ThumbnailState};
use crate::x11_utils::{is_window_eve, AppContext};

/// Handle DamageNotify events - update damaged thumbnail
fn handle_damage_notify(ctx: &AppContext, eves: &HashMap<Window, Thumbnail>, event: x11rb::protocol::damage::NotifyEvent) -> Result<()> {
    if let Some(thumbnail) = eves
        .values()
        .find(|thumbnail| thumbnail.damage == event.damage)
    {
        thumbnail.update()
            .context(format!("Failed to update thumbnail for damage event (damage={})", event.damage))?;
        ctx.conn.damage_subtract(event.damage, 0u32, 0u32)
            .context(format!("Failed to subtract damage region (damage={})", event.damage))?;
        ctx.conn.flush()
            .context("Failed to flush X11 connection after damage update")?;
    }
    Ok(())
}

/// Handle CreateNotify events - create thumbnail for new EVE window
fn handle_create_notify<'a>(
    ctx: &AppContext<'a>,
    persistent_state: &PersistentState,
    eves: &mut HashMap<Window, Thumbnail<'a>>,
    event: CreateNotifyEvent,
    session_state: &SavedState,
    cycle_state: &mut CycleState,
    check_and_create_window: &impl Fn(&AppContext<'a>, &PersistentState, Window, &SavedState) -> Result<Option<Thumbnail<'a>>>,
) -> Result<()> {
    if let Some(thumbnail) = check_and_create_window(ctx, persistent_state, event.window, session_state)
        .context(format!("Failed to check/create window for new window {}", event.window))? {
        // Register with cycle state
        cycle_state.add_window(thumbnail.character_name.clone(), event.window);
        eves.insert(event.window, thumbnail);
    }
    Ok(())
}

/// Handle DestroyNotify events - remove destroyed window
fn handle_destroy_notify(
    eves: &mut HashMap<Window, Thumbnail>,
    event: DestroyNotifyEvent,
    cycle_state: &mut CycleState,
) -> Result<()> {
    cycle_state.remove_window(event.window);
    eves.remove(&event.window);
    Ok(())
}

/// Handle FocusIn events - update focused state and visibility
fn handle_focus_in(
    ctx: &AppContext,
    eves: &mut HashMap<Window, Thumbnail>,
    event: FocusInEvent,
) -> Result<()> {
    if let Some(thumbnail) = eves.get_mut(&event.event) {
        // Transition to focused normal state (from minimized or unfocused)
        thumbnail.state = ThumbnailState::Normal { focused: true };
        thumbnail.border(true)
            .context(format!("Failed to update border on focus for '{}'", thumbnail.character_name))?;
        if ctx.config.hide_when_no_focus && eves.values().any(|x| !x.state.is_visible()) {
            for thumbnail in eves.values_mut() {
                thumbnail.visibility(true)
                    .context(format!("Failed to show thumbnail '{}' on focus", thumbnail.character_name))?;
            }
        }
    }
    Ok(())
}

/// Handle FocusOut events - update focused state and visibility  
fn handle_focus_out(
    ctx: &AppContext,
    eves: &mut HashMap<Window, Thumbnail>,
    event: FocusOutEvent,
) -> Result<()> {
    if let Some(thumbnail) = eves.get_mut(&event.event) {
        // Transition to unfocused normal state
        thumbnail.state = ThumbnailState::Normal { focused: false };
        thumbnail.border(false)
            .context(format!("Failed to clear border on focus loss for '{}'", thumbnail.character_name))?;
        if ctx.config.hide_when_no_focus && eves.values().all(|x| !x.state.is_focused() && !x.state.is_minimized()) {
            for thumbnail in eves.values_mut() {
                thumbnail.visibility(false)
                    .context(format!("Failed to hide thumbnail '{}' on focus loss", thumbnail.character_name))?;
            }
        }
    }
    Ok(())
}

/// Handle ButtonPress events - start dragging or set current character
fn handle_button_press(
    ctx: &AppContext,
    eves: &mut HashMap<Window, Thumbnail>,
    event: ButtonPressEvent,
    cycle_state: &mut CycleState,
) -> Result<()> {
    if let Some((_, thumbnail)) = eves
        .iter_mut()
        .find(|(_, thumb)| thumb.is_hovered(event.root_x, event.root_y) && thumb.state.is_visible())
    {
        let geom = ctx.conn.get_geometry(thumbnail.window)
            .context("Failed to send geometry query on button press")?
            .reply()
            .context(format!("Failed to get geometry on button press for '{}'", thumbnail.character_name))?;
        thumbnail.input_state.drag_start = Position::new(event.root_x, event.root_y);
        thumbnail.input_state.win_start = Position::new(geom.x, geom.y);
        // Only allow dragging with right-click
        if event.detail == mouse::BUTTON_RIGHT {
            thumbnail.input_state.dragging = true;
        }
        // Left-click sets current character for cycling
        if event.detail == mouse::BUTTON_LEFT {
            cycle_state.set_current(&thumbnail.character_name);
        }
    }
    Ok(())
}

/// Handle ButtonRelease events - focus window and save position after drag
fn handle_button_release(
    ctx: &AppContext,
    persistent_state: &mut PersistentState,
    eves: &mut HashMap<Window, Thumbnail>,
    event: ButtonReleaseEvent,
    session_state: &mut SavedState,
) -> Result<()> {
    if let Some((_, thumbnail)) = eves
        .iter_mut()
        .find(|(_, thumb)| thumb.is_hovered(event.root_x, event.root_y))
    {
        // Left-click focuses the window
        // (dragging is only enabled for right-click, so left-click never drags)
        if event.detail == mouse::BUTTON_LEFT {
            thumbnail.focus()
                .context(format!("Failed to focus window for '{}'", thumbnail.character_name))?;
        }
        
        // Save position after drag ends (right-click release)
        if thumbnail.input_state.dragging {
            // Query actual position from X11
            let geom = ctx.conn.get_geometry(thumbnail.window)
                .context("Failed to send geometry query after drag")?
                .reply()
                .context(format!("Failed to get geometry after drag for '{}'", thumbnail.character_name))?;
            
            // Update session state
            session_state.update_window_position(thumbnail.window, geom.x, geom.y);
            // Persist character position AND dimensions
            persistent_state.update_position(
                &thumbnail.character_name,
                geom.x,
                geom.y,
                thumbnail.dimensions.width,
                thumbnail.dimensions.height,
            )
            .context(format!("Failed to save position for '{}' after drag", thumbnail.character_name))?;
        }
        
        thumbnail.input_state.dragging = false;
    }
    Ok(())
}

/// Handle MotionNotify events - process drag motion with snapping
fn handle_motion_notify(
    ctx: &AppContext,
    persistent_state: &PersistentState,
    eves: &mut HashMap<Window, Thumbnail>,
    event: MotionNotifyEvent,
) -> Result<()> {
    // Build list of other thumbnails for snapping (query actual positions)
    let others: Vec<_> = eves
        .iter()
        .filter(|(_, t)| !t.input_state.dragging && t.state.is_visible())
        .filter_map(|(win, t)| {
            ctx.conn.get_geometry(t.window).ok()
                .and_then(|req| req.reply().ok())
                .map(|geom| (*win, Rect {
                    x: geom.x,
                    y: geom.y,
                    width: t.dimensions.width,
                    height: t.dimensions.height,
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
            thumbnail.dimensions.width,
            thumbnail.dimensions.height,
            snap_threshold,
        )
        .context(format!("Failed to handle drag motion for '{}'", thumbnail.character_name))?;
    }
    Ok(())
}

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
        DamageNotify(event) => handle_damage_notify(ctx, eves, event),
        CreateNotify(event) => handle_create_notify(ctx, persistent_state, eves, event, session_state, cycle_state, &check_and_create_window),
        DestroyNotify(event) => handle_destroy_notify(eves, event, cycle_state),
        Event::FocusIn(event) => handle_focus_in(ctx, eves, event),
        Event::FocusOut(event) => handle_focus_out(ctx, eves, event),
        Event::ButtonPress(event) => handle_button_press(ctx, eves, event, cycle_state),
        Event::ButtonRelease(event) => handle_button_release(ctx, persistent_state, eves, event, session_state),
        Event::MotionNotify(event) => handle_motion_notify(ctx, persistent_state, eves, event),
        PropertyNotify(event) => {
            if event.atom == ctx.atoms.wm_name
                && let Some(thumbnail) = eves.get_mut(&event.window)
                && let Some(eve_window) = is_window_eve(ctx.conn, event.window, ctx.atoms)
                    .context(format!("Failed to check if window {} is EVE client during property change", event.window))?
            {
                // Character name changed (login/logout/character switch)
                let old_name = thumbnail.character_name.clone();
                let new_character_name = eve_window.character_name();
                
                // Query actual position from X11
                let geom = ctx.conn.get_geometry(thumbnail.window)
                    .context("Failed to send geometry query during character change")?
                    .reply()
                    .context(format!("Failed to get geometry during character change for window {}", thumbnail.window))?;
                let current_pos = Position::new(geom.x, geom.y);
                
                // Update cycle state with new character name
                cycle_state.update_character(event.window, new_character_name.to_string());
                
                // Ask persistent state what to do - pass current dimensions to ensure they're saved
                let new_position = persistent_state.handle_character_change(
                    &old_name,
                    &new_character_name,
                    current_pos,
                    thumbnail.dimensions.width,
                    thumbnail.dimensions.height,
                )
                .context(format!("Failed to handle character change from '{}' to '{}'", old_name, new_character_name))?;
                
                // Update session state
                session_state.update_window_position(event.window, current_pos.x, current_pos.y);
                
                // Update thumbnail (may move to new position)
                thumbnail.set_character_name(new_character_name.to_string(), new_position)
                    .context(format!("Failed to update thumbnail after character change from '{}'", old_name))?;
                
            } else if event.atom == ctx.atoms.wm_name
                && let Some(thumbnail) = check_and_create_window(ctx, persistent_state, event.window, session_state)
                    .context(format!("Failed to create thumbnail for newly detected EVE window {}", event.window))?
            {
                // New EVE window detected
                cycle_state.add_window(thumbnail.character_name.clone(), event.window);
                eves.insert(event.window, thumbnail);
            } else if event.atom == ctx.atoms.net_wm_state
                && let Some(thumbnail) = eves.get_mut(&event.window)
                && let Some(state) = ctx.conn
                    .get_property(false, event.window, event.atom, AtomEnum::ATOM, 0, 1024)
                    .context(format!("Failed to query window state for window {}", event.window))?
                    .reply()
                    .context(format!("Failed to get window state reply for window {}", event.window))?
                    .value32()
                && state.collect::<Vec<_>>().contains(&ctx.atoms.net_wm_state_hidden)
            {
                thumbnail.minimized()
                    .context(format!("Failed to set minimized state for '{}'", thumbnail.character_name))?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}
