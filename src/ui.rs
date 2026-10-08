use crate::config::{
    self, Aim, AimButton, AimRule, Crosshair, Game, MAX_GRID, MAX_IMAGE, MAX_SCALE, Mode, Settings,
    Theme,
};
use crate::overlay::{self, KEY_OK, Status, VISIBLE};
use crate::{apps, install, picture, render, share, update};
use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Painter, Pos2, Rect, RichText, Sense, Slider,
    Stroke, StrokeKind, TextureHandle, pos2, vec2,
};
use egui_phosphor::bold as icon;
use std::collections::{BTreeMap, HashMap};
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Dwm::{
    DWMWA_BORDER_COLOR, DWMWA_CAPTION_COLOR, DWMWA_TEXT_COLOR, DWMWA_USE_IMMERSIVE_DARK_MODE,
    DwmSetWindowAttribute,
};

/// Warm monochrome; colour only where it means something. A pair is (background, text).
struct Palette {
    bg: Color32,
    side: Color32,  // sidebar
    field: Color32, // buttons and inputs at rest
    hover: Color32,
    press: Color32,   // while a button is held down
    control: Color32, // checkbox boxes and slider rails
    edge: Color32,    // outline of checkboxes, 3:1 against the page so you can see them
    popup: Color32,
    border: Color32,
    text: Color32,
    muted: Color32,
    accent: Color32,
    selection: f32, // how strongly the accent tints selected things
    danger: Color32,
    green: (Color32, Color32),
    yellow: (Color32, Color32),
    blue: (Color32, Color32),
    gray: (Color32, Color32),
}

const DARK: Palette = Palette {
    bg: Color32::from_rgb(18, 18, 17),
    side: Color32::from_rgb(23, 23, 22),
    field: Color32::from_rgb(34, 34, 32),
    hover: Color32::from_rgb(44, 44, 42),
    press: Color32::from_rgb(58, 58, 55),
    control: Color32::from_rgb(46, 46, 43),
    edge: Color32::from_rgb(112, 110, 105),
    popup: Color32::from_rgb(23, 23, 22),
    border: Color32::from_rgb(42, 42, 40),
    text: Color32::from_rgb(232, 230, 225),
    muted: Color32::from_rgb(146, 144, 139),
    accent: Color32::from_rgb(150, 205, 160),
    selection: 0.35,
    danger: Color32::from_rgb(230, 120, 110),
    green: (
        Color32::from_rgb(30, 46, 34),
        Color32::from_rgb(150, 205, 160),
    ),
    yellow: (
        Color32::from_rgb(52, 44, 24),
        Color32::from_rgb(222, 190, 110),
    ),
    blue: (
        Color32::from_rgb(26, 40, 54),
        Color32::from_rgb(130, 180, 225),
    ),
    gray: (
        Color32::from_rgb(38, 38, 36),
        Color32::from_rgb(150, 148, 142),
    ),
};

const LIGHT: Palette = Palette {
    bg: Color32::from_rgb(252, 251, 249),
    side: Color32::from_rgb(244, 243, 239),
    field: Color32::from_rgb(238, 237, 233),
    hover: Color32::from_rgb(227, 226, 221),
    press: Color32::from_rgb(212, 210, 204),
    control: Color32::from_rgb(226, 224, 219),
    edge: Color32::from_rgb(128, 126, 120),
    popup: Color32::from_rgb(255, 255, 255),
    border: Color32::from_rgb(224, 222, 216),
    text: Color32::from_rgb(34, 36, 38),
    muted: Color32::from_rgb(104, 102, 98),
    accent: Color32::from_rgb(38, 112, 72),
    selection: 0.5,
    danger: Color32::from_rgb(178, 58, 46),
    green: (
        Color32::from_rgb(226, 240, 229),
        Color32::from_rgb(36, 104, 60),
    ),
    yellow: (
        Color32::from_rgb(251, 243, 219),
        Color32::from_rgb(149, 100, 0),
    ),
    blue: (
        Color32::from_rgb(225, 243, 254),
        Color32::from_rgb(31, 108, 159),
    ),
    gray: (
        Color32::from_rgb(234, 233, 229),
        Color32::from_rgb(92, 90, 86),
    ),
};

static LIGHT_NOW: AtomicBool = AtomicBool::new(false);

/// The colours for whichever theme is showing. Set once per frame from egui's own theme, which
/// follows Windows unless you've picked one in Settings.
fn pal() -> &'static Palette {
    if LIGHT_NOW.load(Ordering::Relaxed) {
        &LIGHT
    } else {
        &DARK
    }
}

// Layout: page column width, and the preview column that appears beside it on wide windows.
const COLUMN: f32 = 640.0;
const PREVIEW_COLUMN: f32 = 400.0;
const GUTTER: f32 = 32.0;
const SCROLL_GAP: f32 = 24.0;
// Label column of the editor grids, so their sliders start at the same x.
// Just wider than the longest label ("Ring width"), so the cards still fit the narrowest window.
const LABEL_COL: f32 = 72.0;

/// The settings window's egui context, so other threads can wake it. Set once the window exists.
static CTX: OnceLock<egui::Context> = OnceLock::new();

