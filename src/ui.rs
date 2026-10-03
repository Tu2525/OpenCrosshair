use crate::config::{Crosshair, Game, MAX_GRID, MAX_SCALE, Mode, Settings};
use crate::overlay::{self, KEY_OK, Status, VISIBLE};
use crate::{install, render, update};
use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Painter, Pos2, Rect, RichText, Sense, Slider,
    Stroke, StrokeKind, TextureHandle, pos2, vec2,
};
use egui_phosphor::bold as icon;
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

// Warm monochrome; colour only where it means something.
const BG: Color32 = Color32::from_rgb(18, 18, 17);
const SIDE: Color32 = Color32::from_rgb(23, 23, 22);
const FIELD: Color32 = Color32::from_rgb(34, 34, 32);
const HOVER: Color32 = Color32::from_rgb(44, 44, 42);
const BORDER: Color32 = Color32::from_rgb(42, 42, 40);
const TEXT: Color32 = Color32::from_rgb(232, 230, 225);
const MUTED: Color32 = Color32::from_rgb(132, 130, 125);
const ACCENT: Color32 = Color32::from_rgb(150, 205, 160); // muted pale green
const GREEN: (Color32, Color32) = (
    Color32::from_rgb(30, 46, 34),
    Color32::from_rgb(150, 205, 160),
);
const YELLOW: (Color32, Color32) = (
    Color32::from_rgb(52, 44, 24),
    Color32::from_rgb(222, 190, 110),
);
const BLUE: (Color32, Color32) = (
    Color32::from_rgb(26, 40, 54),
    Color32::from_rgb(130, 180, 225),
);
// Layout: page column width, and the preview column that appears beside it on wide windows.
const COLUMN: f32 = 640.0;
const SIDE_PREVIEW: f32 = 400.0;
const GUTTER: f32 = 32.0;
const SCROLL_GAP: f32 = 24.0;
const GRAY: (Color32, Color32) = (
    Color32::from_rgb(38, 38, 36),
    Color32::from_rgb(150, 148, 142),
);

pub fn run(
    shared: Arc<Mutex<Settings>>,
    updates: mpsc::Sender<()>,
    hidden: bool,
) -> eframe::Result {
    let local = shared.lock().unwrap().clone();
    // Drawn by build.rs, same artwork as the exe icon.
    let icon = egui::IconData {
        rgba: include_bytes!(concat!(env!("OUT_DIR"), "/icon-64.rgba")).to_vec(),
        width: 64,
        height: 64,
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(overlay::TITLE) // overlay.rs finds the window by this title
            .with_icon(icon)
            .with_inner_size([880.0, 700.0])
            .with_min_inner_size([720.0, 540.0]),
        ..Default::default()
    };
    eframe::run_native(
        overlay::TITLE,
        options,
        Box::new(|cc| {
            style(&cc.egui_ctx);
            Ok(Box::new(App {
                shared,
                local,
                page: Page::Crosshair,
                listening: None,
                brush: [255, 255, 255, 255],
                mirror_x: true,
                mirror_y: true,
                preset_name: String::new(),
                game_input: String::new(),
                running: overlay::running_apps(),
                textures: HashMap::new(),
                autostart: install::autostart(),
                updates,
                start_hidden: hidden,
                last_cell: None,
                undo: Vec::new(),
                confirm_delete: None,
                dirty: false,
            }))
        }),
    )
}

fn style(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Bold);
    ctx.set_fonts(fonts);

    let mut v = egui::Visuals::dark();
    v.panel_fill = BG;
    v.window_fill = SIDE;
    v.window_stroke = Stroke::new(1.0, BORDER);
    v.extreme_bg_color = FIELD;
    v.override_text_color = Some(TEXT);
    v.selection.bg_fill = ACCENT.gamma_multiply(0.35);
    v.selection.stroke = Stroke::new(1.0, ACCENT);
    v.slider_trailing_fill = true;
    for (w, fill) in [
        (&mut v.widgets.inactive, FIELD),
        (&mut v.widgets.hovered, HOVER),
        (&mut v.widgets.active, HOVER),
        (&mut v.widgets.open, HOVER),
    ] {
        w.corner_radius = CornerRadius::same(6);
        w.bg_fill = fill;
        w.weak_bg_fill = fill;
        w.bg_stroke = Stroke::NONE;
    }
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, BORDER);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
    ctx.set_visuals(v);
    ctx.global_style_mut(|s| {
        s.spacing.item_spacing = vec2(10.0, 9.0);
        s.spacing.button_padding = vec2(12.0, 6.0);
        s.spacing.slider_width = 260.0;
        s.spacing.interact_size.y = 28.0;
        use egui::TextStyle::*;
        s.text_styles.insert(Heading, FontId::proportional(26.0));
        s.text_styles.insert(Body, FontId::proportional(14.5));
        s.text_styles.insert(Button, FontId::proportional(14.5));
        s.text_styles.insert(Small, FontId::proportional(11.5));
        s.text_styles.insert(Monospace, FontId::monospace(13.0));
    });
}

