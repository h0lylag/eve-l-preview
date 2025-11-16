use eframe::egui;
use crate::config::profile::Profile;
use crate::gui::constants::*;

pub fn ui(ui: &mut egui::Ui, profile: &mut Profile) -> bool {
    let mut changed = false;
    
    ui.group(|ui| {
        ui.label(egui::RichText::new("Visual Settings").strong());
        ui.add_space(ITEM_SPACING);
        
        // Opacity
        ui.horizontal(|ui| {
            ui.label("Opacity:");
            if ui.add(egui::Slider::new(&mut profile.opacity_percent, 0..=100)
                .suffix("%")).changed() {
                changed = true;
            }
        });
        
        // Border toggle
        ui.horizontal(|ui| {
            ui.label("Borders:");
            if ui.checkbox(&mut profile.border_enabled, "Enabled").changed() {
                changed = true;
            }
        });
        
        // Border settings (only if enabled)
        if profile.border_enabled {
            ui.indent("border_settings", |ui| {
                ui.horizontal(|ui| {
                    ui.label("Border Size:");
                    if ui.add(egui::DragValue::new(&mut profile.border_size)
                        .range(1..=20)).changed() {
                        changed = true;
                    }
                });
                
                ui.horizontal(|ui| {
                    ui.label("Border Color:");
                    if ui.text_edit_singleline(&mut profile.border_color).changed() {
                        changed = true;
                    }
                    // TODO: Add color picker button
                });
            });
        }
        
        ui.add_space(ITEM_SPACING);
        
        // Text settings
        ui.horizontal(|ui| {
            ui.label("Text Size:");
            if ui.add(egui::DragValue::new(&mut profile.text_size)
                .range(8..=48)).changed() {
                changed = true;
            }
        });
        
        ui.horizontal(|ui| {
            ui.label("Text Position:");
            ui.label("X:");
            if ui.add(egui::DragValue::new(&mut profile.text_x)
                .range(0..=100)).changed() {
                changed = true;
            }
            ui.label("Y:");
            if ui.add(egui::DragValue::new(&mut profile.text_y)
                .range(0..=100)).changed() {
                changed = true;
            }
        });
        
        ui.horizontal(|ui| {
            ui.label("Text Foreground:");
            if ui.text_edit_singleline(&mut profile.text_foreground).changed() {
                changed = true;
            }
            // TODO: Add color picker button
        });
        
        ui.horizontal(|ui| {
            ui.label("Text Background:");
            if ui.text_edit_singleline(&mut profile.text_background).changed() {
                changed = true;
            }
            // TODO: Add color picker button
        });
    });
    
    changed
}