/// Redraw the settings window because something it shows changed on another thread. While it's
/// hidden this does nothing, so the app idles in the tray at zero CPU.
pub fn repaint() {
    if let Some(ctx) = CTX.get()
        && overlay::settings_visible()
    {
        ctx.request_repaint();
    }
}

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
            cc.egui_ctx.set_theme(theme_preference(local.theme));
            overlay::adopt_settings_window();
            let _ = CTX.set(cc.egui_ctx.clone());
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
                running: apps::running(),
                apps: HashMap::new(),
                textures: HashMap::new(),
                autostart: install::autostart(),
                updates,
                start_hidden: hidden,
                last_cell: None,
                undo: Vec::new(),
                confirm_delete: None,
                renaming: None,
                image_error: None,
                preset_status: None,
                open_game: None,
                dirty: false,
                caption: None,
            }))
        }),
    )
}

fn theme_preference(t: Theme) -> egui::ThemePreference {
    match t {
        Theme::System => egui::ThemePreference::System,
        Theme::Dark => egui::ThemePreference::Dark,
        Theme::Light => egui::ThemePreference::Light,
    }
}

/// Colours the native title bar to match the page. Windows' own caption is a cool blue-grey that
/// doesn't fit the warm palette. Windows 10 ignores these attributes, which is fine.
fn colour_title_bar(hwnd: HWND, light: bool) {
    let p = pal();
    let colorref = |c: Color32| c.r() as u32 | (c.g() as u32) << 8 | (c.b() as u32) << 16;
    let attrs = [
        (DWMWA_USE_IMMERSIVE_DARK_MODE, u32::from(!light)),
        (DWMWA_CAPTION_COLOR, colorref(p.side)),
        (DWMWA_TEXT_COLOR, colorref(p.text)),
        (DWMWA_BORDER_COLOR, colorref(p.border)),
    ];
    for (attr, value) in attrs {
        unsafe {
            let _ = DwmSetWindowAttribute(
                hwnd,
                attr,
                &value as *const u32 as *const c_void,
                size_of::<u32>() as u32,
            );
        }
    }
}

fn visuals(base: egui::Visuals, p: &Palette) -> egui::Visuals {
    let mut v = base;
    v.panel_fill = p.bg;
    v.window_fill = p.popup;
    v.window_stroke = Stroke::new(1.0, p.border);
    v.extreme_bg_color = p.field;
    v.override_text_color = Some(p.text);
    v.selection.bg_fill = p.accent.gamma_multiply(p.selection);
    v.selection.stroke = Stroke::new(1.0, p.accent);
    v.slider_trailing_fill = true;
    for (w, fill) in [
        (&mut v.widgets.inactive, p.field),
        (&mut v.widgets.hovered, p.hover),
        (&mut v.widgets.active, p.press),
        (&mut v.widgets.open, p.hover),
    ] {
        w.corner_radius = CornerRadius::same(6);
        // Buttons use the weak fill; checkbox boxes and slider rails use the other one, which
        // needs to be clearly different from the page behind them.
        w.weak_bg_fill = fill;
        w.bg_fill = if fill == p.field { p.control } else { fill };
        w.bg_stroke = Stroke::NONE;
    }
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, p.border);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, p.border);
    v
}

const SEMIBOLD: &str = "semibold";

fn semibold() -> egui::FontFamily {
    egui::FontFamily::Name(SEMIBOLD.into())
}

/// A real semibold weight, for titles and labels. (egui's own `.semi()` only changes the colour,
/// so the interface had a single weight.)
trait Semi {
    fn semi(self) -> RichText;
}

impl Semi for RichText {
    fn semi(self) -> RichText {
        self.family(semibold())
    }
}

/// A font file from Windows' own font folder. Nothing is bundled or downloaded; if it's missing
/// the interface quietly keeps egui's built-in fonts.
fn system_font(file: &str) -> Option<Arc<egui::FontData>> {
    let windows = std::env::var_os("WINDIR").unwrap_or_else(|| r"C:\Windows".into());
    let bytes = std::fs::read(std::path::Path::new(&windows).join("Fonts").join(file)).ok()?;
    Some(Arc::new(egui::FontData::from_owned(bytes)))
}

fn fonts() -> egui::FontDefinitions {
    use egui::FontFamily::{Monospace, Proportional};
    let mut fonts = egui::FontDefinitions::default();
    // Semibold starts as a copy of the normal fallbacks, so it still works without the files.
    let fallbacks = fonts.families[&Proportional].clone();
    fonts.families.insert(semibold(), fallbacks);
    for (key, file, families) in [
        ("segoe", "segoeui.ttf", vec![Proportional]),
        ("segoe-semibold", "seguisb.ttf", vec![semibold()]),
        ("consolas", "consola.ttf", vec![Monospace]),
    ] {
        if let Some(data) = system_font(file) {
            fonts.font_data.insert(key.into(), data);
            for family in families {
                fonts
                    .families
                    .get_mut(&family)
                    .unwrap()
                    .insert(0, key.into());
            }
        }
    }
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Bold);
    // Icons sit inside labels ("icon  Name"), so semibold text needs them as a fallback too.
    fonts
        .families
        .get_mut(&semibold())
        .unwrap()
        .insert(1, "phosphor".into());
    fonts
}