#[derive(PartialEq, Clone, Copy)]
enum Page {
    Crosshair,
    Games,
    Presets,
    Settings,
}

struct App {
    shared: Arc<Mutex<Settings>>,
    local: Settings,
    page: Page,
    listening: Option<usize>, // 0 = toggle key, 1 = menu key
    brush: [u8; 4],
    mirror_x: bool,
    mirror_y: bool,
    preset_name: String,
    game_input: String,
    running: Vec<String>,
    /// Rendered crosshairs for the preview and preset thumbnails, re-rendered only on change.
    textures: HashMap<String, (TextureHandle, Crosshair)>,
    autostart: bool,
    updates: mpsc::Sender<()>,
    start_hidden: bool,
    /// Canvas cell painted last frame, so a fast drag fills the cells in between.
    last_cell: Option<(isize, isize)>,
    /// Canvas snapshots (grid size, pixels) for Ctrl+Z.
    undo: Vec<(u32, Vec<[u8; 4]>)>,
    /// Preset whose delete button was clicked once; a second click deletes it.
    confirm_delete: Option<String>,
    dirty: bool,
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // eframe shows the window after its first frame no matter what, but it applies viewport
        // commands right after that, so hiding it here means it never reaches the screen.
        if std::mem::take(&mut self.start_hidden) {
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }
        // Status (focused game, update progress) changes outside the UI, so poll it while
        // someone can see it. Hidden or minimized, the UI doesn't run at all.
        if overlay::settings_visible()
            && !ui.ctx().input(|i| i.viewport().minimized.unwrap_or(false))
        {
            ui.ctx().request_repaint_after(Duration::from_millis(500));
        }
        let before = self.local.clone();

        egui::Panel::left("nav")
            .exact_size(210.0)
            .resizable(false)
            .show_separator_line(false)
            .frame(
                egui::Frame::new()
                    .fill(SIDE)
                    .stroke(Stroke::new(1.0, BORDER))
                    .inner_margin(18),
            )
            .show(ui, |ui| self.nav(ui));
        let margin = egui::Margin {
            left: 24,
            right: 8,
            top: 26,
            bottom: 0,
        };
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(BG).inner_margin(margin))
            .show(ui, |ui| {
                let full = ui.available_width();
                // On a wide window the Crosshair page gets the preview in its own column, so it
                // stays in view while you scroll through the controls.
                let side = self.page == Page::Crosshair
                    && full >= COLUMN + SCROLL_GAP + GUTTER + SIDE_PREVIEW + 48.0;
                let column = (full - SCROLL_GAP).min(COLUMN);
                let group = column + SCROLL_GAP + if side { GUTTER + SIDE_PREVIEW } else { 0.0 };
                // Centre the content in whatever room the window gives it.
                let indent = ((full - group) / 2.0).floor().max(0.0);
                ui.horizontal_top(|ui| {
                    ui.add_space(indent);
                    let height = ui.available_height();
                    ui.allocate_ui_with_layout(
                        vec2(column + SCROLL_GAP, height),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            egui::ScrollArea::vertical()
                                .auto_shrink(false)
                                .show(ui, |ui| {
                                    // Keep clear of the scrollbar and leave room at the bottom.
                                    let pad = egui::Margin {
                                        left: 0,
                                        right: SCROLL_GAP as i8,
                                        top: 0,
                                        bottom: 26,
                                    };
                                    egui::Frame::new().inner_margin(pad).show(ui, |ui| {
                                        ui.set_width(column);
                                        match self.page {
                                            Page::Crosshair => self.crosshair_page(ui, !side),
                                            Page::Games => self.games_page(ui),
                                            Page::Presets => self.presets_page(ui),
                                            Page::Settings => self.settings_page(ui),
                                        }
                                    });
                                });
                        },
                    );
                    if side {
                        ui.add_space(GUTTER);
                        ui.vertical(|ui| {
                            ui.add_space(78.0); // line up with the first card, below the heading
                            let size = vec2(SIDE_PREVIEW, SIDE_PREVIEW * 1.3);
                            self.preview(ui, size, true);
                        });
                    }
                });
            });

        if self.local != before {
            *self.shared.lock().unwrap() = self.local.clone();
            overlay::refresh();
            self.dirty = true;
        }
        // Save once the user lets go, not on every slider tick / painted cell.
        if self.dirty && !ui.ctx().input(|i| i.pointer.any_down()) {
            self.local.save();
            self.dirty = false;
        }
    }
}

