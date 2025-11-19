//! Font discovery via fontconfig
//!
//! Query system fonts and resolve font family names to file paths

use anyhow::{Context, Result};
use fontconfig::{Fontconfig, Pattern};
use std::collections::BTreeSet;
use std::ffi::CString;
use std::path::PathBuf;
use tracing::{debug, info, warn};

/// Common font style names for parsing family+style strings
/// Order matters: longer/more specific styles must come first to avoid substring matches
/// (e.g., "SemiBold Italic" must be checked before "Bold Italic")
const KNOWN_STYLES: &[&str] = &[
    "Condensed Bold Italic",
    "Condensed Bold",
    "SemiBold Italic",
    "Bold Italic",
    "Bold Oblique",
    "Black Italic",
    "Medium Italic",
    "Light Italic",
    "Thin Italic",
    "ExtraBold",
    "ExtraLight",
    "SemiBold",
    "Italic",
    "Oblique",
    "Bold",
    "Light",
    "Medium",
    "Black",
    "Thin",
    "Regular",
    "Condensed",
    "Expanded",
];

/// Get list of all individual fonts with their full names (e.g., "Roboto Mono Regular", "DejaVu Sans Bold")
/// This provides more granular control than just font families
pub fn list_fonts() -> Result<Vec<String>> {
    info!("Loading available fonts from fontconfig...");
    let fc = Fontconfig::new().context("Failed to initialize fontconfig")?;
    
    // Create empty pattern to match all fonts
    let pattern = Pattern::new(&fc);
    
    // List all fonts
    let font_set = fontconfig::list_fonts(&pattern, None);
    
    // Extract unique font names (use BTreeSet for sorted, deduplicated results)
    let mut fonts = BTreeSet::new();
    
    for font_pattern in font_set.iter() {
        // Use the primary family name (index 0) to avoid weight-specific family aliases
        let family = font_pattern.get_string(fontconfig::FC_FAMILY)
            .unwrap_or("Unknown");
        
        // Get style from FC_STYLE which has full names (e.g., "SemiBold Italic", not "SmBd It")
        let font_name = if let Some(style_str) = font_pattern.get_string(fontconfig::FC_STYLE) {
            // Skip "Regular" style as it's implied
            if style_str == "Regular" {
                family.to_string()
            } else {
                format!("{} {}", family, style_str)
            }
        } else {
            family.to_string()
        };
        
        fonts.insert(font_name);
    }
    
    info!(
        count = fonts.len(),
        "Discovered individual fonts via fontconfig"
    );
    
    Ok(fonts.into_iter().collect())
}

/// Find best matching font file path for a given family name or full font name
/// Expects format: "Family Name" or "Family Name Style" (e.g., "Roboto Mono" or "Roboto Mono SemiBold Italic")
pub fn find_font_path(font_name: &str) -> Result<PathBuf> {
    let fc = Fontconfig::new().context("Failed to initialize fontconfig")?;
    
    let mut family_name = font_name;
    let mut style_name: Option<&str> = None;
    
    // Try to extract style from font name using known style names
    // Must check that style is preceded by space to avoid substring matches (e.g., "SemiBold" vs "Bold")
    for style in KNOWN_STYLES {
        if let Some(style_pos) = font_name.rfind(style) {
            // Check if this is at the end and preceded by a space (or is the whole string)
            if style_pos + style.len() == font_name.len() {
                let prefix = &font_name[..style_pos];
                if prefix.is_empty() || prefix.ends_with(' ') {
                    family_name = prefix.trim();
                    style_name = Some(style);
                    debug!(font = font_name, family = family_name, style = style, "Parsed font into family and style");
                    break;
                }
            }
        }
    }
    
    // Build pattern with family and optional style
    let mut pattern = Pattern::new(&fc);
    let family_cstr = CString::new(family_name)
        .with_context(|| format!("Invalid family name: {}", family_name))?;
    pattern.add_string(fontconfig::FC_FAMILY, &family_cstr);
    
    if let Some(style) = style_name {
        let style_cstr = CString::new(style)
            .with_context(|| format!("Invalid style name: {}", style))?;
        pattern.add_string(fontconfig::FC_STYLE, &style_cstr);
    }
    
    let matched = pattern.font_match();
    
    // Verify we got the right family (fontconfig does fuzzy matching and may return a fallback)
    if let Some(matched_family) = matched.get_string(fontconfig::FC_FAMILY) {
        // Check if the matched family matches our requested family
        if !matched_family.eq_ignore_ascii_case(family_name) {
            warn!(
                requested = font_name,
                requested_family = family_name,
                matched_family = matched_family,
                "Fontconfig returned different font family - requested font may not be installed"
            );
            return Err(anyhow::anyhow!(
                "Font '{}' not found - fontconfig returned family '{}' instead",
                font_name,
                matched_family
            ));
        }
    }
    
    // Extract file path
    let file_path = matched
        .filename()
        .with_context(|| format!("No font file found for '{}'", font_name))?;
    
    let path = PathBuf::from(file_path);
    
    if !path.exists() {
        warn!(
            font = font_name,
            path = %path.display(),
            "Font file path from fontconfig does not exist"
        );
        return Err(anyhow::anyhow!(
            "Font file path '{}' does not exist",
            path.display()
        ));
    }
    
    debug!(
        font = font_name,
        family = family_name,
        style = ?style_name,
        path = %path.display(),
        "Resolved font path via family + style"
    );
    
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_common_fonts() {
        // Test finding some common fonts
        let test_families = vec![
            "DejaVu Sans",
            "Liberation Sans",
            "Monospace",  // Generic family
        ];

        for family in test_families {
            if let Ok(path) = find_font_path(family) {
                println!("{} -> {}", family, path.display());
                assert!(path.is_absolute(), "Font path should be absolute");
            }
        }
    }
}
