use anyhow::{Context, Result};
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
use crate::types::{Dimensions, Position};
use crate::x11_utils::{get_pictformat, to_fixed, AppContext};

#[derive(Debug, Default)]
pub struct InputState {
    pub dragging: bool,
    pub drag_start: Position,
    pub win_start: Position,
}

#[derive(Debug)]
pub struct Thumbnail<'a> {
    // === Application State (public, frequently accessed) ===
    pub character_name: String,
    pub focused: bool,
    pub visible: bool,
    pub minimized: bool,
    pub input_state: InputState,
    
    // === Geometry (public, immutable after creation) ===
    pub dimensions: Dimensions,
    
    // === X11 Window Handles (private/public owned resources) ===
    pub window: Window,      // Our thumbnail window (public for event handling)
    pub src: Window,         // Source EVE window (public for event handling)
    pub damage: Damage,      // DAMAGE extension handle (public for event matching)
    root: Window,            // Root window (private, cached from screen)
    
    // === X11 Render Resources (private, owned resources) ===
    border_fill: Picture,    // Solid color fill for border
    src_picture: Picture,    // Picture wrapping source window
    dst_picture: Picture,    // Picture wrapping our thumbnail window
    overlay_gc: Gcontext,    // Graphics context for text rendering
    overlay_pixmap: Pixmap,  // Backing pixmap for overlay compositing
    overlay_picture: Picture, // Picture wrapping overlay pixmap
    
    // === Borrowed Dependencies (private, references to app context) ===
    conn: &'a RustConnection,
    config: &'a DisplayConfig,
    font_renderer: &'a FontRenderer,
}

impl<'a> Thumbnail<'a> {
    /// Create and configure the X11 window
    fn create_window(
        ctx: &AppContext,
        character_name: &str,
        x: i16,
        y: i16,
        dimensions: Dimensions,
    ) -> Result<Window> {
        let window = ctx.conn.generate_id()
            .context("Failed to generate X11 window ID")?;
        ctx.conn.create_window(
            ctx.screen.root_depth,
            window,
            ctx.screen.root,
            x,
            y,
            dimensions.width,
            dimensions.height,
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
        )
        .context(format!("Failed to create thumbnail window for '{}'", character_name))?;
        
        Ok(window)
    }

    /// Setup window properties (opacity, WM_CLASS, always-on-top)
    fn setup_window_properties(
        ctx: &AppContext,
        window: Window,
        character_name: &str,
    ) -> Result<()> {
        // Set opacity
        let opacity_atom = ctx.conn
            .intern_atom(false, b"_NET_WM_WINDOW_OPACITY")
            .context("Failed to intern _NET_WM_WINDOW_OPACITY atom")?
            .reply()
            .context("Failed to get reply for _NET_WM_WINDOW_OPACITY atom")?
            .atom;
        ctx.conn.change_property32(
            PropMode::REPLACE,
            window,
            opacity_atom,
            AtomEnum::CARDINAL,
            &[ctx.config.opacity],
        )
        .context(format!("Failed to set window opacity for '{}'", character_name))?;

        // Set WM_CLASS
        let wm_class = ctx.conn.intern_atom(false, b"WM_CLASS")
            .context("Failed to intern WM_CLASS atom")?
            .reply()
            .context("Failed to get reply for WM_CLASS atom")?
            .atom;
        ctx.conn.change_property8(
            PropMode::REPLACE,
            window,
            wm_class,
            AtomEnum::STRING,
            b"eve-l-preview\0eve-l-preview\0",
        )
        .context(format!("Failed to set WM_CLASS for '{}'", character_name))?;

        // Set always-on-top
        let net_wm_state = ctx.conn.intern_atom(false, b"_NET_WM_STATE")
            .context("Failed to intern _NET_WM_STATE atom")?
            .reply()
            .context("Failed to get reply for _NET_WM_STATE atom")?
            .atom;
        let above_atom = ctx.conn.intern_atom(false, b"_NET_WM_STATE_ABOVE")
            .context("Failed to intern _NET_WM_STATE_ABOVE atom")?
            .reply()
            .context("Failed to get reply for _NET_WM_STATE_ABOVE atom")?
            .atom;
        ctx.conn.change_property32(
            PropMode::REPLACE,
            window,
            net_wm_state,
            AtomEnum::ATOM,
            &[above_atom],
        )
        .context(format!("Failed to set window always-on-top for '{}'", character_name))?;

        // Map window to make it visible
        ctx.conn.map_window(window)
            .inspect_err(|e| error!("Failed to map thumbnail window {}: {:?}", window, e))
            .context(format!("Failed to map thumbnail window for '{}'", character_name))?;
        info!("Mapped thumbnail window {} for '{}'", window, character_name);

        Ok(())
    }