// ---------- small building blocks ----------

/// Flat card: hairline border, no fill change, small uppercase caption.
fn card<R>(ui: &mut egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let r = egui::Frame::new()
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(10)
        .inner_margin(20)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            if !title.is_empty() {
                ui.label(
                    RichText::new(title.to_uppercase())
                        .small()
                        .strong()
                        .color(MUTED)
                        .extra_letter_spacing(1.0),
                );
                ui.add_space(4.0);
            }
            add(ui)
        })
        .inner;
    ui.add_space(14.0);
    r
}

fn row(ui: &mut egui::Ui, label: &str, add: impl FnOnce(&mut egui::Ui)) {
    ui.label(RichText::new(label).color(MUTED));
    add(ui);
    ui.end_row();
}

fn slider(ui: &mut egui::Ui, label: &str, v: &mut u32, range: std::ops::RangeInclusive<u32>) {
    row(ui, label, |ui| {
        ui.add(Slider::new(v, range));
    });
}

fn heading(ui: &mut egui::Ui, title: &str, sub: &str) {
    ui.label(RichText::new(title).heading().strong());
    ui.label(RichText::new(sub).color(MUTED));
    ui.add_space(18.0);
}

/// Solid light button with dark text: the one primary action on a card.
fn primary(ui: &mut egui::Ui, enabled: bool, text: &str) -> egui::Response {
    ui.add_enabled(
        enabled,
        egui::Button::new(RichText::new(text).color(BG).strong()).fill(TEXT),
    )
}

fn ghost(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add(egui::Button::new(RichText::new(text).color(MUTED)).fill(Color32::TRANSPARENT))
}

/// Small uppercase pill.
fn badge(ui: &mut egui::Ui, text: &str, (bg, fg): (Color32, Color32)) {
    egui::Frame::new()
        .fill(bg)
        .corner_radius(99)
        .inner_margin(egui::Margin::symmetric(9, 3))
        .show(ui, |ui| {
            ui.label(
                RichText::new(text.to_uppercase())
                    .small()
                    .strong()
                    .color(fg)
                    .extra_letter_spacing(0.6),
            );
        });
}

/// Keyboard key rendered as a keycap.
fn keycap(ui: &mut egui::Ui, text: &str, active: bool) -> egui::Response {
    let (fill, stroke) = if active {
        (ACCENT.gamma_multiply(0.18), ACCENT)
    } else {
        (FIELD, BORDER)
    };
    ui.add(
        egui::Button::new(RichText::new(text).monospace().strong())
            .fill(fill)
            .stroke(Stroke::new(1.0, stroke))
            .min_size(vec2(190.0, 34.0)),
    )
}

/// Crosshair texture for `key`, re-rendered only when the crosshair changed.
fn texture(
    cache: &mut HashMap<String, (TextureHandle, Crosshair)>,
    ctx: &egui::Context,
    key: &str,
    c: &Crosshair,
) -> TextureHandle {
    if let Some((t, of)) = cache.get(key)
        && of == c
    {
        return t.clone();
    }
    let img = render::render(c);
    let rgba: Vec<u8> = img
        .px
        .iter()
        .flat_map(|p| {
            let [b, g, r, a] = p.to_le_bytes();
            [r, g, b, a]
        })
        .collect();
    let ci = egui::ColorImage::from_rgba_premultiplied([img.w, img.h], &rgba);
    let t = ctx.load_texture(key, ci, egui::TextureOptions::NEAREST);
    cache.insert(key.to_string(), (t.clone(), c.clone()));
    t
}

/// Draw a texture centred at `at`, integer-zoomed to fit `max` (or shrunk if too big).
fn draw_fit(p: &Painter, t: &TextureHandle, at: Pos2, max: f32) -> f32 {
    let size = t.size_vec2();
    let fit = max / size.x.max(size.y);
    let zoom = if fit >= 1.0 {
        fit.floor().min(6.0)
    } else {
        fit
    };
    let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
    p.image(
        t.id(),
        Rect::from_center_size(pos2(at.x.round(), at.y.round()), size * zoom),
        uv,
        Color32::WHITE,
    );
    zoom
}

/// Small dark tile with the crosshair in it.
fn thumb(ui: &mut egui::Ui, t: &TextureHandle, size: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 6, Color32::from_rgb(10, 10, 10));
    p.rect_stroke(rect, 6, Stroke::new(1.0, BORDER), StrokeKind::Inside);
    draw_fit(&p, t, rect.center(), size - 10.0);
}

// ---------- pages ----------

