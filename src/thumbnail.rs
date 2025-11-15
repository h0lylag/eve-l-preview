use anyhow::Result;
use tracing::{error, info};
use x11rb::connection::Connection;
use x11rb::protocol::damage::{
    ConnectionExt as DamageExt, Damage, ReportLevel as DamageReportLevel,
};
use x11rb::protocol::render::{
    ConnectionExt as RenderExt, CreatePictureAux, Fixed, PictOp, Picture, Transform,
};
use x11rb::protocol::xproto::*;
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as WrapperExt;

use crate::config::DisplayConfig;
use crate::constants::{mouse, positioning, x11};
use crate::font::FontRenderer;
use crate::types::Position;
use crate::x11_utils::{get_pictformat, to_fixed, AppContext};

#[derive(Debug, Default)]
pub struct InputState {
    pub dragging: bool,
    pub drag_start: Position,
    pub win_start: Position,
}

#[derive(Debug)]
pub struct Thumbnail<'a> {
    pub window: Window,
    pub width: u16,
    pub height: u16,

    config: &'a DisplayConfig,
    font_renderer: &'a FontRenderer,
    border_fill: Picture,

    src_picture: Picture,
    dst_picture: Picture,
    overlay_gc: Gcontext,
    overlay_pixmap: Pixmap,
    overlay_picture: Picture,

    pub character_name: String,
    pub focused: bool,
    pub visible: bool,
    pub minimized: bool,

    pub src: Window,
    root: Window,
    pub damage: Damage,
    pub input_state: InputState,
    conn: &'a RustConnection,
}

impl<'a> Thumbnail<'a> {
    pub fn new(
        ctx: &AppContext<'a>,
        character_name: String,
        src: Window,
        font_renderer: &'a FontRenderer,
        position: Option<Position>,
        width: u16,
        height: u16,
    ) -> Result<Self> {
        let src_geom = ctx.conn.get_geometry(src)?.reply()?;
        
        // Use saved position OR top-left of EVE window with 20px padding
        let Position { x, y } = position.unwrap_or_else(|| {
            Position::new(
                src_geom.x + positioning::DEFAULT_SPAWN_OFFSET,
                src_geom.y + positioning::DEFAULT_SPAWN_OFFSET,
            )
        });
        info!("Creating thumbnail for '{}' at position ({}, {}) with size {}x{}", 
              character_name, x, y, width, height);

        let window = ctx.conn.generate_id()?;
        ctx.conn.create_window(
            ctx.screen.root_depth,
            window,
            ctx.screen.root,
            x,
            y,
            width,
            height,
            0,
            WindowClass::INPUT_OUTPUT,
            ctx.screen.root_visual,
            &CreateWindowAux::new()
            .override_redirect(x11::OVERRIDE_REDIRECT)
            .event_mask(
                EventMask::SUBSTRUCTURE_NOTIFY
                | EventMask::BUTTON_PRESS
                | EventMask::BUTTON_RELEASE
                | EventMask::POINTER_MOTION,
            ),
        )?;

        let opacity_atom = ctx.conn
        .intern_atom(false, b"_NET_WM_WINDOW_OPACITY")?
        .reply()?
        .atom;
        ctx.conn.change_property32(
            PropMode::REPLACE,
            window,
            opacity_atom,
            AtomEnum::CARDINAL,
            &[ctx.config.opacity],
        )?;

        let wm_class = ctx.conn.intern_atom(false, b"WM_CLASS")?.reply()?.atom;
        ctx.conn.change_property8(
            PropMode::REPLACE,
            window,
            wm_class,
            AtomEnum::STRING,
            b"eve-l-preview\0eve-l-preview\0",
        )?;

        let net_wm_state = ctx.conn.intern_atom(false, b"_NET_WM_STATE")?.reply()?.atom;
        let above_atom = ctx.conn.intern_atom(false, b"_NET_WM_STATE_ABOVE")?.reply()?.atom;
        ctx.conn.change_property32(
            PropMode::REPLACE,
            window,
            net_wm_state,
            AtomEnum::ATOM,
            &[above_atom],
        )?;

        ctx.conn.map_window(window)
            .inspect_err(|e| error!("Failed to map thumbnail window {}: {:?}", window, e))?;
        info!("Mapped thumbnail window {} for '{}'", window, character_name);

        let border_fill = ctx.conn.generate_id()?;
        ctx.conn.render_create_solid_fill(border_fill, ctx.config.border_color)?;

        let pict_format = get_pictformat(ctx.conn, ctx.screen.root_depth, false)?;
        let src_picture = ctx.conn.generate_id()?;
        let dst_picture = ctx.conn.generate_id()?;
        ctx.conn.render_create_picture(src_picture, src, pict_format, &CreatePictureAux::new())?;
        ctx.conn.render_create_picture(dst_picture, window, pict_format, &CreatePictureAux::new())?;

        let overlay_pixmap = ctx.conn.generate_id()?;
        let overlay_picture = ctx.conn.generate_id()?;
        ctx.conn.create_pixmap(x11::ARGB_DEPTH, overlay_pixmap, ctx.screen.root, width, height)?;
        ctx.conn.render_create_picture(
            overlay_picture,
            overlay_pixmap,
            get_pictformat(ctx.conn, x11::ARGB_DEPTH, true)?,
            &CreatePictureAux::new(),
        )?;

        let overlay_gc = ctx.conn.generate_id()?;
        ctx.conn.create_gc(
            overlay_gc,
            overlay_pixmap,
            &CreateGCAux::new()
                .foreground(ctx.config.text_foreground),
        )?;

        let damage = ctx.conn.generate_id()?;
        ctx.conn.damage_create(damage, src, DamageReportLevel::RAW_RECTANGLES)?;

        let mut _self = Self {
            width,
            height,
            window,
            config: ctx.config,
            font_renderer,

            border_fill,
            src_picture,
            dst_picture,
            overlay_gc,
            overlay_pixmap,
            overlay_picture,

            character_name,
            focused: false,
            visible: true,
            minimized: false,

            src,
            root: ctx.screen.root,
            damage,
            input_state: InputState::default(),
            conn: ctx.conn,
        };
        _self.update_name()?;
        Ok(_self)
    }

