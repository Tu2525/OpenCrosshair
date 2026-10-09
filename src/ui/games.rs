use super::widgets::{
    card, check, empty_state, ghost, heading, hint, name_it, primary, row, segment, texture, thumb,
};
use super::{App, Semi, pal};
use crate::apps;
use crate::config::{Aim, AimButton, AimRule, Game};
use eframe::egui::{self, RichText, TextureHandle, vec2};
use egui_phosphor::bold as icon;
use std::collections::HashMap;

/// The three choices that make up an aiming rule. Used for the default and for a game's own.
fn aim_controls(ui: &mut egui::Ui, id: &str, presets: &[String], rule: &mut AimRule) {
    let shown = match &rule.action {
        Aim::Keep => "Keep the crosshair".to_string(),
        Aim::Hide => "Hide it".to_string(),
        Aim::Preset(name) => format!("Switch to {name}"),
    };
    egui::Grid::new(("aim_rule", id))
        .num_columns(2)
        .spacing([20.0, 10.0])
        .show(ui, |ui| {
            let label = ui.label(RichText::new("When you aim").color(pal().muted));
            egui::ComboBox::from_id_salt((id, "action"))
                .selected_text(shown)
                .width(220.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut rule.action, Aim::Keep, "Keep the crosshair");
                    ui.selectable_value(&mut rule.action, Aim::Hide, "Hide it");
                    for name in presets {
                        let choice = format!("Switch to {name}");
                        ui.selectable_value(&mut rule.action, Aim::Preset(name.clone()), choice);
                    }
                })
                .response
                .labelled_by(label.id);
            ui.end_row();

            // With "Keep the crosshair" nothing changes while aiming, so these two do nothing.
            let live = rule.action != Aim::Keep;
            let label = ui.label(RichText::new("Aim button").color(pal().muted));
            ui.add_enabled_ui(live, |ui| {
                egui::ComboBox::from_id_salt((id, "button"))
                    .selected_text(rule.button.label())
                    .width(220.0)
                    .show_ui(ui, |ui| {
                        for b in AimButton::ALL {
                            ui.selectable_value(&mut rule.button, b, b.label());
                        }
                    })
                    .response
                    .labelled_by(label.id);
            });
            ui.end_row();

            row(ui, "", |ui| {
                ui.add_enabled_ui(live, |ui| {
                    check(
                        ui,
                        &mut rule.toggle,
                        "Toggle: press once to aim, press again to stop",
                    );
                });
            });
        });
    if rule.action == Aim::Keep {
        ui.add_space(4.0);
        hint(
            ui,
            "Choose \"Hide it\" or a preset above to turn on the button and toggle.",
        );
    }
}

/// What a rule does, in a few words, for showing the default next to a game that follows it.
fn describe_aim(rule: &AimRule) -> String {
    let button = rule.button.label().to_lowercase();
    let how = if rule.toggle { "toggled" } else { "held" };
    match &rule.action {
        Aim::Keep => "the crosshair never changes while aiming".to_string(),
        Aim::Hide => format!("hide the crosshair while the {button} is {how}"),
        Aim::Preset(name) => format!("switch to {name} while the {button} is {how}"),
    }
}

/// What the games list shows for an app.
pub(super) struct AppInfo {
    name: String,
    icon: Option<TextureHandle>,
    fso: apps::Fso,
}

/// Name and icon for an app, read from its exe once and then cached. Keyed by path, or by the
/// exe name while the path isn't known yet.
fn app_info<'a>(
    cache: &'a mut HashMap<String, AppInfo>,
    ctx: &egui::Context,
    app: &apps::App,
) -> &'a AppInfo {
    let (exe, path) = (app.exe.as_str(), app.path.as_str());
    let key = if path.is_empty() { exe } else { path };
    cache.entry(key.to_string()).or_insert_with(|| AppInfo {
        name: apps::display_name(exe, path, &app.title),
        icon: (!path.is_empty())
            .then(|| apps::icon(path, 48))
            .flatten()
            .map(|(rgba, size)| {
                let image = egui::ColorImage::from_rgba_unmultiplied(size, &rgba);
                ctx.load_texture(format!("icon:{key}"), image, egui::TextureOptions::LINEAR)
            }),
        fso: if path.is_empty() {
            apps::Fso::On
        } else {
            apps::fullscreen_optimizations(path)
        },
    })
}