impl App {
    fn nav(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new(icon::CROSSHAIR).size(22.0).color(ACCENT));
            ui.label(RichText::new("OpenCrosshair").size(18.0).strong());
        });
        ui.add_space(22.0);
        for (p, ic, name) in [
            (Page::Crosshair, icon::CROSSHAIR_SIMPLE, "Crosshair"),
            (Page::Games, icon::GAME_CONTROLLER, "Games"),
            (Page::Presets, icon::BOOKMARKS_SIMPLE, "Presets"),
            (Page::Settings, icon::GEAR_SIX, "Settings"),
        ] {
            let on = self.page == p;
            let text = RichText::new(format!("{ic}    {name}"))
                .size(15.0)
                .color(if on { TEXT } else { MUTED });
            let btn = egui::Button::new(text)
                .fill(if on { FIELD } else { Color32::TRANSPARENT })
                .min_size(vec2(ui.available_width(), 38.0));
            if ui.add(btn).clicked() {
                self.page = p;
                self.listening = None;
                self.confirm_delete = None;
            }
        }

        ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
            let toggle = overlay::key_name(self.local.toggle_key);
            let (badge_text, tone, detail) = match overlay::STATUS.lock().unwrap().clone() {
                Status::InGame(exe) => ("In game", GREEN, exe),
                Status::Waiting => ("Waiting", YELLOW, "No game in focus".to_string()),
                Status::Hidden => ("Hidden", GRAY, format!("{toggle} shows it again")),
                Status::Everywhere => ("Everywhere", BLUE, "On top of all apps".to_string()),
            };
            ui.label(RichText::new(detail).small().color(MUTED));
            badge(ui, badge_text, tone);
            ui.add_space(6.0);

            let vis = VISIBLE.load(Ordering::Relaxed);
            let btn = if vis {
                egui::Button::new(
                    RichText::new(format!("{}   Overlay on", icon::POWER))
                        .strong()
                        .color(BG),
                )
                .fill(TEXT)
            } else {
                egui::Button::new(
                    RichText::new(format!("{}   Overlay off", icon::POWER))
                        .strong()
                        .color(MUTED),
                )
                .fill(FIELD)
            };
            let hint = format!("Toggle: {toggle}");
            if ui
                .add(btn.min_size(vec2(ui.available_width(), 40.0)))
                .on_hover_text(hint)
                .clicked()
            {
                VISIBLE.store(!vis, Ordering::Relaxed);
                overlay::refresh();
            }
        });
    }

    fn crosshair_page(&mut self, ui: &mut egui::Ui, inline_preview: bool) {
        heading(
            ui,
            "Crosshair",
            "Changes show on the overlay as you make them.",
        );
        if inline_preview {
            self.preview(ui, vec2(ui.available_width(), 210.0), false);
        }

        let c = &mut self.local.crosshair;
        ui.horizontal(|ui| {
            for (m, ic, name) in [
                (Mode::Lines, icon::CROSSHAIR_SIMPLE, "Lines"),
                (Mode::Pixels, icon::PAINT_BRUSH, "Pixel canvas"),
            ] {
                let on = c.mode == m;
                let btn = egui::Button::new(
                    RichText::new(format!("{ic}  {name}"))
                        .color(if on { BG } else { MUTED })
                        .strong(),
                )
                .fill(if on { TEXT } else { FIELD })
                .min_size(vec2(150.0, 34.0));
                if ui.add(btn).clicked() {
                    c.mode = m;
                }
            }
        });
        ui.add_space(14.0);

        match c.mode {
            Mode::Lines => lines_ui(ui, c),
            Mode::Pixels => self.pixels_ui(ui),
        }

        let c = &mut self.local.crosshair;
        card(ui, "Position", |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Offset").color(MUTED));
                ui.add(egui::DragValue::new(&mut c.offset_x).prefix("x  "));
                ui.add(egui::DragValue::new(&mut c.offset_y).prefix("y  "));
                if ghost(ui, &format!("{}  Reset", icon::ARROW_COUNTER_CLOCKWISE)).clicked() {
                    (c.offset_x, c.offset_y) = (0, 0);
                }
            });
        });
    }

    /// Crosshair over a dark and a light background, so you can judge contrast on both.
    /// Side by side normally, stacked when it has its own column on a wide window.
    fn preview(&mut self, ui: &mut egui::Ui, size: egui::Vec2, stacked: bool) {
        let t = texture(
            &mut self.textures,
            ui.ctx(),
            "current",
            &self.local.crosshair,
        );
        let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
        let p = ui.painter_at(rect);
        let round = |nw, ne, sw, se| CornerRadius { nw, ne, sw, se };
        let (dark, light, dark_round, light_round) = if stacked {
            let mid = rect.center().y.round();
            (
                Rect::from_min_max(rect.min, pos2(rect.max.x, mid)),
                Rect::from_min_max(pos2(rect.min.x, mid), rect.max),
                round(10, 10, 0, 0),
                round(0, 0, 10, 10),
            )
        } else {
            let mid = rect.center().x.round();
            (
                Rect::from_min_max(rect.min, pos2(mid, rect.max.y)),
                Rect::from_min_max(pos2(mid, rect.min.y), rect.max),
                round(10, 0, 10, 0),
                round(0, 10, 0, 10),
            )
        };
        p.rect_filled(dark, dark_round, Color32::from_rgb(8, 8, 8));
        p.rect_filled(light, light_round, Color32::from_rgb(196, 192, 184));
        p.rect_stroke(rect, 10, Stroke::new(1.0, BORDER), StrokeKind::Inside);
        let fit = dark.width().min(dark.height()) - 40.0;
        let mut zoom = 1.0;
        for half in [dark, light] {
            zoom = draw_fit(&p, &t, half.center(), fit);
        }
        let size = t.size_vec2();
        p.text(
            rect.right_bottom() - vec2(12.0, 10.0),
            Align2::RIGHT_BOTTOM,
            format!("{}×{} px  ·  {zoom:.0}×", size.x, size.y),
            FontId::monospace(11.0),
            Color32::from_rgb(90, 88, 84),
        );
        ui.add_space(16.0);
    }

    fn pixels_ui(&mut self, ui: &mut egui::Ui) {
        let c = &mut self.local.crosshair;
        card(ui, "Canvas", |ui| {
            egui::Grid::new("canvas_opts")
                .num_columns(2)
                .spacing([20.0, 10.0])
                .show(ui, |ui| {
                    let mut n = c.grid;
                    row(ui, "Size", |ui| {
                        let r = ui.add(Slider::new(&mut n, 3..=MAX_GRID).suffix(" cells"));
                        // Shrinking crops the drawing, so make it undoable (once per drag).
                        if r.drag_started() || (r.changed() && !r.dragged()) {
                            self.undo.push((c.grid, c.pixels.clone()));
                        }
                        if r.changed() {
                            c.resize_grid(n);
                        }
                    });
                    slider(ui, "Pixel scale", &mut c.scale, 1..=MAX_SCALE);
                    row(ui, "Brush", |ui| {
                        ui.horizontal(|ui| {
                            ui.color_edit_button_srgba_unmultiplied(&mut self.brush);
                            ui.toggle_value(
                                &mut self.mirror_x,
                                format!("{} Mirror X", icon::FLIP_HORIZONTAL),
                            );
                            ui.toggle_value(
                                &mut self.mirror_y,
                                format!("{} Mirror Y", icon::FLIP_VERTICAL),
                            );
                        });
                    });
                    row(ui, "", |ui| {
                        ui.horizontal(|ui| {
                            let undo =
                                egui::Button::new(format!("{}  Undo", icon::ARROW_U_UP_LEFT));
                            let undo = ui
                                .add_enabled(!self.undo.is_empty(), undo)
                                .on_hover_text("Ctrl+Z");
                            let ctrl_z =
                                egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::Z);
                            let ctrl_z = ui.input_mut(|i| i.consume_shortcut(&ctrl_z));
                            if (undo.clicked() || ctrl_z)
                                && let Some((grid, px)) = self.undo.pop()
                            {
                                (c.grid, c.pixels) = (grid, px);
                            }
                            if ghost(ui, &format!("{}  Clear", icon::TRASH)).clicked() {
                                self.undo.push((c.grid, c.pixels.clone()));
                                c.pixels.fill([0; 4]);
                            }
                        });
                    });
                });
            ui.label(
                RichText::new(format!(
                    "{} Left paint    {} Right erase    {} Middle pick colour",
                    icon::PAINT_BRUSH,
                    icon::ERASER,
                    icon::EYEDROPPER
                ))
                .small()
                .color(MUTED),
            );
            ui.add_space(8.0);

            if self.undo.len() > 50 {
                self.undo.remove(0);
            }
            let n = c.grid as usize;
            let cell = (480.0 / n as f32).floor().clamp(5.0, 26.0);
            let (resp, p) = ui.allocate_painter(
                vec2(cell * n as f32, cell * n as f32),
                Sense::click_and_drag(),
            );
            let r = resp.rect;
            p.rect_filled(r, 0, Color32::from_rgb(10, 10, 10));
            let cell_rect = |x: usize, y: usize| {
                Rect::from_min_size(
                    pos2(r.min.x + x as f32 * cell, r.min.y + y as f32 * cell),
                    vec2(cell, cell),
                )
            };
            for (i, px) in c.pixels.iter().enumerate() {
                if px[3] > 0 {
                    let [r, g, b, a] = *px;
                    p.rect_filled(
                        cell_rect(i % n, i / n),
                        0.0,
                        Color32::from_rgba_unmultiplied(r, g, b, a),
                    );
                }
            }
            let grid = Stroke::new(1.0, Color32::from_rgb(30, 30, 29));
            for i in 0..=n {
                let o = i as f32 * cell;
                p.line_segment(
                    [pos2(r.min.x + o, r.min.y), pos2(r.min.x + o, r.max.y)],
                    grid,
                );
                p.line_segment(
                    [pos2(r.min.x, r.min.y + o), pos2(r.max.x, r.min.y + o)],
                    grid,
                );
            }
            // Mark the screen centre.
            let centre = Stroke::new(1.0, Color32::from_rgb(200, 90, 85));
            if n % 2 == 1 {
                p.rect_stroke(cell_rect(n / 2, n / 2), 0.0, centre, StrokeKind::Inside);
            } else {
                let m = r.center();
                p.line_segment([pos2(m.x, r.min.y), pos2(m.x, r.max.y)], centre);
                p.line_segment([pos2(r.min.x, m.y), pos2(r.max.x, m.y)], centre);
            }
            if let Some(h) = resp.hover_pos() {
                let (x, y) = (
                    ((h.x - r.min.x) / cell) as usize,
                    ((h.y - r.min.y) / cell) as usize,
                );
                if x < n && y < n {
                    p.rect_stroke(
                        cell_rect(x, y),
                        0.0,
                        Stroke::new(1.0, ACCENT),
                        StrokeKind::Inside,
                    );
                }
            }

            let Some(pos) = resp.interact_pointer_pos() else {
                self.last_cell = None;
                return;
            };
            let cur = (
                ((pos.x - r.min.x) / cell).floor() as isize,
                ((pos.y - r.min.y) / cell).floor() as isize,
            );
            let inside = |(x, y): (isize, isize)| {
                (0..n as isize).contains(&x) && (0..n as isize).contains(&y)
            };
            let (erase, pick) = ui.input(|i| (i.pointer.secondary_down(), i.pointer.middle_down()));
            if pick {
                if inside(cur) {
                    self.brush = c.pixels[cur.1 as usize * n + cur.0 as usize];
                }
                return;
            }
            let from = match self.last_cell {
                Some(prev) => prev,
                None => {
                    self.undo.push((c.grid, c.pixels.clone())); // one undo step per stroke
                    cur
                }
            };
            self.last_cell = Some(cur);
            let col = if erase { [0; 4] } else { self.brush };
            // Walk from the previous cell to this one so quick strokes have no gaps.
            let steps = (cur.0 - from.0).abs().max((cur.1 - from.1).abs()).max(1);
            for step in 0..=steps {
                let t = step as f32 / steps as f32;
                let p = (
                    (from.0 as f32 + (cur.0 - from.0) as f32 * t).round() as isize,
                    (from.1 as f32 + (cur.1 - from.1) as f32 * t).round() as isize,
                );
                if !inside(p) {
                    continue;
                }
                let (x, y) = (p.0 as usize, p.1 as usize);
                let xs = if self.mirror_x {
                    [x, n - 1 - x]
                } else {
                    [x, x]
                };
                let ys = if self.mirror_y {
                    [y, n - 1 - y]
                } else {
                    [y, y]
                };
                for yy in ys {
                    for xx in xs {
                        c.pixels[yy * n + xx] = col;
                    }
                }
            }
        });
    }

    fn games_page(&mut self, ui: &mut egui::Ui) {
        heading(
            ui,
            "Games",
            "Show the crosshair only while one of these is the focused window.",
        );
        let s = &mut self.local;

        card(ui, "", |ui| {
            ui.horizontal(|ui| {
                ui.checkbox(
                    &mut s.only_games,
                    RichText::new("Only show in my games").strong(),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if s.only_games {
                        badge(ui, "Games only", GREEN)
                    } else {
                        badge(ui, "Everywhere", BLUE)
                    }
                });
            });
            let hint = if s.only_games {
                "Hidden in every other app. It still shows while this window is focused, so you can preview."
            } else {
                "The crosshair currently draws on top of everything."
            };
            ui.label(RichText::new(hint).small().color(MUTED));
        });

        card(ui, "My games", |ui| {
            if s.games.is_empty() {
                ui.label(RichText::new("No games yet. Add one below.").color(MUTED));
            }
            let mut remove = None;
            egui::Grid::new("games")
                .num_columns(4)
                .spacing([14.0, 10.0])
                .show(ui, |ui| {
                    for (i, g) in s.games.iter_mut().enumerate() {
                        let c = s.presets.get(&g.preset).unwrap_or(&s.crosshair);
                        let t =
                            texture(&mut self.textures, ui.ctx(), &format!("game:{}", g.exe), c);
                        thumb(ui, &t, 38.0);
                        ui.label(
                            RichText::new(format!("{}  {}", icon::GAME_CONTROLLER, g.exe)).strong(),
                        );
                        let shown = if g.preset.is_empty() {
                            "Current crosshair"
                        } else {
                            g.preset.as_str()
                        };
                        egui::ComboBox::from_id_salt(("preset", i))
                            .selected_text(shown)
                            .width(190.0)
                            .show_ui(ui, |ui| {
                                ui.selectable_value(
                                    &mut g.preset,
                                    String::new(),
                                    "Current crosshair",
                                );
                                for name in s.presets.keys() {
                                    ui.selectable_value(&mut g.preset, name.clone(), name);
                                }
                            });
                        if ghost(ui, icon::X).on_hover_text("Remove").clicked() {
                            remove = Some(i);
                        }
                        ui.end_row();
                    }
                });
            if let Some(i) = remove {
                s.games.remove(i);
            }
        });

        let add = |games: &mut Vec<Game>, exe: &str| {
            let exe = exe.trim().to_lowercase();
            if !exe.is_empty() && !games.iter().any(|g| g.exe == exe) {
                games.push(Game {
                    exe,
                    preset: String::new(),
                });
            }
        };
        card(ui, "Add a game", |ui| {
            ui.horizontal(|ui| {
                let mut picked = None;
                egui::ComboBox::from_id_salt("running")
                    .selected_text("Pick a running app…")
                    .width(260.0)
                    .show_ui(ui, |ui| {
                        for exe in &self.running {
                            if ui.selectable_label(false, exe).clicked() {
                                picked = Some(exe.clone());
                            }
                        }
                    });
                if let Some(exe) = picked {
                    add(&mut s.games, &exe);
                }
                if ghost(ui, &format!("{}  Refresh", icon::ARROWS_CLOCKWISE)).clicked() {
                    self.running = overlay::running_apps();
                }
            });
            ui.horizontal(|ui| {
                let r = ui.add(
                    egui::TextEdit::singleline(&mut self.game_input)
                        .hint_text("or type it, e.g. cs2.exe")
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
                    add(&mut s.games, &self.game_input);
                    self.game_input.clear();
                }
            });
            ui.label(
                RichText::new("Start the game first, then hit Refresh to see it in the list.")
                    .small()
                    .color(MUTED),
            );
        });
    }

    fn presets_page(&mut self, ui: &mut egui::Ui) {
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
                    self.local
                        .presets
                        .insert(name, self.local.crosshair.clone());
                }
            });
        });

        let mut load = None;
        let mut delete = None;
        card(ui, "Saved", |ui| {
            if self.local.presets.is_empty() {
                ui.label(RichText::new("Nothing saved yet.").color(MUTED));
            }
            egui::Grid::new("presets")
                .num_columns(4)
                .spacing([14.0, 10.0])
                .show(ui, |ui| {
                    for (name, c) in &self.local.presets {
                        let t = texture(&mut self.textures, ui.ctx(), &format!("preset:{name}"), c);
                        thumb(ui, &t, 56.0);
                        ui.vertical(|ui| {
                            ui.set_min_width(180.0); // grid cells start narrow; don't wrap names
                            ui.label(RichText::new(name).strong());
                            let kind = match c.mode {
                                Mode::Lines => "Lines".to_string(),
                                Mode::Pixels => format!("Pixel {0}×{0}", c.grid),
                            };
                            ui.label(RichText::new(kind).small().color(MUTED));
                        });
                        if self.local.crosshair == *c {
                            // A pill would stretch to this tall row, so plain text instead.
                            ui.label(
                                RichText::new(format!("{}  In use", icon::CHECK)).color(GREEN.1),
                            );
                        } else if primary(ui, true, "Load").clicked() {
                            load = Some(name.clone());
                        }
                        let armed = self.confirm_delete.as_ref() == Some(name);
                        let text = if armed {
                            format!("{}  Delete?", icon::TRASH)
                        } else {
                            icon::TRASH.to_string()
                        };
                        let colour = if armed {
                            Color32::from_rgb(230, 120, 110)
                        } else {
                            MUTED
                        };
                        let btn = egui::Button::new(RichText::new(text).color(colour))
                            .fill(Color32::TRANSPARENT);
                        let hover = if armed {
                            "Click again to delete"
                        } else {
                            "Delete"
                        };
                        if ui.add(btn).on_hover_text(hover).clicked() {
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
        if let Some(n) = load {
            self.local.crosshair = self.local.presets[&n].clone();
            self.preset_name = n;
        }
        if let Some(n) = delete {
            self.confirm_delete = None;
            self.local.presets.remove(&n);
            self.textures.remove(&format!("preset:{n}"));
            for g in &mut self.local.games {
                if g.preset == n {
                    g.preset.clear();
                }
            }
        }
    }

    fn settings_page(&mut self, ui: &mut egui::Ui) {
        heading(ui, "Settings", "Hotkeys, startup and updates.");
        if let Some(i) = self.listening {
            ui.ctx().request_repaint(); // keep polling while waiting for a key
            if let Some(hk) = overlay::capture_key() {
                if let Some(hk) = hk {
                    *[&mut self.local.toggle_key, &mut self.local.menu_key][i] = hk;
                }
                self.listening = None;
            }
        }
        card(ui, "Keybinds", |ui| {
            egui::Grid::new("keys")
                .num_columns(4)
                .spacing([20.0, 12.0])
                .show(ui, |ui| {
                    for (i, label) in ["Toggle crosshair", "Show / hide this window"]
                        .into_iter()
                        .enumerate()
                    {
                        ui.label(label);
                        let hk = [self.local.toggle_key, self.local.menu_key][i];
                        let listening = self.listening == Some(i);
                        let text = if listening {
                            "Press a key…".into()
                        } else {
                            overlay::key_name(hk)
                        };
                        if keycap(ui, &text, listening).clicked() {
                            self.listening = Some(i);
                        }
                        if ghost(ui, icon::X).on_hover_text("Unbind").clicked() {
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
                                    .color(YELLOW.1),
                            );
                        }
                        ui.end_row();
                    }
                });
            ui.label(
                RichText::new(format!(
                    "{}  Click a key, then press any key or combo. Esc cancels.",
                    icon::KEYBOARD
                ))
                .small()
                .color(MUTED),
            );
        });
        card(ui, "Startup", |ui| {
            ui.horizontal(|ui| {
                if ui
                    .checkbox(
                        &mut self.autostart,
                        RichText::new("Start with Windows").strong(),
                    )
                    .changed()
                {
                    install::set_autostart(self.autostart);
                    self.autostart = install::autostart(); // show what actually got written
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if self.autostart {
                        badge(ui, "On", GREEN)
                    } else {
                        badge(ui, "Off", GRAY)
                    }
                });
            });
            let menu = overlay::key_name(self.local.menu_key);
            ui.label(
                RichText::new(format!(
                    "{}  Starts quietly when you log in: just the crosshair, no window. Press {menu} for settings.",
                    icon::ROCKET_LAUNCH
                ))
                .small()
                .color(MUTED),
            );
        });
        card(ui, "Updates", |ui| {
            ui.horizontal(|ui| {
                ui.checkbox(
                    &mut self.local.auto_update,
                    RichText::new("Install updates automatically").strong(),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new(format!("v{}", update::VERSION))
                            .monospace()
                            .color(MUTED),
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
                ui.label(RichText::new(text).small().color(MUTED));
            });
            ui.label(
                RichText::new("Updates come from the project's GitHub releases. It never restarts while you're in a game.")
                    .small()
                    .color(MUTED),
            );
        });
    }
}

fn lines_ui(ui: &mut egui::Ui, c: &mut Crosshair) {
    card(ui, "Colour", |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Fill").color(MUTED));
            ui.color_edit_button_srgba_unmultiplied(&mut c.color);
            ui.add_space(20.0);
            ui.label(RichText::new("Outline").color(MUTED));
            ui.color_edit_button_srgba_unmultiplied(&mut c.outline_color);
        });
    });
    card(ui, "Lines", |ui| {
        egui::Grid::new("lines")
            .num_columns(2)
            .spacing([20.0, 10.0])
            .show(ui, |ui| {
                slider(ui, "Length", &mut c.length, 0..=50);
                slider(ui, "Thickness", &mut c.thickness, 1..=20);
                slider(ui, "Gap", &mut c.gap, 0..=50);
                slider(ui, "Outline", &mut c.outline, 0..=5);
                row(ui, "", |ui| {
                    ui.checkbox(&mut c.t_style, "T-style (no top line)");
                });
            });
    });
    card(ui, "Dot & circle", |ui| {
        egui::Grid::new("extras")
            .num_columns(2)
            .spacing([20.0, 10.0])
            .show(ui, |ui| {
                row(ui, "", |ui| {
                    ui.checkbox(&mut c.dot, "Centre dot");
                });
                row(ui, "Dot size", |ui| {
                    ui.add_enabled(c.dot, Slider::new(&mut c.dot_size, 1..=20));
                });
                row(ui, "", |ui| {
                    ui.checkbox(&mut c.circle, "Circle");
                });
                row(ui, "Radius", |ui| {
                    ui.add_enabled(c.circle, Slider::new(&mut c.circle_radius, 1..=100));
                });
                row(ui, "Ring", |ui| {
                    ui.add_enabled(c.circle, Slider::new(&mut c.circle_thickness, 1..=10));
                });
            });
    });
}