    pub fn visibility(&mut self, visible: bool) -> Result<()> {
        if visible == self.visible {return Ok(());}
        self.visible = visible;
        if visible {
            self.conn.map_window(self.window)?;
        } else {
            self.conn.unmap_window(self.window)?;
        }
        Ok(())
    }

    fn capture(&self) -> Result<()> {
        let geom = self.conn.get_geometry(self.src)?.reply()?;
        let transform = Transform {
            matrix11: to_fixed(geom.width as f32 / self.width as f32),
            matrix22: to_fixed(geom.height as f32 / self.height as f32),
            matrix33: to_fixed(1.0),
            ..Default::default()
        };
        self.conn
            .render_set_picture_transform(self.src_picture, transform)?;
        self.conn.render_composite(
            PictOp::SRC,
            self.src_picture,
            0u32,
            self.dst_picture,
            0,
            0,
            0,
            0,
            0,
            0,
            self.width,
            self.height,
        )?;
        Ok(())
    }

    pub fn border(&self, focused: bool) -> Result<()> {
        if focused {
            self.conn.render_composite(
                PictOp::SRC,
                self.border_fill,
                0u32,
                self.overlay_picture,
                0,
                0,
                0,
                0,
                0,
                0,
                self.width,
                self.height,
            )?;
        } else {
            self.conn.render_composite(
                PictOp::CLEAR,
                self.overlay_picture,
                0u32,
                self.overlay_picture,
                0,
                0,
                0,
                0,
                0,
                0,
                self.width,
                self.height,
            )?;
        }
        self.update_name()?;
        Ok(())
    }

    pub fn minimized(&mut self) -> Result<()> {
        self.minimized = true;
        self.border(false)?;
        let extents = self
            .conn
            .query_text_extents(
                self.overlay_gc,
                b"MINIMIZED"
                    .iter()
                    .map(|&c| Char2b { byte1: 0, byte2: c })
                    .collect::<Vec<_>>()
                    .as_slice(),
            )?
            .reply()?;
        self.conn.image_text8(
            self.overlay_pixmap,
            self.overlay_gc,
            (self.width as i16 - extents.overall_width as i16) / 2,
            (self.height as i16 + extents.font_ascent + extents.font_descent) / 2,
            b"MINIMIZED",
        )?;
        self.update()?;

        Ok(())
    }

