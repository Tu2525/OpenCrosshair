use super::widgets::{
    caption, card, empty_state, ghost, heading, hint, icon_button, name_it, primary, texture, thumb,
};
use super::{App, Semi, pal};
use crate::config::{Crosshair, Mode};
use crate::{overlay, picture, share};
use eframe::egui::{self, Align2, FontId, RichText, Sense, vec2};
use egui_phosphor::bold as icon;
use std::collections::BTreeMap;

impl App {
    pub(super) fn presets_page(&mut self, ui: &mut egui::Ui) {
        heading(
            ui,
            "Presets",
            "Save crosshairs and switch between them. Each game can use its own.",
        );
        card(ui, "Save current crosshair", |ui| {
            ui.horizontal(|ui| {
                let t = texture(
                    &mut self.textures,
                    ui.ctx(),
                    "current",
                    &self.local.crosshair,
                );
                thumb(ui, &t, 38.0);
                ui.add(
                    egui::TextEdit::singleline(&mut self.preset_name)
                        .hint_text("Preset name")
                        .desired_width(240.0),
                );
                let name = self.preset_name.trim().to_string();
                let label = if self.local.presets.contains_key(&name) {
                    "Overwrite"
                } else {
                    "Save"
                };
                if primary(
                    ui,
                    !name.is_empty(),
                    &format!("{}  {label}", icon::FLOPPY_DISK),
                )
                .clicked()
                {
                    let verb = if self.local.presets.contains_key(&name) {
                        "Updated"
                    } else {
                        "Saved"
                    };
                    self.preset_status = Some(format!("{verb} {name}."));
                    self.local
                        .presets
                        .insert(name, self.local.crosshair.clone());
                }
            });
        });

        let mut load = None;
        let mut delete = None;
        let mut export = None;
        let mut export_all = false;
        let mut import = false;
        let mut commit_rename = None;
        card(ui, "", |ui| {
            ui.horizontal(|ui| {
                caption(ui, "Saved");
                // Right to left, so Import goes first to sit to the right of Export all.
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ghost(ui, &format!("{}  Import", icon::DOWNLOAD_SIMPLE)).clicked() {
                        import = true;
                    }
                    if !self.local.presets.is_empty()
                        && ghost(ui, &format!("{}  Export all", icon::EXPORT)).clicked()
                    {
                        export_all = true;
                    }
                });
            });
            ui.add_space(6.0);
            if let Some(s) = &self.preset_status {
                hint(ui, s);
            }
            ui.add_space(4.0);
            if self.local.presets.is_empty() {
                empty_state(
                    ui,
                    icon::BOOKMARKS_SIMPLE,
                    "Nothing saved yet",
                    "Save the crosshair above to keep it, and give each game its own. Or import presets someone shared with you.",
                );
            }
            egui::Grid::new("presets")
                .num_columns(5)
                .spacing([14.0, 10.0])
                .show(ui, |ui| {
                    for (name, c) in &self.local.presets {
                        let t = texture(&mut self.textures, ui.ctx(), &format!("preset:{name}"), c);
                        thumb(ui, &t, 56.0);
                        ui.vertical(|ui| {
                            ui.set_min_width(140.0); // grid cells start narrow; don't wrap names
                            match &mut self.renaming {
                                Some((old, text)) if old == name => {
                                    let r = ui.add(
                                        egui::TextEdit::singleline(&mut *text).desired_width(140.0),
                                    );
                                    if !r.has_focus() && !r.lost_focus() {
                                        r.request_focus(); // once, as it appears
                                    }
                                    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                                        self.renaming = None;
                                    } else if r.lost_focus() {
                                        commit_rename = Some((old.clone(), text.clone()));
                                        self.renaming = None;
                                    }
                                }
                                _ => {
                                    let r = ui.add(
                                        egui::Label::new(RichText::new(name).semi())
                                            .sense(Sense::click()),
                                    );
                                    // Nothing else hints that the name can be clicked.
                                    if r.hovered() {
                                        ui.painter().text(
                                            r.rect.right_center() + vec2(6.0, 0.0),
                                            Align2::LEFT_CENTER,
                                            icon::PENCIL_SIMPLE,
                                            FontId::proportional(13.0),
                                            pal().muted,
                                        );
                                    }
                                    if name_it(r, format!("Rename {name}"))
                                        .on_hover_text("Click to rename")
                                        .clicked()
                                    {
                                        self.renaming = Some((name.clone(), name.clone()));
                                    }
                                }
                            }
                            let kind = match c.mode {
                                Mode::Lines => "Lines".to_string(),
                                Mode::Pixels => format!("Pixel {0}×{0}", c.grid),
                                Mode::Image => "Image".to_string(),
                            };
                            hint(ui, kind);
                        });
                        if self.local.crosshair == *c {
                            // A pill would stretch to this tall row, so plain text instead.
                            ui.label(
                                RichText::new(format!("{}  In use", icon::CHECK))
                                    .color(pal().green.1),
                            );
                        } else if name_it(ui.add(egui::Button::new("Load")), format!("Load {name}"))
                            .clicked()
                        {
                            load = Some(name.clone());
                        }
                        if icon_button(ui, icon::EXPORT, format!("Export {name}"), "Export")
                            .clicked()
                        {
                            export = Some(name.clone());
                        }
                        let armed = self.confirm_delete.as_ref() == Some(name);
                        let text = if armed {
                            format!("{}  Delete?", icon::TRASH)
                        } else {
                            icon::TRASH.to_string()
                        };
                        let colour = if armed { pal().danger } else { pal().muted };
                        let btn = egui::Button::new(RichText::new(text).color(colour))
                            .frame_when_inactive(false);
                        let hover = if armed {
                            "Click again to delete"
                        } else {
                            "Delete"
                        };
                        let r = name_it(
                            ui.add(btn),
                            if armed {
                                format!("Confirm delete {name}")
                            } else {
                                format!("Delete {name}")
                            },
                        );
                        if r.on_hover_text(hover).clicked() {
                            if armed {
                                delete = Some(name.clone());
                            } else {
                                self.confirm_delete = Some(name.clone());
                            }
                        }
                        ui.end_row();
                    }
                });
        });
        if let Some((old, new)) = commit_rename {
            match self.local.rename_preset(&old, &new) {
                Ok(()) => {
                    self.textures.remove(&format!("preset:{old}"));
                    if self.preset_name == old {
                        self.preset_name = new.trim().into();
                    }
                    if self.confirm_delete.as_ref() == Some(&old) {
                        self.confirm_delete = None;
                    }
                }
                Err(e) => self.preset_status = Some(e),
            }
        }
        if let Some(n) = export {
            let one = BTreeMap::from([(n.clone(), self.local.presets[&n].clone())]);
            // Windows won't take these in a file name, and a preset name can hold any of them.
            let file: String = n
                .chars()
                .map(|c| if r#"\/:*?"<>|"#.contains(c) { '_' } else { c })
                .collect();
            self.export_presets(&one, &format!("{file}.opencrosshair"));
        }
        if export_all {
            let all = self.local.presets.clone();
            self.export_presets(&all, "OpenCrosshair presets.opencrosshair");
        }
        if import {
            self.import_presets();
        }
        if let Some(n) = load {
            self.local.crosshair = self.local.presets[&n].clone();
            self.preset_name = n;
        }
        if let Some(n) = delete {
            self.confirm_delete = None;
            self.local.delete_preset(&n);
            self.textures.remove(&format!("preset:{n}"));
        }
    }

    /// Ask where to save, then write the presets to that file.
    fn export_presets(&mut self, presets: &BTreeMap<String, Crosshair>, default_name: &str) {
        let Some(path) = picture::save_share(overlay::settings_window(), default_name) else {
            return;
        };
        self.preset_status = Some(match std::fs::write(&path, share::export(presets)) {
            Ok(()) => "Exported.".into(),
            Err(_) => "Couldn't save that file.".into(),
        });
    }

    /// Ask for a share file and add the presets in it.
    fn import_presets(&mut self) {
        let Some(path) = picture::pick_share(overlay::settings_window()) else {
            return;
        };
        let Ok(text) = std::fs::read_to_string(&path) else {
            self.preset_status = Some("Couldn't read that file.".into());
            return;
        };
        self.preset_status = Some(match share::import(&text, &self.local.presets) {
            Ok(new) => {
                let n = new.len();
                self.local.presets.extend(new);
                format!("Imported {n} preset{}.", if n == 1 { "" } else { "s" })
            }
            Err(e) => e,
        });
    }
}