fn style(ctx: &egui::Context) {
    ctx.set_fonts(fonts());

    ctx.set_visuals_of(egui::Theme::Dark, visuals(egui::Visuals::dark(), &DARK));
    ctx.set_visuals_of(egui::Theme::Light, visuals(egui::Visuals::light(), &LIGHT));
    ctx.all_styles_mut(|s| {
        s.spacing.item_spacing = vec2(10.0, 9.0);
        s.spacing.button_padding = vec2(12.0, 6.0);
        s.spacing.slider_width = 232.0; // leaves room for "15 cells" at the narrowest window
        s.spacing.interact_size.y = 28.0;
        use egui::TextStyle::*;
        s.text_styles.insert(Heading, FontId::new(28.0, semibold()));
        s.text_styles.insert(Body, FontId::proportional(14.0));
        s.text_styles.insert(Button, FontId::proportional(14.0));
        s.text_styles.insert(Small, FontId::proportional(12.5)); // 12 is about the floor for legibility
        s.text_styles.insert(Monospace, FontId::monospace(13.5));
    });
}

#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
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
    running: Vec<apps::App>,
    /// Display name and icon per app, looked up once.
    apps: HashMap<String, AppInfo>,
    /// Rendered crosshairs for the preview and preset thumbnails, re-rendered only on change.
    textures: HashMap<String, (Tex, Crosshair)>,
    autostart: bool,
    updates: mpsc::Sender<()>,
    start_hidden: bool,
    /// Canvas cell painted last frame, so a fast drag fills the cells in between.
    last_cell: Option<(isize, isize)>,
    /// Canvas snapshots (grid size, pixels) for Ctrl+Z.
    undo: Vec<(u32, Vec<[u8; 4]>)>,
    /// Preset whose delete button was clicked once; a second click deletes it.
    confirm_delete: Option<String>,
    /// Preset being renamed: its current name, and the name typed so far.
    renaming: Option<(String, String)>,
    /// Why the last picture couldn't be used, shown on the Image card.
    image_error: Option<String>,
    /// Result of the last preset export or import, shown on the Presets page.
    preset_status: Option<String>,
    /// The game (by exe) whose aiming settings are open on the Games page.
    open_game: Option<String>,
    dirty: bool,
    /// Theme last applied to the native title bar; None until the window has been found.
    caption: Option<bool>,
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // Colours follow egui's theme, which is Windows' own light/dark setting unless you've
        // picked one in Settings.
        let light = ui.ctx().theme() == egui::Theme::Light;
        LIGHT_NOW.store(light, Ordering::Relaxed);
        if self.caption != Some(light)
            && let Some(hwnd) = overlay::settings_window()
        {
            colour_title_bar(hwnd, light);
            self.caption = Some(light);
        }
        // eframe shows the window after its first frame no matter what, but it applies viewport
        // commands right after that, so hiding it here means it never reaches the screen.
        if std::mem::take(&mut self.start_hidden) {
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }
        // Closing just hides the window; OpenCrosshair keeps running in the tray, and Quit lives
        // in the tray icon's menu.
        if ui.ctx().input(|i| i.viewport().close_requested()) {
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::CancelClose);
            overlay::hide_settings();
        }
        let before = self.local.clone();
        // Drop a picture anywhere on the window to use it as the crosshair.
        let dropped = ui
            .ctx()
            .input(|i| i.raw.dropped_files.first().map(|f| f.path().to_path_buf()));
        if let Some(path) = dropped {
            self.use_image(&path);
        }

        egui::Panel::left("nav")
            .exact_size(210.0)
            .resizable(false)
            .show_separator_line(false)
            .frame(
                egui::Frame::new()
                    .fill(pal().side)
                    .stroke(Stroke::new(1.0, pal().border))
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
            .frame(egui::Frame::new().fill(pal().bg).inner_margin(margin))
            .show(ui, |ui| {
                let full = ui.available_width();
                // On a wide window the Crosshair page gets the preview in its own column, so it
                // stays in view while you scroll through the controls.
                let side = self.page == Page::Crosshair
                    && full >= COLUMN + SCROLL_GAP + GUTTER + PREVIEW_COLUMN + 48.0;
                // Otherwise the heading and a smaller preview stay put at the top, and only the
                // controls below them scroll, so you can watch the crosshair while you drag.
                let sticky = self.page == Page::Crosshair && !side;
                let column = (full - SCROLL_GAP).min(COLUMN);
                let group = column + SCROLL_GAP + if side { GUTTER + PREVIEW_COLUMN } else { 0.0 };
                // Centre the content in whatever room the window gives it.
                let indent = ((full - group) / 2.0).floor().max(0.0);
                ui.horizontal_top(|ui| {
                    ui.add_space(indent);
                    let height = ui.available_height();
                    ui.allocate_ui_with_layout(
                        vec2(column + SCROLL_GAP, height),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            if sticky {
                                egui::Frame::new()
                                    .inner_margin(egui::Margin {
                                        right: SCROLL_GAP as i8,
                                        ..Default::default()
                                    })
                                    .show(ui, |ui| {
                                        ui.set_width(column);
                                        self.crosshair_header(ui, true);
                                    });
                            }
                            egui::ScrollArea::vertical()
                                .id_salt(self.page) // each page keeps its own scroll position
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
                                            Page::Crosshair => {
                                                if !sticky {
                                                    self.crosshair_header(ui, false);
                                                }
                                                self.crosshair_body(ui);
                                            }
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
                            let size = vec2(PREVIEW_COLUMN, PREVIEW_COLUMN * 1.3);
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
        if self.local.theme != before.theme {
            ui.ctx().set_theme(theme_preference(self.local.theme));
            ui.ctx().request_repaint(); // redraw in the new colours straight away
        }
        // Save once the user lets go, not on every slider tick / painted cell.
        if self.dirty && !ui.ctx().input(|i| i.pointer.any_down()) {
            self.local.save();
            self.dirty = false;
        }
    }
}

// ---------- small building blocks ----------

/// Flat card: hairline border, no fill change, and a quiet semibold caption in sentence case.
fn card<R>(ui: &mut egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let r = egui::Frame::new()
        .stroke(Stroke::new(1.0, pal().border))
        .corner_radius(10)
        .inner_margin(20)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            if !title.is_empty() {
                ui.label(RichText::new(title).size(13.0).semi().color(pal().muted));
                ui.add_space(6.0);
            }
            add(ui)
        })
        .inner;
    ui.add_space(14.0);
    r
}

/// What to show where a list would be: what this is, and how to start.
fn empty_state(ui: &mut egui::Ui, icon: &str, title: &str, body: &str) {
    ui.vertical_centered(|ui| {
        ui.add_space(6.0);
        let (rect, _) = ui.allocate_exact_size(vec2(44.0, 44.0), Sense::hover());
        let p = ui.painter();
        p.rect_filled(rect, 10, pal().field);
        p.text(
            rect.center(),
            Align2::CENTER_CENTER,
            icon,
            FontId::proportional(22.0),
            pal().muted,
        );
        ui.add_space(6.0);
        ui.label(RichText::new(title).semi());
        ui.label(RichText::new(body).small().color(pal().muted));
        ui.add_space(6.0);
    });
}

/// A sidebar entry. Icons sit in a fixed column so labels line up; hover gets a soft fill, and the
/// page you're on gets a stronger one, an accent bar and a semibold label.
fn nav_item(ui: &mut egui::Ui, icon: &str, label: &str, on: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 38.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, on, label));
    if ui.is_rect_visible(rect) {
        let p = pal();
        let fill = if on {
            p.hover
        } else if resp.hovered() {
            p.field
        } else {
            Color32::TRANSPARENT
        };
        let ink = if on || resp.hovered() {
            p.text
        } else {
            p.muted
        };
        let painter = ui.painter();
        painter.rect_filled(rect, 8, fill);
        if on {
            let bar =
                Rect::from_center_size(pos2(rect.left() + 4.0, rect.center().y), vec2(3.0, 18.0));
            painter.rect_filled(bar, 2, p.accent);
        }
        if resp.has_focus() {
            painter.rect_stroke(rect, 8, Stroke::new(1.5, p.accent), StrokeKind::Inside);
        }
        let family = if on {
            semibold()
        } else {
            egui::FontFamily::Proportional
        };
        let mid = rect.center().y;
        painter.text(
            pos2(rect.left() + 18.0, mid),
            Align2::LEFT_CENTER,
            icon,
            FontId::proportional(17.0),
            ink,
        );
        painter.text(
            pos2(rect.left() + 46.0, mid),
            Align2::LEFT_CENTER,
            label,
            FontId::new(15.0, family),
            ink,
        );
    }
    resp
}

