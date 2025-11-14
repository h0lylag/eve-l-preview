use anyhow::Result;
use std::cell::RefCell;
use tracing::info;
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

use crate::config::Config;
use crate::x11_utils::{get_pictformat, to_fixed};

#[derive(Debug, Default)]
pub struct InputState {
    pub dragging: bool,
    pub drag_start: (i16, i16),
    pub win_start: (i16, i16),
}

#[derive(Debug)]
pub struct Thumbnail<'a> {
    pub window: Window,
    pub x: i16,
    pub y: i16,

    config: &'a RefCell<Config>,
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
        conn: &'a RustConnection,
        screen: &Screen,
        character_name: String,
        src: Window,
        font: Font,
        config: &'a RefCell<Config>,
        position: Option<(i16, i16)>,
    ) -> Result<Self> {
        let src_geom = conn.get_geometry(src)?.reply()?;
        
        // Borrow config for the initialization
        let cfg = config.borrow();
        
        // Use saved position OR center on source window
        let (x, y) = position.unwrap_or_else(|| {
            let x = src_geom.x + (src_geom.width - cfg.width) as i16 / 2;
            let y = src_geom.y + (src_geom.height - cfg.height) as i16 / 2;
            (x, y)
        });

        let window = conn.generate_id()?;
        conn.create_window(
            screen.root_depth,
            window,
            screen.root,
            x,
            y,
            cfg.width,
            cfg.height,
            0,
            WindowClass::INPUT_OUTPUT,
            screen.root_visual,
            &CreateWindowAux::new()
            .override_redirect(1)
            .event_mask(
                EventMask::SUBSTRUCTURE_NOTIFY
                | EventMask::BUTTON_PRESS
                | EventMask::BUTTON_RELEASE
                | EventMask::POINTER_MOTION,
            ),
        )?;

        let opacity_atom = conn
        .intern_atom(false, b"_NET_WM_WINDOW_OPACITY")?
        .reply()?
        .atom;
        conn.change_property32(
            PropMode::REPLACE,
            window,
            opacity_atom,
            AtomEnum::CARDINAL,
            &[cfg.opacity],
        )?;

        let wm_class = conn.intern_atom(false, b"WM_CLASS")?.reply()?.atom;
        conn.change_property8(
            PropMode::REPLACE,
            window,
            wm_class,
            AtomEnum::STRING,
            b"eve-l-preview\0eve-l-preview\0",
        )?;

        let net_wm_state = conn.intern_atom(false, b"_NET_WM_STATE")?.reply()?.atom;
        let above_atom = conn.intern_atom(false, b"_NET_WM_STATE_ABOVE")?.reply()?.atom;
        conn.change_property32(
            PropMode::REPLACE,
            window,
            net_wm_state,
            AtomEnum::ATOM,
            &[above_atom],
        )?;

        conn.map_window(window)?;

        let border_fill = conn.generate_id()?;
        conn.render_create_solid_fill(border_fill, cfg.border_color)?;

        let pict_format = get_pictformat(conn, screen.root_depth, false)?;
        let src_picture = conn.generate_id()?;
        let dst_picture = conn.generate_id()?;
        conn.render_create_picture(src_picture, src, pict_format, &CreatePictureAux::new())?;
        conn.render_create_picture(dst_picture, window, pict_format, &CreatePictureAux::new())?;

        let overlay_pixmap = conn.generate_id()?;
        let overlay_picture = conn.generate_id()?;
        conn.create_pixmap(32, overlay_pixmap, screen.root, cfg.width, cfg.height)?;
        conn.render_create_picture(
            overlay_picture,
            overlay_pixmap,
            get_pictformat(conn, 32, true)?,
            &CreatePictureAux::new(),
        )?;

        let overlay_gc = conn.generate_id()?;
        conn.create_gc(
            overlay_gc,
            overlay_pixmap,
            &CreateGCAux::new()
                .font(font)
                .foreground(cfg.text_foreground)
                .background(cfg.text_background),
        )?;

        let damage = conn.generate_id()?;
        conn.damage_create(damage, src, DamageReportLevel::RAW_RECTANGLES)?;

        // Drop the borrow before creating Self
        drop(cfg);

        let mut _self = Self {
            x,
            y,
            window,
            config,

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
            root: screen.root,
            damage,
            input_state: InputState::default(),
            conn,
        };
        _self.update_name()?;
        Ok(_self)
    }

    fn cfg(&self) -> std::cell::Ref<Config> {
        self.config.borrow()
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
            matrix11: to_fixed(geom.width as f32 / self.cfg().width as f32),
            matrix22: to_fixed(geom.height as f32 / self.cfg().height as f32),
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
            self.cfg().width,
            self.cfg().height,
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
                self.cfg().width,
                self.cfg().height,
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
                self.cfg().width,
                self.cfg().height,
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
            (self.cfg().width as i16 - extents.overall_width as i16) / 2,
            (self.cfg().height as i16 + extents.font_ascent + extents.font_descent) / 2,
            b"MINIMIZED",
        )?;
        self.update()?;

        Ok(())
    }

    pub fn update_name(&self) -> Result<()> {
        self.conn.render_composite(
            PictOp::CLEAR,
            self.overlay_picture,
            0u32,
            self.overlay_picture,
            0,
            0,
            0,
            0,
            self.cfg().border_size as i16,
            self.cfg().border_size as i16,
            self.cfg().width - self.cfg().border_size * 2,
            self.cfg().height - self.cfg().border_size * 2,
        )?;
        self.conn.image_text8(
            self.overlay_pixmap,
            self.overlay_gc,
            self.cfg().text_x,
            self.cfg().text_y,
            self.character_name.as_bytes(),
        )?;
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
            self.cfg().width,
            self.cfg().height,
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
        self.x = x;
        self.y = y;
        Ok(())
    }

    /// Called when character name changes (login/logout)
    /// Updates name and optionally moves to new position
    pub fn set_character_name(&mut self, new_name: String, new_position: Option<(i16, i16)>) -> Result<()> {
        self.character_name = new_name;
        self.update_name()?;
        
        if let Some((x, y)) = new_position {
            self.reposition(x, y)?;
        }
        Ok(())
    }

    pub fn is_hovered(&self, x: i16, y: i16) -> bool {
        x >= self.x
            && x <= self.x + self.cfg().width as i16
            && y >= self.y
            && y <= self.y + self.cfg().height as i16
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
