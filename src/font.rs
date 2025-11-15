//! TrueType font rendering using fontdue (pure Rust)

use anyhow::{Context, Result};
use fontdue::{Font, FontSettings};
use std::fs;
use std::path::PathBuf;

/// Rendered text as ARGB bitmap
pub struct RenderedText {
    pub width: usize,
    pub height: usize,
    pub data: Vec<u32>, // ARGB pixels (premultiplied alpha)
}

/// Font renderer using fontdue
#[derive(Debug)]
pub struct FontRenderer {
    font: Font,
    size: f32,
}

impl FontRenderer {
    /// Load a TrueType font from a file path
    pub fn from_path(path: PathBuf, size: f32) -> Result<Self> {
        let font_data = fs::read(&path)
            .with_context(|| format!("Failed to read font file: {}", path.display()))?;
        
        let font = Font::from_bytes(font_data, FontSettings::default())
            .map_err(|e| anyhow::anyhow!("Failed to parse font: {}", e))?;
        
        Ok(Self { font, size })
    }
    
    /// Try to find and load a common system font
    pub fn from_system_font(size: f32) -> Result<Self> {
        // Try compile-time font path first (set by Nix build via FONT_PATH env var)
        const FONT_PATH: Option<&str> = option_env!("FONT_PATH");
        if let Some(nix_font_path) = FONT_PATH {
            if let Ok(renderer) = Self::from_path(PathBuf::from(nix_font_path), size) {
                return Ok(renderer);
            }
        }
        
        // Fallback to hardcoded paths for non-NixOS systems
        let font_paths = [
            "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
            "/usr/share/fonts/TTF/DejaVuSans-Bold.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            "/usr/share/fonts/TTF/DejaVuSans.ttf",
            "/usr/share/fonts/truetype/liberation/LiberationSans-Bold.ttf",
            "/usr/share/fonts/liberation/LiberationSans-Bold.ttf",
        ];
        
        for path in &font_paths {
            if let Ok(renderer) = Self::from_path(PathBuf::from(path), size) {
                return Ok(renderer);
            }
        }
        
        Err(anyhow::anyhow!(
            "Could not find any system fonts. Tried FONT_PATH ({:?}) and hardcoded paths: {:?}",
            FONT_PATH,
            font_paths
        ))
    }
    
    /// Render text to an ARGB bitmap with the given foreground color (transparent background)
    pub fn render_text(
        &self,
        text: &str,
        fg_color: u32,  // ARGB format
    ) -> Result<RenderedText> {
        if text.is_empty() {
            return Ok(RenderedText {
                width: 0,
                height: 0,
                data: Vec::new(),
            });
        }
        
        // Layout glyphs
        let mut glyphs = Vec::new();
        let mut x = 0.0f32;
        let mut max_ascent = 0i32;
        let mut max_descent = 0i32;
        
        for ch in text.chars() {
            let (metrics, bitmap) = self.font.rasterize(ch, self.size);
            
            // Track the maximum ascent and descent
            let ascent = metrics.height as i32 + metrics.ymin;
            let descent = -metrics.ymin;
            max_ascent = max_ascent.max(ascent);
            max_descent = max_descent.max(descent);
            
            glyphs.push((x as i32, metrics, bitmap));
            x += metrics.advance_width;
        }
        
        let width = x.ceil() as usize;
        let height = (max_ascent + max_descent) as usize;
        
        if width == 0 || height == 0 {
            return Ok(RenderedText {
                width: 0,
                height: 0,
                data: Vec::new(),
            });
        }
        
        // Create ARGB bitmap filled with fully transparent pixels
        let mut data = vec![0x00000000; width * height];
        
        // Extract color components (foreground is NOT premultiplied - raw ARGB)
        let fg_a = ((fg_color >> 24) & 0xFF) as f32 / 255.0;
        let fg_r = ((fg_color >> 16) & 0xFF) as f32 / 255.0;
        let fg_g = ((fg_color >> 8) & 0xFF) as f32 / 255.0;
        let fg_b = (fg_color & 0xFF) as f32 / 255.0;
        
        // Render each glyph
        for (x_offset, metrics, bitmap) in glyphs {
            // Position glyph relative to baseline (which is at max_ascent from top)
            let baseline_y = max_ascent - (metrics.height as i32 + metrics.ymin);
            
            for gy in 0..metrics.height {
                for gx in 0..metrics.width {
                    let px = x_offset + gx as i32;
                    let py = baseline_y + gy as i32;
                    
                    if px < 0 || py < 0 || px >= width as i32 || py >= height as i32 {
                        continue;
                    }
                    
                    let coverage = bitmap[gy * metrics.width + gx] as f32 / 255.0;
                    
                    if coverage > 0.0 {
                        // Premultiply: alpha = fg_alpha * coverage, RGB = fg_RGB * coverage
                        let alpha = (fg_a * coverage * 255.0) as u32;
                        let r = (fg_r * coverage * 255.0) as u32;
                        let g = (fg_g * coverage * 255.0) as u32;
                        let b = (fg_b * coverage * 255.0) as u32;
                        
                        let pixel = (alpha << 24) | (r << 16) | (g << 8) | b;
                        data[(py as usize) * width + (px as usize)] = pixel;
                    }
                }
            }
        }
        
        Ok(RenderedText {
            width,
            height,
            data,
        })
    }
}
