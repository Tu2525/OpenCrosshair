mod crosshair;
mod games;
mod presets;
mod settings;
mod widgets;

use crate::config::{Crosshair, Settings, Theme};
use crate::overlay::{self, Status, VISIBLE};
use crate::{apps, install};
use eframe::egui::{self, Color32, CornerRadius, FontId, RichText, Stroke, vec2};
use egui_phosphor::bold as icon;
use games::AppInfo;
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use widgets::{Tex, badge, hint, nav_item, switch};
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Dwm::{
    DWMWA_BORDER_COLOR, DWMWA_CAPTION_COLOR, DWMWA_TEXT_COLOR, DWMWA_USE_IMMERSIVE_DARK_MODE,
    DwmSetWindowAttribute,
};
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
use windows::Win32::UI::Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW};
use windows::Win32::UI::WindowsAndMessaging::{
    SPI_GETCLIENTAREAANIMATION, SPI_GETHIGHCONTRAST, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
    SystemParametersInfoW,
};
use windows::core::{BOOL, w};

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

/// Windows' contrast theme: muted text, borders and edges take the full text colour, so nothing
/// relies on a subtle grey.
const fn high_contrast(p: Palette) -> Palette {
    Palette {
        muted: p.text,
        border: p.text,
        edge: p.text,
        ..p
    }
}

const HIGH_DARK: Palette = high_contrast(DARK);
const HIGH_LIGHT: Palette = high_contrast(LIGHT);

static LIGHT_NOW: AtomicBool = AtomicBool::new(false);
/// Set once at startup from Windows' contrast theme setting.
static HIGH_CONTRAST: AtomicBool = AtomicBool::new(false);

/// The colours for whichever theme is showing. Set once per frame from egui's own theme, which
/// follows Windows unless you've picked one in Settings.
fn pal() -> &'static Palette {
    palette(LIGHT_NOW.load(Ordering::Relaxed))
}

fn palette(light: bool) -> &'static Palette {
    match (light, HIGH_CONTRAST.load(Ordering::Relaxed)) {
        (false, false) => &DARK,
        (true, false) => &LIGHT,
        (false, true) => &HIGH_DARK,
        (true, true) => &HIGH_LIGHT,
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
    let (animations, text_scale, high_contrast) = windows_accessibility();
    HIGH_CONTRAST.store(high_contrast, Ordering::Relaxed);
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
            style(&cc.egui_ctx, animations);
            cc.egui_ctx.set_zoom_factor(text_scale);
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
                removed_game: None,
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

/// Windows' accessibility settings, read once at startup: animation effects, text size as a zoom
/// factor, and whether a contrast theme is on. Anything that can't be read keeps its default.
fn windows_accessibility() -> (bool, f32, bool) {
    let mut animations = BOOL(1);
    let mut contrast = HIGHCONTRASTW {
        cbSize: size_of::<HIGHCONTRASTW>() as u32,
        ..Default::default()
    };
    let mut percent = 100u32;
    let mut size = size_of::<u32>() as u32;
    unsafe {
        let _ = SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            Some(&mut animations as *mut BOOL as *mut c_void),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
        let _ = SystemParametersInfoW(
            SPI_GETHIGHCONTRAST,
            contrast.cbSize,
            Some(&mut contrast as *mut HIGHCONTRASTW as *mut c_void),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
        let _ = RegGetValueW(
            HKEY_CURRENT_USER,
            w!(r"Software\Microsoft\Accessibility"),
            w!("TextScaleFactor"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut percent as *mut u32 as *mut c_void),
            Some(&mut size),
        );
    }
    (
        animations.as_bool(),
        zoom_for_percent(percent),
        contrast.dwFlags.contains(HCF_HIGHCONTRASTON),
    )
}

/// Windows' text size, a percentage Settings keeps between 100 and 225, as egui's zoom factor.
fn zoom_for_percent(percent: u32) -> f32 {
    percent.clamp(100, 225) as f32 / 100.0
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

fn style(ctx: &egui::Context, animations: bool) {
    ctx.set_fonts(fonts());

    ctx.set_visuals_of(
        egui::Theme::Dark,
        visuals(egui::Visuals::dark(), palette(false)),
    );
    ctx.set_visuals_of(
        egui::Theme::Light,
        visuals(egui::Visuals::light(), palette(true)),
    );
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
        // Animation effects are off in Windows: hover fades and the switch's slide happen at once.
        if !animations {
            s.animation_time = 0.0;
        }
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
    /// The game last removed from the Games page, with its old position, until Undo or a change.
    removed_game: Option<(usize, crate::config::Game)>,
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
                self.removed_game = None;
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
            hint(ui, detail);
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
                ui.label(RichText::new("Show crosshair").semi());
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::zoom_for_percent;

    #[test]
    fn text_size_percent_becomes_zoom_within_settings_range() {
        assert_eq!(zoom_for_percent(100), 1.0);
        assert_eq!(zoom_for_percent(150), 1.5);
        assert_eq!(zoom_for_percent(225), 2.25);
        // A value outside the range, or a junk one, lands on the nearest end.
        assert_eq!(zoom_for_percent(0), 1.0);
        assert_eq!(zoom_for_percent(u32::MAX), 2.25);
    }
}
