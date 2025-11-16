//! Settings editor component for modifying configuration

use eframe::egui;
use crate::config::persistent::GlobalSettings;

// Import constants from parent module
use super::super::constants::{ITEM_SPACING, SECTION_SPACING};

/// Renders the settings editor UI and returns true if any changes were made
pub fn ui(ui: &mut egui::Ui, settings: &mut GlobalSettings) -> bool {
    let mut changed = false;
    
    // Visual Settings Section
    ui.group(|ui| {
        ui.label(egui::RichText::new("Visual Settings").heading().strong());
        ui.add_space(ITEM_SPACING);
        
        // Opacity
        ui.horizontal(|ui| {
            ui.label("Opacity:");
            ui.add_space(5.0);
            if ui.add(egui::Slider::new(&mut settings.opacity_percent, 0..=100)
                .suffix("%")
                .text(""))
                .changed() 
            {
                changed = true;
            }
        });
        
        ui.add_space(ITEM_SPACING);
        
        // Border Size
        ui.horizontal(|ui| {
            ui.label("Border Size:");
            ui.add_space(5.0);
            if ui.add(egui::Slider::new(&mut settings.border_size, 0..=20)
                .text(""))
                .changed() 
            {
                changed = true;
            }
        });
        
        ui.add_space(ITEM_SPACING);
        
        // Border Color
        ui.horizontal(|ui| {
            ui.label("Border Color:");
            ui.add_space(5.0);
            if ui.text_edit_singleline(&mut settings.border_color_hex).changed() {
                changed = true;
            }
            ui.label("(hex: #AARRGGBB)");
        });
        
        ui.add_space(ITEM_SPACING);
        
        // Text Size
        ui.horizontal(|ui| {
            ui.label("Text Size:");
            ui.add_space(5.0);
            if ui.add(egui::Slider::new(&mut settings.text_size, 8.0..=48.0)
                .text(""))
                .changed() 
            {
                changed = true;
            }
        });
        
        ui.add_space(ITEM_SPACING);
        
        // Text Position
        ui.horizontal(|ui| {
            ui.label("Text Position:");
            ui.add_space(5.0);
            ui.label("X:");
            if ui.add(egui::DragValue::new(&mut settings.text_x)
                .range(0..=100))
                .changed() 
            {
                changed = true;
            }
            ui.add_space(5.0);
            ui.label("Y:");
            if ui.add(egui::DragValue::new(&mut settings.text_y)
                .range(0..=100))
                .changed() 
            {
                changed = true;
            }
        });
        
        ui.add_space(ITEM_SPACING);
        
        // Text Color
        ui.horizontal(|ui| {
            ui.label("Text Color:");
            ui.add_space(5.0);
            if ui.text_edit_singleline(&mut settings.text_color_hex).changed() {
                changed = true;
            }
            ui.label("(hex: #AARRGGBB)");
        });
    });
    
    ui.add_space(SECTION_SPACING);
    
    // Behavior Settings Section
    ui.group(|ui| {
        ui.label(egui::RichText::new("Behavior Settings").heading().strong());
        ui.add_space(ITEM_SPACING);
        
        // Hide when no focus
        if ui.checkbox(&mut settings.hide_when_no_focus, "Hide thumbnails when EVE loses focus").changed() {
            changed = true;
        }
        
        ui.add_space(ITEM_SPACING);
        
        // Snap threshold
        ui.horizontal(|ui| {
            ui.label("Snap Threshold:");
            ui.add_space(5.0);
            if ui.add(egui::Slider::new(&mut settings.snap_threshold, 0..=50)
                .suffix(" px")
                .text(""))
                .changed() 
            {
                changed = true;
            }
        });
        
        ui.label(egui::RichText::new("(Distance for edge/corner snapping)")
            .small()
            .italics());
    });
    
    ui.add_space(SECTION_SPACING);
    
    // Hotkey Settings Section
    ui.group(|ui| {
        ui.label(egui::RichText::new("Hotkey Settings").heading().strong());
        ui.add_space(ITEM_SPACING);
        
        // Hotkey require EVE focus
        if ui.checkbox(&mut settings.hotkey_require_eve_focus, "Require EVE window focused for hotkeys").changed() {
            changed = true;
        }
        
        ui.add_space(ITEM_SPACING);
        
        // Hotkey order (read-only for now - Phase 3 will add reordering)
        ui.label("Character Cycle Order:");
        ui.label(egui::RichText::new("(Tab/Shift+Tab cycles through this order)")
            .small()
            .italics());
        
        ui.add_space(ITEM_SPACING / 2.0);
        
        egui::ScrollArea::vertical()
            .max_height(150.0)
            .show(ui, |ui| {
                for (idx, character) in settings.hotkey_order.iter().enumerate() {
                    ui.label(format!("{}. {}", idx + 1, character));
                }
                
                if settings.hotkey_order.is_empty() {
                    ui.label(egui::RichText::new("(No characters configured)")
                        .italics()
                        .weak());
                }
            });
        
        ui.add_space(ITEM_SPACING);
        ui.label(egui::RichText::new("Note: Edit hotkey_order in TOML for now")
            .small()
            .weak());
    });
    
    changed
}