    /// Create render pictures and resources
    fn create_render_resources(
        ctx: &AppContext,
        window: Window,
        src: Window,
        dimensions: Dimensions,
        character_name: &str,
    ) -> Result<(Picture, Picture, Picture, Pixmap, Picture, Gcontext)> {
        // Border fill
        let border_fill = ctx.conn.generate_id()
            .context("Failed to generate ID for border fill picture")?;
        ctx.conn.render_create_solid_fill(border_fill, ctx.config.border_color)
            .context(format!("Failed to create border fill for '{}'", character_name))?;

        // Source and destination pictures
        let pict_format = get_pictformat(ctx.conn, ctx.screen.root_depth, false)
            .context("Failed to get picture format for thumbnail rendering")?;
        let src_picture = ctx.conn.generate_id()
            .context("Failed to generate ID for source picture")?;
        let dst_picture = ctx.conn.generate_id()
            .context("Failed to generate ID for destination picture")?;
        ctx.conn.render_create_picture(src_picture, src, pict_format, &CreatePictureAux::new())
            .context(format!("Failed to create source picture for '{}'", character_name))?;
        ctx.conn.render_create_picture(dst_picture, window, pict_format, &CreatePictureAux::new())
            .context(format!("Failed to create destination picture for '{}'", character_name))?;

        // Overlay resources
        let overlay_pixmap = ctx.conn.generate_id()
            .context("Failed to generate ID for overlay pixmap")?;
        let overlay_picture = ctx.conn.generate_id()
            .context("Failed to generate ID for overlay picture")?;
        ctx.conn.create_pixmap(x11::ARGB_DEPTH, overlay_pixmap, ctx.screen.root, dimensions.width, dimensions.height)
            .context(format!("Failed to create overlay pixmap for '{}'", character_name))?;
        ctx.conn.render_create_picture(
            overlay_picture,
            overlay_pixmap,
            get_pictformat(ctx.conn, x11::ARGB_DEPTH, true)
                .context("Failed to get ARGB picture format for overlay")?,
            &CreatePictureAux::new(),
        )
        .context(format!("Failed to create overlay picture for '{}'", character_name))?;

        let overlay_gc = ctx.conn.generate_id()
            .context("Failed to generate ID for overlay graphics context")?;
        ctx.conn.create_gc(
            overlay_gc,
            overlay_pixmap,
            &CreateGCAux::new()
                .foreground(ctx.config.text_foreground),
        )
        .context(format!("Failed to create graphics context for '{}'", character_name))?;

        Ok((border_fill, src_picture, dst_picture, overlay_pixmap, overlay_picture, overlay_gc))
    }

    /// Create damage tracking for source window
    fn create_damage_tracking(
        ctx: &AppContext,
        src: Window,
        character_name: &str,
    ) -> Result<Damage> {
        let damage = ctx.conn.generate_id()
            .context("Failed to generate ID for damage tracking")?;
        ctx.conn.damage_create(damage, src, DamageReportLevel::RAW_RECTANGLES)
            .context(format!("Failed to create damage tracking for '{}' (check DAMAGE extension)", character_name))?;
        Ok(damage)
    }