/// One option of a segmented control: solid when chosen, quiet (with hover feedback) otherwise.
fn segment(ui: &mut egui::Ui, text: String, on: bool, size: egui::Vec2) -> egui::Response {
    let ink = if on { pal().bg } else { pal().muted };
    let mut btn = egui::Button::new(RichText::new(&text).semi().color(ink)).min_size(size);
    if on {
        btn = btn.fill(pal().text);
    }
    let r = ui.add(btn);
    // Screen readers hear "Lines, selected", not the icon character in front of it.
    r.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, on, plain(&text)));
    r
}

/// Text without the icon glyphs (they live in Unicode's private-use area, so a screen reader would
/// announce them as nothing or as noise) and without the padding after them.
fn plain(text: &str) -> String {
    text.chars()
        .filter(|c| !('\u{E000}'..='\u{F8FF}').contains(c))
        .collect::<String>()
        .trim()
        .to_string()
}

/// What a screen reader should say for a widget, when the widget's own text isn't enough.
fn name_it(r: egui::Response, label: impl ToString) -> egui::Response {
    let label = label.to_string();
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label.clone()));
    r
}

fn row(ui: &mut egui::Ui, label: &str, add: impl FnOnce(&mut egui::Ui)) {
    ui.label(RichText::new(label).color(pal().muted));
    add(ui);
    ui.end_row();
}

fn slider(ui: &mut egui::Ui, label: &str, v: &mut u32, range: std::ops::RangeInclusive<u32>) {
    slider_when(ui, true, label, v, range);
}

/// A labelled slider that can be greyed out, for settings that only apply when another is on.
fn slider_when(
    ui: &mut egui::Ui,
    enabled: bool,
    label: &str,
    v: &mut u32,
    range: std::ops::RangeInclusive<u32>,
) {
    let text = ui.label(RichText::new(label).color(pal().muted));
    ui.add_enabled(enabled, Slider::new(v, range))
        .labelled_by(text.id);
    ui.end_row();
}

