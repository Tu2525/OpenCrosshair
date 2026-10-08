use super::widgets::{
    card, check, draw_fit, form, ghost, heading, hint, name_it, primary, row, segment, texture,
};
use super::{App, Page, pal};
use crate::config::{self, Crosshair, MAX_GRID, MAX_IMAGE, MAX_SCALE, Mode};
use crate::{overlay, picture};
use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Rect, RichText, Sense, Slider, Stroke, StrokeKind,
    pos2, vec2,
};
use egui_phosphor::bold as icon;

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

impl App {
    /// The Crosshair page's heading, and the preview when it isn't in its own column.
    pub(super) fn crosshair_header(&mut self, ui: &mut egui::Ui, with_preview: bool) {
        heading(
            ui,
            "Crosshair",
            "Changes show on the overlay as you make them.",
        );
        if with_preview {
            self.preview(ui, vec2(ui.available_width(), 150.0), false);
        }
    }

    pub(super) fn crosshair_body(&mut self, ui: &mut egui::Ui) {
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
    pub(super) fn preview(&mut self, ui: &mut egui::Ui, size: egui::Vec2, stacked: bool) {
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
            hint(
                ui,
                "Or drop a picture onto this window. PNG with a transparent background works best.",
            );
            if let Some(e) = &self.image_error {
                ui.label(
                    RichText::new(format!("{}  {e}", icon::WARNING))
                        .small()
                        .color(pal().yellow.1),
                );
            }
            ui.add_space(4.0);
            form(ui, "image_opts", |ui| {
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
    pub(super) fn use_image(&mut self, path: &std::path::Path) {
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
            form(ui, "canvas_opts", |ui| {
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
                        let undo = egui::Button::new(format!("{}  Undo", icon::ARROW_U_UP_LEFT));
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
            hint(
                ui,
                format!(
                    "{} Left paint    {} Right erase    {} Middle pick colour",
                    icon::PAINT_BRUSH,
                    icon::ERASER,
                    icon::EYEDROPPER
                ),
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
        form(ui, "lines", |ui| {
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
        form(ui, "extras", |ui| {
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