    pub fn update_name(&self) -> Result<()> {
        // Clear the overlay area (inside border)
        self.conn.render_composite(
            PictOp::CLEAR,
            self.overlay_picture,
            0u32,
            self.overlay_picture,
            0,
            0,
            0,
            0,
            self.config.border_size as i16,
            self.config.border_size as i16,
            self.width - self.config.border_size * 2,
            self.height - self.config.border_size * 2,
        )?;
        
        // Render text with fontdue
        let rendered = self.font_renderer.render_text(
            &self.character_name,
            self.config.text_foreground,
        )?;
        
        if rendered.width > 0 && rendered.height > 0 {
            // Upload rendered text bitmap to X11
            let text_pixmap = self.conn.generate_id()?;
            self.conn.create_pixmap(
                x11::ARGB_DEPTH,
                text_pixmap,
                self.overlay_pixmap,
                rendered.width as u16,
                rendered.height as u16,
            )?;
            
            // Convert Vec<u32> ARGB to bytes in X11 native format (little-endian BGRA)
            let mut image_data = Vec::with_capacity(rendered.data.len() * 4);
            for pixel in &rendered.data {
                image_data.push(*pixel as u8);        // B
                image_data.push((pixel >> 8) as u8);  // G
                image_data.push((pixel >> 16) as u8); // R
                image_data.push((pixel >> 24) as u8); // A
            }
            
            self.conn.put_image(
                ImageFormat::Z_PIXMAP,
                text_pixmap,
                self.overlay_gc,
                rendered.width as u16,
                rendered.height as u16,
                0,
                0,
                0,
                x11::ARGB_DEPTH,
                &image_data,
            )?;
            
            // Create picture for the text pixmap
            let text_picture = self.conn.generate_id()?;
            self.conn.render_create_picture(
                text_picture,
                text_pixmap,
                get_pictformat(self.conn, x11::ARGB_DEPTH, true)?,
                &CreatePictureAux::new(),
            )?;
            
            // Composite text onto overlay
            self.conn.render_composite(
                PictOp::OVER,
                text_picture,
                0u32,
                self.overlay_picture,
                0,
                0,
                0,
                0,
                self.config.text_x,
                self.config.text_y,
                rendered.width as u16,
                rendered.height as u16,
            )?;
            
            // Cleanup
            self.conn.render_free_picture(text_picture)?;
            self.conn.free_pixmap(text_pixmap)?;
        }
        
        Ok(())
    }

    fn overlay(&self) -> Result<()> {
        self.conn.render_composite(
            PictOp::OVER,
            self.overlay_picture,
            0u32,
            self.dst_picture,
            0,
            0,
            0,
            0,
            0,
            0,
            self.width,
            self.height,
        )?;
        Ok(())
    }

    pub fn update(&self) -> Result<()> {
        self.capture()?;
        self.overlay()?;
        Ok(())
    }

    pub fn focus(&self) -> Result<(), x11rb::errors::ReplyError> {
        let net_active = self
            .conn
            .intern_atom(false, b"_NET_ACTIVE_WINDOW")?
            .reply()?
            .atom;

        let ev = ClientMessageEvent {
            response_type: CLIENT_MESSAGE_EVENT,
            format: 32,
            sequence: 0,
            window: self.src,
            type_: net_active,
            data: [2, 0, 0, 0, 0].into(),
        };

        self.conn.send_event(
            false,
            self.root,
            EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
            ev,
        )?;
        self.conn.flush()?;
        info!("focused window: window={}", self.window);
        Ok(())
    }

    pub fn reposition(&mut self, x: i16, y: i16) -> Result<()> {
        self.conn.configure_window(
            self.window,
            &ConfigureWindowAux::new().x(x as i32).y(y as i32),
        )?;
        self.conn.flush()?;
        Ok(())
    }

    /// Called when character name changes (login/logout)
    /// Updates name and optionally moves to new position
    pub fn set_character_name(&mut self, new_name: String, new_position: Option<Position>) -> Result<()> {
        self.character_name = new_name;
        self.update_name()?;
        
        if let Some(Position { x, y }) = new_position {
            self.reposition(x, y)?;
        }
        Ok(())
    }

    pub fn is_hovered(&self, x: i16, y: i16) -> bool {
        // Query actual window geometry to avoid desync when compositor moves window
        if let Ok(req) = self.conn.get_geometry(self.window)
            && let Ok(geom) = req.reply()
        {
            return x >= geom.x
                && x <= geom.x + geom.width as i16
                && y >= geom.y
                && y <= geom.y + geom.height as i16;
        }
        false
    }
}

impl Drop for Thumbnail<'_> {
    fn drop(&mut self) {
        if let Err(e) = (|| {
            self.conn.damage_destroy(self.damage)?;
            self.conn.free_gc(self.overlay_gc)?;
            self.conn.render_free_picture(self.overlay_picture)?;
            self.conn.render_free_picture(self.src_picture)?;
            self.conn.render_free_picture(self.dst_picture)?;
            self.conn.render_free_picture(self.border_fill)?;
            self.conn.free_pixmap(self.overlay_pixmap)?;
            self.conn.destroy_window(self.window)?;
            self.conn.flush()?;
            Ok::<(), anyhow::Error>(())
        })() {
            tracing::error!("error during thumbnail drop: {e:?}");
        }
    }
}