fn heading(ui: &mut egui::Ui, title: &str, sub: &str) {
    ui.label(RichText::new(title).heading().extra_letter_spacing(-0.4));
    ui.label(RichText::new(sub).color(pal().muted));
    ui.add_space(18.0);
}

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
        ui.label(
            RichText::new("Choose \"Hide it\" or a preset above to turn on the button and toggle.")
                .small()
                .color(pal().muted),
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

/// A checkbox whose box you can actually see against the page.
fn check(
    ui: &mut egui::Ui,
    value: &mut bool,
    label: impl Into<egui::WidgetText>,
) -> egui::Response {
    ui.scope(|ui| {
        let w = &mut ui.visuals_mut().widgets;
        for state in [&mut w.inactive, &mut w.hovered, &mut w.active] {
            state.corner_radius = CornerRadius::same(4); // a box, not a bubble
        }
        w.inactive.bg_stroke = Stroke::new(1.5, pal().edge);
        w.hovered.bg_stroke = Stroke::new(1.5, pal().text);
        w.active.bg_stroke = Stroke::new(1.5, pal().text);
        ui.checkbox(value, label)
    })
    .inner
}

/// An on/off switch: a pill with a knob that slides across, accent when on.
fn switch(ui: &mut egui::Ui, on: &mut bool) -> egui::Response {
    let size = vec2(44.0, 24.0);
    let (rect, mut response) = ui.allocate_exact_size(size, Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::Checkbox,
            ui.is_enabled(),
            *on,
            "Show crosshair",
        )
    });

    if ui.is_rect_visible(rect) {
        let how_on = ui.ctx().animate_bool_responsive(response.id, *on);
        let p = pal();
        let (track, knob) = if *on {
            (p.accent, p.bg)
        } else {
            (p.edge, p.muted)
        };
        let radius = 0.5 * rect.height();
        let painter = ui.painter();
        painter.rect_filled(rect, radius, track);
        if response.has_focus() {
            painter.rect_stroke(
                rect,
                radius,
                ui.visuals().selection.stroke,
                StrokeKind::Outside,
            );
        }
        let x = egui::lerp((rect.left() + radius)..=(rect.right() - radius), how_on);
        painter.circle_filled(pos2(x, rect.center().y), 0.75 * radius, knob);
    }
    response
}

/// Solid button in the text colour: the one primary action on a card. It softens a little on hover
/// and a little more while pressed.
fn primary(ui: &mut egui::Ui, enabled: bool, text: &str) -> egui::Response {
    let r = ui
        .scope(|ui| {
            let p = pal();
            let w = &mut ui.visuals_mut().widgets;
            w.inactive.weak_bg_fill = p.text;
            w.hovered.weak_bg_fill = p.text.lerp_to_gamma(p.bg, 0.14);
            w.active.weak_bg_fill = p.text.lerp_to_gamma(p.bg, 0.28);
            ui.add_enabled(
                enabled,
                egui::Button::new(RichText::new(text).color(p.bg).semi()),
            )
        })
        .inner;
    name_it(r, plain(text))
}

/// A quiet button: no frame until you point at it.
fn ghost(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let r = ui
        .add(egui::Button::new(RichText::new(text).color(pal().muted)).frame_when_inactive(false));
    name_it(r, plain(text))
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
                    .semi()
                    .color(fg)
                    .extra_letter_spacing(0.6),
            );
        });
}

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

/// Six one-click colours for the usual picks, beside a colour button. Keeps the alpha.
fn swatches(ui: &mut egui::Ui, colour: &mut [u8; 4]) {
    const PICKS: [(&str, [u8; 3]); 6] = [
        ("Green", [0, 255, 0]),
        ("Cyan", [0, 255, 255]),
        ("Yellow", [255, 255, 0]),
        ("Magenta", [255, 0, 255]),
        ("Red", [255, 0, 0]),
        ("White", [255, 255, 255]),
    ];
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for (name, rgb) in PICKS {
            let (rect, mut resp) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::click());
            let ring = rect.expand(2.0);
            let p = ui.painter();
            p.rect_filled(rect, 4, Color32::from_rgb(rgb[0], rgb[1], rgb[2]));
            p.rect_stroke(rect, 4, Stroke::new(1.0, pal().edge), StrokeKind::Inside);
            if colour[..3] == rgb || resp.hovered() {
                p.rect_stroke(ring, 6, Stroke::new(2.0, pal().text), StrokeKind::Outside);
            }
            if resp.has_focus() {
                p.rect_stroke(ring, 6, ui.visuals().selection.stroke, StrokeKind::Outside);
            }
            if resp.clicked() {
                colour[..3].copy_from_slice(&rgb);
                resp.mark_changed();
            }
            name_it(resp, name).on_hover_text(name);
        }
    });
}

/// A rendered crosshair, and where its centre is inside the texture.
#[derive(Clone)]
struct Tex {
    handle: TextureHandle,
    centre: f32, // texels; see render::Image::centre
}

/// Crosshair texture for `key`, re-rendered only when the crosshair changed.
fn texture(
    cache: &mut HashMap<String, (Tex, Crosshair)>,
    ctx: &egui::Context,
    key: &str,
    c: &Crosshair,
) -> Tex {
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
    let t = Tex {
        // Sharp blocks when zoomed in; smooth when a big crosshair is shrunk into a small tile,
        // where nearest-neighbour would drop rows unevenly and knock it off-centre.
        handle: ctx.load_texture(
            key,
            ci,
            egui::TextureOptions {
                minification: egui::TextureFilter::Linear,
                ..egui::TextureOptions::NEAREST
            },
        ),
        centre: img.centre,
    };
    cache.insert(key.to_string(), (t.clone(), c.clone()));
    t
}