    pub fn new(
        ctx: &AppContext<'a>,
        character_name: String,
        src: Window,
        font_renderer: &'a FontRenderer,
        position: Option<Position>,
        dimensions: Dimensions,
    ) -> Result<Self> {
        // Validate dimensions are non-zero
        if dimensions.width == 0 || dimensions.height == 0 {
            return Err(anyhow::anyhow!(
                "Invalid thumbnail dimensions for '{}': {}x{} (must be non-zero)",
                character_name, dimensions.width, dimensions.height
            ));
        }
        
        // Query source window geometry
        let src_geom = ctx.conn.get_geometry(src)
            .context("Failed to send geometry query for source EVE window")?
            .reply()
            .context(format!("Failed to get geometry for source window {} (character: '{}')", src, character_name))?;
        
        // Use saved position OR top-left of EVE window with 20px padding
        let Position { x, y } = position.unwrap_or_else(|| {
            Position::new(
                src_geom.x + positioning::DEFAULT_SPAWN_OFFSET,
                src_geom.y + positioning::DEFAULT_SPAWN_OFFSET,
            )
        });
        info!("Creating thumbnail for '{}' at position ({}, {}) with size {}x{}", 
              character_name, x, y, dimensions.width, dimensions.height);

        // Create window and setup properties
        let window = Self::create_window(ctx, &character_name, x, y, dimensions)?;
        
        // Setup a cleanup guard that destroys the window if we fail during initialization
        // This prevents leaking the window if later steps fail
        struct WindowGuard<'a> {
            conn: &'a RustConnection,
            window: Window,
            character_name: String,
            should_cleanup: bool,
        }
        
