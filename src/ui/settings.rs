use super::widgets::{card, check, ghost, heading, hint, name_it, segment};
use super::{App, Semi, pal};
use crate::config::Theme;
use crate::overlay::{self, KEY_OK};
use crate::{install, update};
use eframe::egui::{self, Align2, FontId, RichText, Sense, Stroke, StrokeKind, vec2};
use egui_phosphor::bold as icon;
use std::sync::atomic::Ordering;

/// Keyboard key rendered as a keycap.
fn keycap(ui: &mut egui::Ui, text: &str, active: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(190.0, 34.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, text));
    let p = pal();
    let (fill, stroke) = if active {
        (p.accent.gamma_multiply(0.18), p.accent)
    } else if resp.is_pointer_button_down_on() {
        (p.press, p.border)
    } else if resp.hovered() {
        (p.hover, p.border)
    } else {
        (p.field, p.border)
    };
    let painter = ui.painter();
    painter.rect(rect, 6, fill, Stroke::new(1.0, stroke), StrokeKind::Inside);
    if resp.has_focus() {
        painter.rect_stroke(rect, 6, Stroke::new(1.5, p.accent), StrokeKind::Inside);
    }
    // Monospace fonts reserve more room below the baseline, which makes centred text look high.
    let nudge = vec2(0.0, 2.5);
    painter.text(
        rect.center() + nudge,
        Align2::CENTER_CENTER,
        text,
        FontId::monospace(13.5),
        p.text,
    );
    resp
}

impl App {
    pub(super) fn settings_page(&mut self, ui: &mut egui::Ui) {
        heading(ui, "Settings", "Appearance, hotkeys, startup and updates.");
        if let Some(i) = self.listening {
            ui.ctx().request_repaint(); // keep polling while waiting for a key
            if let Some(hk) = overlay::capture_key() {
                if let Some(hk) = hk {
                    *[&mut self.local.toggle_key, &mut self.local.menu_key][i] = hk;
                }
                self.listening = None;
            }
        }
        card(ui, "Appearance", |ui| {
            // Wraps onto a second line when the text size leaves no room for three across.
            ui.horizontal_wrapped(|ui| {
                for (t, ic, name) in [
                    (Theme::System, icon::MONITOR, "System"),
                    (Theme::Light, icon::SUN, "Light"),
                    (Theme::Dark, icon::MOON, "Dark"),
                ] {
                    let on = self.local.theme == t;
                    if segment(ui, format!("{ic}  {name}"), on, vec2(110.0, 34.0)).clicked() {
                        self.local.theme = t;
                    }
                }
            });
            hint(ui, "System follows Windows' light or dark app mode.");
        });
        card(ui, "Keybinds", |ui| {
            egui::Grid::new("keys")
                .num_columns(4)
                .spacing([20.0, 12.0])
                .show(ui, |ui| {
                    for (i, label) in ["Toggle crosshair", "Show / hide this window"]
                        .into_iter()
                        .enumerate()
                    {
                        let action = ui.label(label);
                        let hk = [self.local.toggle_key, self.local.menu_key][i];
                        let listening = self.listening == Some(i);
                        let text = if listening {
                            "Press a key…".into()
                        } else {
                            overlay::key_name(hk)
                        };
                        // "Toggle crosshair, F8" rather than a button that only says "F8".
                        if keycap(ui, &text, listening)
                            .labelled_by(action.id)
                            .clicked()
                        {
                            self.listening = Some(i);
                        }
                        if name_it(ghost(ui, icon::X), format!("Unbind {label}"))
                            .on_hover_text("Unbind")
                            .clicked()
                        {
                            *[&mut self.local.toggle_key, &mut self.local.menu_key][i] =
                                Default::default();
                        }
                        let same = hk.vk != 0 && self.local.toggle_key == self.local.menu_key;
                        if same || !KEY_OK[i].load(Ordering::Relaxed) {
                            let why = if same {
                                "Both actions use this key"
                            } else {
                                "Another app is using this key"
                            };
                            ui.label(
                                RichText::new(format!("{}  {why}", icon::WARNING))
                                    .small()
                                    .color(pal().yellow.1),
                            );
                        }
                        ui.end_row();
                    }
                });
            hint(
                ui,
                format!(
                    "{}  Click a key, then press any key or combo. Esc cancels.",
                    icon::KEYBOARD
                ),
            );
        });
        card(ui, "Startup", |ui| {
            ui.horizontal(|ui| {
                if check(
                    ui,
                    &mut self.autostart,
                    RichText::new("Start with Windows").semi(),
                )
                .changed()
                {
                    install::set_autostart(self.autostart);
                    self.autostart = install::autostart(); // show what actually got written
                }
            });
            // An unbound key would read "Press Not set", so leave it out.
            let how = if self.local.menu_key.vk == 0 {
                "Click the tray icon for settings.".to_string()
            } else {
                format!(
                    "Press {} or click the tray icon for settings.",
                    overlay::key_name(self.local.menu_key)
                )
            };
            hint(
                ui,
                format!(
                    "{}  Starts quietly when you log in, in the tray. {how}",
                    icon::ROCKET_LAUNCH
                ),
            );
        });
        card(ui, "Updates", |ui| {
            ui.horizontal(|ui| {
                check(
                    ui,
                    &mut self.local.auto_update,
                    RichText::new("Install updates automatically").semi(),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new(format!("v{}", update::VERSION))
                            .monospace()
                            .color(pal().muted),
                    );
                });
            });
            let (text, busy) = match update::STATUS.lock().unwrap().clone() {
                update::Status::Idle => ("Hasn't checked yet.".to_string(), false),
                update::Status::DevBuild => {
                    ("Updates are off in development builds.".to_string(), true)
                }
                update::Status::Checking => ("Checking GitHub…".to_string(), true),
                update::Status::UpToDate => ("You're on the latest version.".to_string(), false),
                update::Status::Downloading(v) => (format!("Downloading {v}…"), true),
                update::Status::WaitingForGame(v) => (
                    format!("{v} is ready and installs once you leave your game."),
                    true,
                ),
                update::Status::Failed(e) => (format!("Last check failed: {e}"), false),
            };
            ui.horizontal(|ui| {
                let check = egui::Button::new(format!("{}  Check now", icon::ARROWS_CLOCKWISE));
                if ui.add_enabled(!busy, check).clicked() {
                    let _ = self.updates.send(());
                }
                hint(ui, text);
            });
            hint(
                ui,
                "Updates come from the project's GitHub releases. It never restarts while you're in a game.",
            );
        });
        card(ui, "About", |ui| {
            let repo = env!("CARGO_PKG_REPOSITORY");
            hint(ui, "Free and open source, under the MIT license.");
            ui.horizontal(|ui| {
                if ghost(ui, &format!("{}  Source on GitHub", icon::GITHUB_LOGO)).clicked() {
                    install::open_url(repo);
                }
                if ghost(ui, &format!("{}  Report a problem", icon::BUG)).clicked() {
                    install::open_url(&format!("{repo}/issues/new"));
                }
            });
        });
    }
}