/// Draw a crosshair with its own centre (not the texture's) on `at`, as big as fits in `max`
/// points. Each texel is a whole number of screen pixels, so it stays crisp at any display scale.
/// Returns the zoom in screen pixels per texel.
fn draw_fit(p: &Painter, t: &Tex, at: Pos2, max: f32) -> f32 {
    let ppp = p.ctx().pixels_per_point();
    let size = t.handle.size_vec2();
    let fit = max * ppp / size.x.max(size.y);
    let zoom = if fit >= 1.0 {
        fit.floor().min(8.0)
    } else {
        fit
    };
    let scale = zoom / ppp; // points per texel
    // Zoomed in, snap to whole screen pixels so every texel is a crisp block. Shrunk, the
    // smoothing handles fractions, so place it exactly instead.
    let snap = |v: f32| {
        if zoom >= 1.0 {
            (v * ppp).round() / ppp
        } else {
            v
        }
    };
    let min = pos2(snap(at.x - t.centre * scale), snap(at.y - t.centre * scale));
    let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
    p.image(
        t.handle.id(),
        Rect::from_min_size(min, size * scale),
        uv,
        Color32::WHITE,
    );
    zoom
}

/// Small dark tile with the crosshair in it.
fn thumb(ui: &mut egui::Ui, t: &Tex, size: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 6, Color32::from_rgb(10, 10, 10));
    p.rect_stroke(rect, 6, Stroke::new(1.0, pal().border), StrokeKind::Inside);
    draw_fit(&p, t, rect.center(), size - 10.0);
}