        impl Drop for WindowGuard<'_> {
            fn drop(&mut self) {
                if self.should_cleanup {
                    if let Err(e) = self.conn.destroy_window(self.window) {
                        error!("Failed to cleanup window {} for '{}' after initialization failure: {}", 
                               self.window, self.character_name, e);
                    }
                    // Flush to ensure cleanup is sent to server
                    let _ = self.conn.flush();
                }
            }
        }
        
        let mut window_guard = WindowGuard {
            conn: ctx.conn,
            window,
            character_name: character_name.clone(),
            should_cleanup: true,
        };
        
        Self::setup_window_properties(ctx, window, &character_name)?;

        // Create rendering resources
        let (border_fill, src_picture, dst_picture, overlay_pixmap, overlay_picture, overlay_gc) = 
            Self::create_render_resources(ctx, window, src, dimensions, &character_name)?;

        // Setup damage tracking
        let damage = Self::create_damage_tracking(ctx, src, &character_name)?;

        let thumbnail = Self {
            // Application State
            character_name,
            focused: false,
            visible: true,
            minimized: false,
            input_state: InputState::default(),
            
            // Geometry
            dimensions,
            
            // X11 Window Handles
            window,
            src,
            damage,
            root: ctx.screen.root,
            
            // X11 Render Resources
            border_fill,
            src_picture,
            dst_picture,
            overlay_gc,
            overlay_pixmap,
            overlay_picture,
            
            // Borrowed Dependencies
            conn: ctx.conn,
            config: ctx.config,
            font_renderer,
        };
        
        // Render initial name overlay
        thumbnail.update_name()
            .context(format!("Failed to render initial name overlay for '{}'", thumbnail.character_name))?;
        
        // Success! Disable cleanup guard since Thumbnail's Drop will handle it now
        window_guard.should_cleanup = false;
        
        Ok(thumbnail)
    }

    pub fn visibility(&mut self, visible: bool) -> Result<()> {
        if visible == self.visible {return Ok(());}
        self.visible = visible;
        if visible {
            self.conn.map_window(self.window)
                .context(format!("Failed to map window for '{}'", self.character_name))?;
        } else {
            self.conn.unmap_window(self.window)
                .context(format!("Failed to unmap window for '{}'", self.character_name))?;
        }
        Ok(())
    }

    fn capture(&self) -> Result<()> {
        let geom = self.conn.get_geometry(self.src)
            .context("Failed to send geometry query for source window")?
            .reply()
            .context(format!("Failed to get geometry for source window (character: '{}')", self.character_name))?;
        let transform = Transform {
            matrix11: to_fixed(geom.width as f32 / self.dimensions.width as f32),
            matrix22: to_fixed(geom.height as f32 / self.dimensions.height as f32),
            matrix33: to_fixed(1.0),
            ..Default::default()
        };
        self.conn
            .render_set_picture_transform(self.src_picture, transform)
            .context(format!("Failed to set transform for '{}'", self.character_name))?;
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
            self.dimensions.width,
            self.dimensions.height,
        )
        .context(format!("Failed to composite source window for '{}'", self.character_name))?;
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
                self.dimensions.width,
                self.dimensions.height,
            )
            .context(format!("Failed to render border for '{}'", self.character_name))?;
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
                self.dimensions.width,
                self.dimensions.height,
            )
            .context(format!("Failed to clear border for '{}'", self.character_name))?;
        }
        self.update_name()
            .context(format!("Failed to update name overlay after border change for '{}'", self.character_name))?;
        Ok(())
    }

    pub fn minimized(&mut self) -> Result<()> {
        self.minimized = true;
        self.border(false)
            .context(format!("Failed to clear border for minimized window '{}'", self.character_name))?;
        let extents = self
            .conn
            .query_text_extents(
                self.overlay_gc,
                b"MINIMIZED"
                    .iter()
                    .map(|&c| Char2b { byte1: 0, byte2: c })
                    .collect::<Vec<_>>()
                    .as_slice(),
            )
            .context("Failed to send text extents query for MINIMIZED text")?
            .reply()
            .context("Failed to get text extents for MINIMIZED text")?;
        self.conn.image_text8(
            self.overlay_pixmap,
            self.overlay_gc,
            (self.dimensions.width as i16 - extents.overall_width as i16) / 2,
            (self.dimensions.height as i16 + extents.font_ascent + extents.font_descent) / 2,
            b"MINIMIZED",
        )
        .context(format!("Failed to render MINIMIZED text for '{}'", self.character_name))?;
        self.update()
            .context(format!("Failed to update minimized display for '{}'", self.character_name))?;

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
            self.dimensions.width - self.config.border_size * 2,
            self.dimensions.height - self.config.border_size * 2,
        )
        .context(format!("Failed to clear overlay area for '{}'", self.character_name))?;
        
        // Render text with fontdue
        let rendered = self.font_renderer.render_text(
            &self.character_name,
            self.config.text_foreground,
        )
        .context(format!("Failed to render text '{}' with font renderer", self.character_name))?;
        
        if rendered.width > 0 && rendered.height > 0 {
            // Upload rendered text bitmap to X11
            let text_pixmap = self.conn.generate_id()
                .context("Failed to generate ID for text pixmap")?;
            self.conn.create_pixmap(
                x11::ARGB_DEPTH,
                text_pixmap,
                self.overlay_pixmap,
                rendered.width as u16,
                rendered.height as u16,
            )
            .context(format!("Failed to create text pixmap for '{}'", self.character_name))?;
            
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
            )
            .context(format!("Failed to upload text image for '{}'", self.character_name))?;
            
            // Create picture for the text pixmap
            let text_picture = self.conn.generate_id()
                .context("Failed to generate ID for text picture")?;
            self.conn.render_create_picture(
                text_picture,
                text_pixmap,
                get_pictformat(self.conn, x11::ARGB_DEPTH, true)
                    .context("Failed to get ARGB picture format for text")?,
                &CreatePictureAux::new(),
            )
            .context(format!("Failed to create text picture for '{}'", self.character_name))?;
            
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
                self.config.text_offset.x,
                self.config.text_offset.y,
                rendered.width as u16,
                rendered.height as u16,
            )
            .context(format!("Failed to composite text onto overlay for '{}'", self.character_name))?;
            
            // Cleanup
            self.conn.render_free_picture(text_picture)
                .context("Failed to free text picture")?;
            self.conn.free_pixmap(text_pixmap)
                .context("Failed to free text pixmap")?;
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
            self.dimensions.width,
            self.dimensions.height,
        )
        .context(format!("Failed to composite overlay onto destination for '{}'", self.character_name))?;
        Ok(())
    }

    pub fn update(&self) -> Result<()> {
        self.capture()
            .context(format!("Failed to capture source window for '{}'", self.character_name))?;
        self.overlay()
            .context(format!("Failed to apply overlay for '{}'", self.character_name))?;
        Ok(())
    }

    pub fn focus(&self) -> Result<()> {
        let net_active = self
            .conn
            .intern_atom(false, b"_NET_ACTIVE_WINDOW")
            .context("Failed to intern _NET_ACTIVE_WINDOW atom")?
            .reply()
            .context("Failed to get reply for _NET_ACTIVE_WINDOW atom")?
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
        )
        .context(format!("Failed to send focus event for '{}'", self.character_name))?;
        self.conn.flush()
            .context("Failed to flush X11 connection after focus event")?;
        info!("focused window: window={}", self.window);
        Ok(())
    }

    pub fn reposition(&mut self, x: i16, y: i16) -> Result<()> {
        self.conn.configure_window(
            self.window,
            &ConfigureWindowAux::new().x(x as i32).y(y as i32),
        )
        .context(format!("Failed to reposition window for '{}' to ({}, {})", self.character_name, x, y))?;
        self.conn.flush()
            .context("Failed to flush X11 connection after reposition")?;
        Ok(())
    }

    /// Called when character name changes (login/logout)
    /// Updates name and optionally moves to new position
    pub fn set_character_name(&mut self, new_name: String, new_position: Option<Position>) -> Result<()> {
        self.character_name = new_name;
        self.update_name()
            .context(format!("Failed to update name overlay to '{}'", self.character_name))?;
        
        if let Some(Position { x, y }) = new_position {
            self.reposition(x, y)
                .context(format!("Failed to reposition after character change to '{}'", self.character_name))?;
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
        // Clean up each resource independently to prevent cascade failures
        // If one cleanup fails, we still attempt to clean up the rest
        
        if let Err(e) = self.conn.damage_destroy(self.damage) {
            error!("Failed to destroy damage {}: {}", self.damage, e);
        }
        
        if let Err(e) = self.conn.free_gc(self.overlay_gc) {
            error!("Failed to free GC {}: {}", self.overlay_gc, e);
        }
        
        if let Err(e) = self.conn.render_free_picture(self.overlay_picture) {
            error!("Failed to free overlay picture {}: {}", self.overlay_picture, e);
        }
        
        if let Err(e) = self.conn.render_free_picture(self.src_picture) {
            error!("Failed to free source picture {}: {}", self.src_picture, e);
        }
        
        if let Err(e) = self.conn.render_free_picture(self.dst_picture) {
            error!("Failed to free destination picture {}: {}", self.dst_picture, e);
        }
        
        if let Err(e) = self.conn.render_free_picture(self.border_fill) {
            error!("Failed to free border fill picture {}: {}", self.border_fill, e);
        }
        
        if let Err(e) = self.conn.free_pixmap(self.overlay_pixmap) {
            error!("Failed to free pixmap {}: {}", self.overlay_pixmap, e);
        }
        
        if let Err(e) = self.conn.destroy_window(self.window) {
            error!("Failed to destroy window {} for '{}': {}", 
                   self.window, self.character_name, e);
        }
        
        if let Err(e) = self.conn.flush() {
            error!("Failed to flush X11 connection during cleanup: {}", e);
        }
    }
}