/// The app's own icon, or a controller when the exe doesn't have one we can read.
fn app_icon(ui: &mut egui::Ui, info: &AppInfo, size: f32) {
    match &info.icon {
        Some(t) => {
            ui.add(egui::Image::from_texture((t.id(), vec2(size, size))));
        }
        None => {
            let glyph = RichText::new(icon::GAME_CONTROLLER)
                .size(size * 0.8)
                .color(pal().muted);
            ui.add_sized([size, size], egui::Label::new(glyph));
        }
    }
}

impl App {
    pub(super) fn games_page(&mut self, ui: &mut egui::Ui) {
        heading(
            ui,
            "Games",
            "Show the crosshair only in the games you choose.",
        );
        // Games added by typing their exe name get a proper name and icon once they've been seen
        // running.
        for g in &mut self.local.games {
            if let Some(app) = self.running.iter().find(|a| a.exe == g.exe) {
                if g.path.is_empty() {
                    g.path = app.path.clone();
                }
                if g.name.is_empty() {
                    g.name = apps::display_name(&app.exe, &app.path, &app.title);
                }
            }
        }
        let s = &mut self.local;

        card(ui, "", |ui| {
            check(
                ui,
                &mut s.only_games,
                RichText::new("Only show in my games").semi(),
            );
            let note = if s.only_games {
                "Hidden in every other app. It still shows while this window is focused, so you can preview."
            } else {
                "The crosshair currently draws on top of everything."
            };
            hint(ui, note);
        });

        // The default rule, which every game follows unless it has its own (see its row below).
        let default_rule = AimRule {
            action: s.aim.clone(),
            button: s.aim_button,
            toggle: s.aim_toggle,
        };
        let preset_names: Vec<String> = s.presets.keys().cloned().collect();

        card(ui, "My games", |ui| {
            if s.games.is_empty() {
                empty_state(
                    ui,
                    icon::GAME_CONTROLLER,
                    "No games yet",
                    "Add one below and the crosshair will only show while you're playing it.",
                );
            }
            let mut remove = None;
            for (i, g) in s.games.iter_mut().enumerate() {
                let app = apps::App {
                    exe: g.exe.clone(),
                    path: g.path.clone(),
                    title: String::new(),
                };
                let info = app_info(&mut self.apps, ui.ctx(), &app);
                let name = if g.name.is_empty() {
                    &info.name
                } else {
                    &g.name
                };
                let open = self.open_game.as_deref() == Some(g.exe.as_str());
                ui.horizontal_wrapped(|ui| {
                    // On a narrow window whole controls move to a second line; the words in them
                    // don't break ("Windowe / d").
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                    // Tighter than the default, so a row still fits a ~1100 px wide window.
                    ui.spacing_mut().item_spacing.x = 6.0;
                    ui.spacing_mut().button_padding.x = 6.0;
                    // Fixed width, so every row lines up whatever the game is called.
                    let name_cell = vec2(150.0, 34.0);
                    let cell = egui::Layout::left_to_right(egui::Align::Center);
                    ui.allocate_ui_with_layout(name_cell, cell, |ui| {
                        ui.set_min_size(name_cell); // otherwise it shrinks to fit the name
                        app_icon(ui, info, 24.0);
                        let full = if g.path.is_empty() { &g.exe } else { &g.path };
                        ui.add(egui::Label::new(RichText::new(name).semi()).truncate())
                            .on_hover_text(full);
                    });
                    let c = s.presets.get(&g.preset).unwrap_or(&s.crosshair);
                    let t = texture(&mut self.textures, ui.ctx(), &format!("game:{}", g.exe), c);
                    thumb(ui, &t, 34.0);
                    let shown = if g.preset.is_empty() {
                        "Current crosshair"
                    } else {
                        g.preset.as_str()
                    };
                    egui::ComboBox::from_id_salt(("preset", i))
                        .selected_text(shown)
                        .width(122.0)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut g.preset, String::new(), "Current crosshair");
                            for name in s.presets.keys() {
                                ui.selectable_value(&mut g.preset, name.clone(), name);
                            }
                        });
                    // "Windowed", not a longer label: the row has to fit an ~1100 px window.
                    let windowed = check(ui, &mut g.windowed, "Windowed").on_hover_text(
                        "Only for games you play in a window: centres the crosshair on the \
                         game's window instead of the middle of the screen.",
                    );
                    // Every row says "Windowed", so say which game each belongs to.
                    let (on, label) = (g.windowed, format!("Windowed: {name}"));
                    windowed.widget_info(|| {
                        egui::WidgetInfo::selected(
                            egui::WidgetType::Checkbox,
                            true,
                            on,
                            label.clone(),
                        )
                    });
                    // This game's own aiming rule: accent when it has one, filled while open.
                    let custom = g.aim.is_some();
                    let ink = if custom { pal().accent } else { pal().muted };
                    let caret = if open {
                        icon::CARET_UP
                    } else {
                        icon::CARET_DOWN
                    };
                    let aim_text = if custom { "Custom aim" } else { "Default aim" };
                    let text = format!("{aim_text} {caret}");
                    let btn = egui::Button::new(RichText::new(text).color(ink))
                        .frame_when_inactive(open)
                        .min_size(vec2(114.0, 0.0)); // same width either way, so the ✕ lines up
                    let tip = if custom {
                        "This game has its own aiming rule"
                    } else {
                        "Aiming settings for this game"
                    };
                    let btn = name_it(ui.add(btn), format!("Aiming settings for {name}"));
                    if btn.on_hover_text(tip).clicked() {
                        self.open_game = if open { None } else { Some(g.exe.clone()) };
                    }
                    if name_it(ghost(ui, icon::X), format!("Remove {name}"))
                        .on_hover_text("Remove")
                        .clicked()
                    {
                        remove = Some(i);
                    }
                });
                ui.add_space(2.0); // separates a wrapped row from the next one
                if open {
                    egui::Frame::new()
                        .fill(pal().field)
                        .corner_radius(8)
                        .inner_margin(egui::Margin::symmetric(16, 14))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.label(RichText::new(format!("Aiming in {name}")).semi());
                            hint(
                                ui,
                                "What a mouse button does to the crosshair in this game. \
                                 Choose \"Keep the crosshair\" to make it ignore aiming here.",
                            );
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                let default = vec2(140.0, 30.0);
                                if segment(ui, "Use my default".into(), g.aim.is_none(), default)
                                    .clicked()
                                {
                                    g.aim = None;
                                }
                                let own = vec2(180.0, 30.0);
                                if segment(ui, "Custom for this game".into(), g.aim.is_some(), own)
                                    .clicked()
                                    && g.aim.is_none()
                                {
                                    g.aim = Some(default_rule.clone()); // start from the default
                                }
                            });
                            match &mut g.aim {
                                Some(rule) => aim_controls(ui, &g.exe, &preset_names, rule),
                                None => {
                                    hint(
                                        ui,
                                        format!(
                                            "Using your default: {}.",
                                            describe_aim(&default_rule)
                                        ),
                                    );
                                }
                            }
                        });
                    ui.add_space(6.0);
                }
            }
            if let Some(i) = remove {
                if self.open_game.as_deref() == Some(s.games[i].exe.as_str()) {
                    self.open_game = None;
                }
                self.removed_game = Some((i, s.games.remove(i)));
            }
            // Fullscreen optimizations switched off means true exclusive fullscreen, where no
            // overlay can show. Say so, and fix it when it's our account's setting.
            for g in s.games.iter().filter(|g| !g.path.is_empty()) {
                let Some(info) = self.apps.get_mut(&g.path) else {
                    continue;
                };
                if info.fso == apps::Fso::On {
                    continue;
                }
                let name = if g.name.is_empty() {
                    &info.name
                } else {
                    &g.name
                };
                ui.add_space(4.0);
                ui.label(
                    RichText::new(format!(
                        "{}  {name} has fullscreen optimizations turned off, so in fullscreen nothing can draw over it.",
                        icon::WARNING
                    ))
                    .small()
                    .color(pal().yellow.1),
                );
                if info.fso == apps::Fso::OffForEveryone {
                    hint(
                        ui,
                        "It's set for every account. Untick \"Disable fullscreen optimizations\" \
                         in the game's Properties > Compatibility tab (needs admin), or play borderless.",
                    );
                } else if ghost(ui, &format!("{}  Turn them back on", icon::WRENCH)).clicked()
                    && apps::enable_fullscreen_optimizations(&g.path)
                {
                    info.fso = apps::Fso::On;
                }
            }
            // A removal is one click, so offer one line to take it back.
            let mut undo = false;
            if let Some((_, g)) = &self.removed_game {
                let name = if g.name.is_empty() {
                    apps::display_name(&g.exe, "", "")
                } else {
                    g.name.clone()
                };
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    hint(ui, format!("Removed {name}."));
                    undo = name_it(ghost(ui, "Undo"), format!("Undo removing {name}")).clicked();
                });
            }
            if undo && let Some((i, g)) = self.removed_game.take() {
                s.games.insert(i.min(s.games.len()), g);
            }
        });

        // Says whether it added a game, so pressing Enter on an empty box keeps the Undo line.
        let add = |games: &mut Vec<Game>, exe: &str, path: &str, name: &str| -> bool {
            let exe = exe.trim().to_lowercase();
            // "cs2" works as well as "cs2.exe".
            let exe = if exe.is_empty() || exe.ends_with(".exe") {
                exe
            } else {
                format!("{exe}.exe")
            };
            let new = !exe.is_empty() && !games.iter().any(|g| g.exe == exe);
            if new {
                games.push(Game {
                    exe,
                    name: name.to_string(),
                    path: path.to_string(),
                    ..Default::default()
                });
            }
            new
        };
        card(ui, "Add a game", |ui| {
            ui.horizontal(|ui| {
                let mut picked = None;
                egui::ComboBox::from_id_salt("running")
                    .selected_text("Pick a running app…")
                    .width(260.0)
                    .show_ui(ui, |ui| {
                        for (i, app) in self.running.iter().enumerate() {
                            let info = app_info(&mut self.apps, ui.ctx(), app);
                            let btn = match &info.icon {
                                Some(t) => egui::Button::image_and_text(
                                    egui::Image::from_texture((t.id(), vec2(18.0, 18.0))),
                                    info.name.as_str(),
                                ),
                                None => egui::Button::new(info.name.as_str()),
                            };
                            let btn = btn.frame_when_inactive(false).min_size(vec2(240.0, 0.0));
                            if ui.add(btn).on_hover_text(&app.exe).clicked() {
                                picked = Some(i);
                            }
                        }
                    });
                if let Some(app) = picked.map(|i| &self.running[i]) {
                    let name = app_info(&mut self.apps, ui.ctx(), app).name.clone();
                    if add(&mut s.games, &app.exe, &app.path, &name) {
                        self.removed_game = None;
                    }
                }
                if ghost(ui, &format!("{}  Refresh", icon::ARROWS_CLOCKWISE)).clicked() {
                    self.running = apps::running();
                }
            });
            ui.horizontal(|ui| {
                let r = ui.add(
                    egui::TextEdit::singleline(&mut self.game_input)
                        .hint_text("or type its exe name, e.g. cs2")
                        .desired_width(260.0),
                );
                let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if primary(
                    ui,
                    !self.game_input.trim().is_empty(),
                    &format!("{}  Add", icon::PLUS),
                )
                .clicked()
                    || enter
                {
                    if add(&mut s.games, &self.game_input, "", "") {
                        self.removed_game = None;
                    }
                    self.game_input.clear();
                }
            });
            hint(ui, "Open the game first, then press Refresh to find it.");
        });

        card(ui, "While aiming", |ui| {
            let mut rule = default_rule.clone();
            aim_controls(ui, "default", &preset_names, &mut rule);
            (s.aim, s.aim_button, s.aim_toggle) = (rule.action, rule.button, rule.toggle);
            hint(
                ui,
                "For games where you aim down sights. It reads the button's state, with no input hooks.",
            );
            hint(
                ui,
                "This is the default for every game. To give one game its own, use Default aim on its row in My games.",
            );
        });
    }
}