/// What the games list shows for an app.
struct AppInfo {
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

// ---------- pages ----------

impl App {
    fn nav(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(icon::CROSSHAIR)
                    .size(22.0)
                    .color(pal().accent),
            );
            ui.label(
                RichText::new("OpenCrosshair")
                    .size(18.0)
                    .semi()
                    .extra_letter_spacing(-0.2),
            );
        });
        ui.add_space(22.0);
        for (p, ic, name) in [
            (Page::Crosshair, icon::CROSSHAIR_SIMPLE, "Crosshair"),
            (Page::Games, icon::GAME_CONTROLLER, "Games"),
            (Page::Presets, icon::BOOKMARKS_SIMPLE, "Presets"),
            (Page::Settings, icon::GEAR_SIX, "Settings"),
        ] {
            if nav_item(ui, ic, name, self.page == p).clicked() {
                self.page = p;
                self.listening = None;
                self.confirm_delete = None;
                self.renaming = None;
            }
        }

        ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
            let toggle = overlay::key_name(self.local.toggle_key);
            let game_name = |exe: &str| {
                let game = self.local.games.iter().find(|g| g.exe == exe);
                let name = game.map(|g| g.name.clone()).filter(|n| !n.is_empty());
                name.unwrap_or_else(|| apps::display_name(exe, "", ""))
            };
            let (badge_text, tone, detail) = match overlay::STATUS.lock().unwrap().clone() {
                Status::InGame(exe) => ("In game", pal().green, game_name(&exe)),
                Status::Fullscreen(exe) => (
                    "Fullscreen",
                    pal().yellow,
                    format!("{}: can't see it? Use borderless.", game_name(&exe)),
                ),
                Status::Waiting => (
                    "Waiting",
                    pal().gray,
                    "Shows when one of your games is in front".to_string(),
                ),
                Status::Hidden => ("Hidden", pal().gray, format!("Press {toggle} to show it")),
                Status::Everywhere => (
                    "Everywhere",
                    pal().blue,
                    "Showing over every app".to_string(),
                ),
            };
            ui.label(RichText::new(detail).small().color(pal().muted));
            badge(ui, badge_text, tone);
            ui.add_space(6.0);

            let mut on = VISIBLE.load(Ordering::Relaxed);
            // Switch first, left to right: a right_to_left layout (or egui::Sides) inside this
            // bottom_up column leaves the whole window unpainted.
            ui.horizontal(|ui| {
                let hint = format!("Toggle: {toggle}");
                if switch(ui, &mut on).on_hover_text(hint).changed() {
                    VISIBLE.store(on, Ordering::Relaxed);
                    overlay::refresh();
                }
                ui.label(RichText::new("Crosshair").semi());
            });
        });
    }

    /// The Crosshair page's heading, and the preview when it isn't in its own column.
    fn crosshair_header(&mut self, ui: &mut egui::Ui, with_preview: bool) {
        heading(
            ui,
            "Crosshair",
            "Changes show on the overlay as you make them.",
        );
        if with_preview {
            self.preview(ui, vec2(ui.available_width(), 150.0), false);
        }
    }

    fn crosshair_body(&mut self, ui: &mut egui::Ui) {
        let c = &mut self.local.crosshair;
        ui.horizontal(|ui| {
            for (m, ic, name) in [
                (Mode::Lines, icon::CROSSHAIR_SIMPLE, "Lines"),
                (Mode::Pixels, icon::PAINT_BRUSH, "Pixel canvas"),
                (Mode::Image, icon::IMAGE, "Image"),
            ] {
                if segment(ui, format!("{ic}  {name}"), c.mode == m, vec2(130.0, 34.0)).clicked() {
                    c.mode = m;
                }
            }
        });
        ui.add_space(14.0);

        match c.mode {
            Mode::Lines => lines_ui(ui, c),
            Mode::Pixels => self.pixels_ui(ui),
            Mode::Image => self.image_ui(ui),
        }

        let c = &mut self.local.crosshair;
        card(ui, "Position", |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Offset").color(pal().muted));
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
        p.rect_stroke(rect, 10, Stroke::new(1.0, pal().border), StrokeKind::Inside);
        let fit = dark.width().min(dark.height()) - 40.0;
        let mut zoom = 1.0;
        for half in [dark, light] {
            zoom = draw_fit(&p, &t, half.center(), fit);
        }
        let size = t.handle.size_vec2();
        p.text(
            rect.right_bottom() - vec2(12.0, 10.0),
            Align2::RIGHT_BOTTOM,
            format!("{}×{} px  ·  shown at {zoom:.0}×", size.x, size.y),
            FontId::monospace(11.0),
            Color32::from_rgb(90, 88, 84),
        );
        ui.add_space(16.0);
    }

    fn image_ui(&mut self, ui: &mut egui::Ui) {
        let mut choose = false;
        let c = &mut self.local.crosshair;
        card(ui, "Image", |ui| {
            ui.horizontal(|ui| {
                choose =
                    primary(ui, true, &format!("{}  Choose image…", icon::FOLDER_OPEN)).clicked();
                let current = if c.image.is_empty() {
                    "No image yet".to_string()
                } else {
                    "Image in use".to_string()
                };
                ui.label(RichText::new(current).color(pal().muted));
            });
            ui.label(
                RichText::new("Or drop a picture onto this window. PNG with a transparent background works best.")
                    .small()
                    .color(pal().muted),
            );
            if let Some(e) = &self.image_error {
                ui.label(
                    RichText::new(format!("{}  {e}", icon::WARNING))
                        .small()
                        .color(pal().yellow.1),
                );
            }
            ui.add_space(4.0);
            egui::Grid::new("image_opts")
                .num_columns(2)
                .min_col_width(LABEL_COL)
                .spacing([20.0, 10.0])
                .show(ui, |ui| {
                    row(ui, "Size", |ui| {
                        ui.add(Slider::new(&mut c.image_size, 4..=MAX_IMAGE).suffix(" px"));
                    });
                    row(ui, "Opacity", |ui| {
                        ui.add(Slider::new(&mut c.image_opacity, 5..=100).suffix("%"));
                    });
                });
        });
        if choose && let Some(path) = picture::pick(overlay::settings_window()) {
            self.use_image(&path);
        }
    }

    /// Import a picture (from the dialog or dropped on the window) and switch to it.
    fn use_image(&mut self, path: &std::path::Path) {
        if picture::load(path, 16).is_none() {
            self.image_error = Some("That file isn't a picture Windows can open.".into());
            return;
        }
        match config::import_image(path) {
            Ok(name) => {
                let c = &mut self.local.crosshair;
                c.mode = Mode::Image;
                c.image = name;
                self.page = Page::Crosshair;
                self.image_error = None;
            }
            Err(e) => self.image_error = Some(format!("Couldn't import it: {e}")),
        }
    }

    fn pixels_ui(&mut self, ui: &mut egui::Ui) {
        let c = &mut self.local.crosshair;
        card(ui, "Canvas", |ui| {
            egui::Grid::new("canvas_opts")
                .num_columns(2)
                .min_col_width(LABEL_COL)
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
                            name_it(
                                ui.color_edit_button_srgba_unmultiplied(&mut self.brush),
                                "Brush colour",
                            );
                            swatches(ui, &mut self.brush);
                        });
                    });
                    // The mirror toggles get their own line: next to the swatches they'd run off
                    // a narrow window.
                    row(ui, "", |ui| {
                        ui.horizontal(|ui| {
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
                .color(pal().muted),
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
                        Stroke::new(1.0, pal().accent),
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
            let hint = if s.only_games {
                "Hidden in every other app. It still shows while this window is focused, so you can preview."
            } else {
                "The crosshair currently draws on top of everything."
            };
            ui.label(RichText::new(hint).small().color(pal().muted));
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
                            ui.label(
                                RichText::new(
                                    "What a mouse button does to the crosshair in this game. \
                                     Choose \"Keep the crosshair\" to make it ignore aiming here.",
                                )
                                .small()
                                .color(pal().muted),
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
                                    ui.label(
                                        RichText::new(format!(
                                            "Using your default: {}.",
                                            describe_aim(&default_rule)
                                        ))
                                        .small()
                                        .color(pal().muted),
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
                s.games.remove(i);
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
                    ui.label(
                        RichText::new(
                            "It's set for every account. Untick \"Disable fullscreen optimizations\" \
                             in the game's Properties > Compatibility tab (needs admin), or play borderless.",
                        )
                        .small()
                        .color(pal().muted),
                    );
                } else if ghost(ui, &format!("{}  Turn them back on", icon::WRENCH)).clicked()
                    && apps::enable_fullscreen_optimizations(&g.path)
                {
                    info.fso = apps::Fso::On;
                }
            }
        });

        let add = |games: &mut Vec<Game>, exe: &str, path: &str, name: &str| {
            let exe = exe.trim().to_lowercase();
            // "cs2" works as well as "cs2.exe".
            let exe = if exe.is_empty() || exe.ends_with(".exe") {
                exe
            } else {
                format!("{exe}.exe")
            };
            if !exe.is_empty() && !games.iter().any(|g| g.exe == exe) {
                games.push(Game {
                    exe,
                    name: name.to_string(),
                    path: path.to_string(),
                    ..Default::default()
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
                    add(&mut s.games, &app.exe, &app.path, &name);
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
                    add(&mut s.games, &self.game_input, "", "");
                    self.game_input.clear();
                }
            });
            ui.label(
                RichText::new("Open the game first, then press Refresh to find it.")
                    .small()
                    .color(pal().muted),
            );
        });

        card(ui, "While aiming", |ui| {
            let mut rule = default_rule.clone();
            aim_controls(ui, "default", &preset_names, &mut rule);
            (s.aim, s.aim_button, s.aim_toggle) = (rule.action, rule.button, rule.toggle);
            ui.label(
                RichText::new(
                    "For games where you aim down sights. It reads the button's state, with no input hooks.",
                )
                .small()
                .color(pal().muted),
            );
            ui.label(
                RichText::new(
                    "This is the default for every game. To give one game its own, use Default aim on its row in My games.",
                )
                .small()
                .color(pal().muted),
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
        let mut export = None;
        let mut export_all = false;
        let mut import = false;
        let mut commit_rename = None;
        card(ui, "", |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Saved").size(13.0).semi().color(pal().muted));
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
                ui.label(RichText::new(s).small().color(pal().muted));
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
                            ui.label(RichText::new(kind).small().color(pal().muted));
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
                        let export_btn =
                            egui::Button::new(RichText::new(icon::EXPORT).color(pal().muted))
                                .frame_when_inactive(false);
                        if name_it(ui.add(export_btn), format!("Export {name}"))
                            .on_hover_text("Export")
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

    fn settings_page(&mut self, ui: &mut egui::Ui) {
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
            ui.horizontal(|ui| {
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
            ui.label(
                RichText::new("System follows Windows' light or dark app mode.")
                    .small()
                    .color(pal().muted),
            );
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
            ui.label(
                RichText::new(format!(
                    "{}  Click a key, then press any key or combo. Esc cancels.",
                    icon::KEYBOARD
                ))
                .small()
                .color(pal().muted),
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
            let menu = overlay::key_name(self.local.menu_key);
            ui.label(
                RichText::new(format!(
                    "{}  Starts quietly when you log in, in the tray. Press {menu} or click the tray icon for settings.",
                    icon::ROCKET_LAUNCH
                ))
                .small()
                .color(pal().muted),
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
                ui.label(RichText::new(text).small().color(pal().muted));
            });
            ui.label(
                RichText::new("Updates come from the project's GitHub releases. It never restarts while you're in a game.")
                    .small()
                    .color(pal().muted),
            );
        });
        card(ui, "About", |ui| {
            let repo = env!("CARGO_PKG_REPOSITORY");
            ui.label(
                RichText::new("Free and open source, under the MIT license.")
                    .small()
                    .color(pal().muted),
            );
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

fn lines_ui(ui: &mut egui::Ui, c: &mut Crosshair) {
    card(ui, "Colour", |ui| {
        ui.horizontal(|ui| {
            let fill = ui.label(RichText::new("Fill").color(pal().muted));
            ui.color_edit_button_srgba_unmultiplied(&mut c.color)
                .labelled_by(fill.id);
            ui.add_space(8.0);
            swatches(ui, &mut c.color);
            ui.add_space(20.0);
            let outline = ui.label(RichText::new("Outline").color(pal().muted));
            ui.color_edit_button_srgba_unmultiplied(&mut c.outline_color)
                .labelled_by(outline.id);
        });
    });
    card(ui, "Lines", |ui| {
        egui::Grid::new("lines")
            .num_columns(2)
            .min_col_width(LABEL_COL)
            .spacing([20.0, 10.0])
            .show(ui, |ui| {
                slider(ui, "Length", &mut c.length, 0..=50);
                slider(ui, "Thickness", &mut c.thickness, 1..=20);
                slider(ui, "Gap", &mut c.gap, 0..=50);
                slider(ui, "Outline", &mut c.outline, 0..=5);
                row(ui, "", |ui| {
                    check(ui, &mut c.t_style, "T-style (no top line)");
                });
            });
    });
    card(ui, "Dot & circle", |ui| {
        egui::Grid::new("extras")
            .num_columns(2)
            .min_col_width(LABEL_COL)
            .spacing([20.0, 10.0])
            .show(ui, |ui| {
                row(ui, "", |ui| {
                    check(ui, &mut c.dot, "Centre dot");
                });
                slider_when(ui, c.dot, "Dot size", &mut c.dot_size, 1..=20);
                row(ui, "", |ui| {
                    check(ui, &mut c.circle, "Circle");
                });
                slider_when(ui, c.circle, "Radius", &mut c.circle_radius, 1..=100);
                slider_when(ui, c.circle, "Ring width", &mut c.circle_thickness, 1..=10);
            });
    });
}
