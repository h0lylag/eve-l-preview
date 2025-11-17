use eframe::egui;
use crate::config::profile::{Config, Profile};
use crate::gui::constants::*;

pub struct ProfileSelector {
    edit_profile_name: String,
    edit_profile_desc: String,
    show_new_dialog: bool,
    show_duplicate_dialog: bool,
    show_delete_confirm: bool,
    show_edit_dialog: bool,
}

impl ProfileSelector {
    pub fn new() -> Self {
        Self {
            edit_profile_name: String::new(),
            edit_profile_desc: String::new(),
            show_new_dialog: false,
            show_duplicate_dialog: false,
            show_delete_confirm: false,
            show_edit_dialog: false,
        }
    }
    
    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        config: &mut Config,
        selected_idx: &mut usize,
    ) -> ProfileAction {
        let mut action = ProfileAction::None;
        
        ui.group(|ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Profile:").strong());
                
                // Profile dropdown
                let selected_profile = &config.profiles[*selected_idx];
                egui::ComboBox::from_id_salt("profile_selector")
                    .selected_text(&selected_profile.name)
                    .show_ui(ui, |ui| {
                        for (idx, profile) in config.profiles.iter().enumerate() {
                            let label = if profile.description.is_empty() {
                                profile.name.clone()
                            } else {
                                format!("{} - {}", profile.name, profile.description)
                            };
                            
                            if ui.selectable_value(selected_idx, idx, label).clicked() {
                                config.global.selected_profile = profile.name.clone();
                                action = ProfileAction::SwitchProfile;
                            }
                        }
                    });
            });
            
            ui.add_space(ITEM_SPACING);
            
            // Action buttons
            ui.horizontal(|ui| {
                if ui.button("➕ New").clicked() {
                    self.show_new_dialog = true;
                    self.edit_profile_name.clear();
                    self.edit_profile_desc.clear();
                }
                
                if ui.button("📋 Duplicate").clicked() {
                    self.show_duplicate_dialog = true;
                    let current = &config.profiles[*selected_idx];
                    self.edit_profile_name = format!("{} (copy)", current.name);
                    self.edit_profile_desc = current.description.clone();
                }

                if ui.button("✏ Edit").clicked() {
                    self.show_edit_dialog = true;
                    let current = &config.profiles[*selected_idx];
                    self.edit_profile_name = current.name.clone();
                    self.edit_profile_desc = current.description.clone();
                }
                
                if ui.button("🗑 Delete").clicked() && config.profiles.len() > 1 {
                    self.show_delete_confirm = true;
                }
                
                if config.profiles.len() == 1 {
                    ui.label("(Cannot delete last profile)");
                }
            });
        });
        
        // Modal dialogs
        if self.show_new_dialog {
            action = self.new_profile_dialog(ui.ctx(), config);
        }
        
        if self.show_duplicate_dialog {
            action = self.duplicate_profile_dialog(ui.ctx(), config, *selected_idx);
        }
        
        if self.show_edit_dialog {
            action = self.edit_profile_dialog(ui.ctx(), config, *selected_idx);
        }

        if self.show_delete_confirm {
            action = self.delete_confirm_dialog(ui.ctx(), config, selected_idx);
        }
        
        action
    }
    
    fn new_profile_dialog(&mut self, ctx: &egui::Context, config: &mut Config) -> ProfileAction {
        let mut action = ProfileAction::None;
        
        egui::Window::new("New Profile")
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label("Profile Name:");
                ui.text_edit_singleline(&mut self.edit_profile_name);
                
                ui.label("Description (optional):");
                ui.text_edit_singleline(&mut self.edit_profile_desc);
                
                ui.add_space(ITEM_SPACING);
                
                ui.horizontal(|ui| {
                    if ui.button("Create").clicked() {
                        if !self.edit_profile_name.is_empty() {
                            // Create new profile from default template
                            let new_profile = Profile::default_with_name(
                                self.edit_profile_name.clone(),
                                self.edit_profile_desc.clone(),
                            );
                            config.profiles.push(new_profile);
                            action = ProfileAction::ProfileCreated;
                            self.show_new_dialog = false;
                        }
                    }
                    
                    if ui.button("Cancel").clicked() {
                        self.show_new_dialog = false;
                    }
                });
            });
        
        action
    }
    
    fn duplicate_profile_dialog(
        &mut self,
        ctx: &egui::Context,
        config: &mut Config,
        source_idx: usize,
    ) -> ProfileAction {
        let mut action = ProfileAction::None;
        
        egui::Window::new("Duplicate Profile")
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label("New Profile Name:");
                ui.text_edit_singleline(&mut self.edit_profile_name);
                
                ui.label("Description (optional):");
                ui.text_edit_singleline(&mut self.edit_profile_desc);
                
                ui.add_space(ITEM_SPACING);
                
                ui.horizontal(|ui| {
                    if ui.button("Duplicate").clicked() {
                        if !self.edit_profile_name.is_empty() {
                            let mut new_profile = config.profiles[source_idx].clone();
                            new_profile.name = self.edit_profile_name.clone();
                            new_profile.description = self.edit_profile_desc.clone();
                            config.profiles.push(new_profile);
                            action = ProfileAction::ProfileCreated;
                            self.show_duplicate_dialog = false;
                        }
                    }
                    
                    if ui.button("Cancel").clicked() {
                        self.show_duplicate_dialog = false;
                    }
                });
            });
        
        action
    }

    fn edit_profile_dialog(
        &mut self,
        ctx: &egui::Context,
        config: &mut Config,
        selected_idx: usize,
    ) -> ProfileAction {
        let mut action = ProfileAction::None;

        egui::Window::new("Edit Profile")
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label("Profile Name:");
                ui.text_edit_singleline(&mut self.edit_profile_name);

                ui.label("Description (optional):");
                ui.text_edit_singleline(&mut self.edit_profile_desc);

                ui.add_space(ITEM_SPACING);

                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() {
                        if !self.edit_profile_name.is_empty() {
                            let profile = &mut config.profiles[selected_idx];
                            profile.name = self.edit_profile_name.clone();
                            profile.description = self.edit_profile_desc.clone();
                            config.global.selected_profile = profile.name.clone();
                            action = ProfileAction::ProfileUpdated;
                            self.show_edit_dialog = false;
                        }
                    }

                    if ui.button("Cancel").clicked() {
                        self.show_edit_dialog = false;
                    }
                });
            });

        action
    }
    
    fn delete_confirm_dialog(
        &mut self,
        ctx: &egui::Context,
        config: &mut Config,
        selected_idx: &mut usize,
    ) -> ProfileAction {
        let mut action = ProfileAction::None;
        
        egui::Window::new("Confirm Delete")
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(format!(
                    "Delete profile '{}'?",
                    config.profiles[*selected_idx].name
                ));
                ui.colored_label(
                    egui::Color32::from_rgb(200, 0, 0),
                    "This cannot be undone!"
                );
                
                ui.add_space(ITEM_SPACING);
                
                ui.horizontal(|ui| {
                    if ui.button("Delete").clicked() {
                        config.profiles.remove(*selected_idx);
                        if *selected_idx >= config.profiles.len() {
                            *selected_idx = config.profiles.len() - 1;
                        }
                        config.global.selected_profile = config.profiles[*selected_idx].name.clone();
                        action = ProfileAction::ProfileDeleted;
                        self.show_delete_confirm = false;
                    }
                    
                    if ui.button("Cancel").clicked() {
                        self.show_delete_confirm = false;
                    }
                });
            });
        
        action
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProfileAction {
    None,
    SwitchProfile,
    ProfileCreated,
    ProfileDeleted,
    ProfileUpdated,
}
