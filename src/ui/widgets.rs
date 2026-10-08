use super::{Semi, pal, semibold};
use crate::config::Crosshair;
use crate::render;
use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Painter, Pos2, Rect, RichText, Sense, Stroke,
    StrokeKind, TextureHandle, pos2, vec2,
};
use std::collections::HashMap;

// ---------- small building blocks ----------

/// Flat card: hairline border, no fill change, and a quiet semibold caption in sentence case.
pub(super) fn card<R>(ui: &mut egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
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
pub(super) fn empty_state(ui: &mut egui::Ui, icon: &str, title: &str, body: &str) {
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
pub(super) fn nav_item(ui: &mut egui::Ui, icon: &str, label: &str, on: bool) -> egui::Response {
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
pub(super) fn segment(
    ui: &mut egui::Ui,
    text: String,
    on: bool,
    size: egui::Vec2,
) -> egui::Response {
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
pub(super) fn name_it(r: egui::Response, label: impl ToString) -> egui::Response {
    let label = label.to_string();
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label.clone()));
    r
}

pub(super) fn row(ui: &mut egui::Ui, label: &str, add: impl FnOnce(&mut egui::Ui)) {
    ui.label(RichText::new(label).color(pal().muted));
    add(ui);
    ui.end_row();
}

pub(super) fn heading(ui: &mut egui::Ui, title: &str, sub: &str) {
    ui.label(RichText::new(title).heading().extra_letter_spacing(-0.4));
    ui.label(RichText::new(sub).color(pal().muted));
    ui.add_space(18.0);
}

/// A checkbox whose box you can actually see against the page.
pub(super) fn check(
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
pub(super) fn switch(ui: &mut egui::Ui, on: &mut bool) -> egui::Response {
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
pub(super) fn primary(ui: &mut egui::Ui, enabled: bool, text: &str) -> egui::Response {
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
pub(super) fn ghost(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let r = ui
        .add(egui::Button::new(RichText::new(text).color(pal().muted)).frame_when_inactive(false));
    name_it(r, plain(text))
}

/// Small uppercase pill.
pub(super) fn badge(ui: &mut egui::Ui, text: &str, (bg, fg): (Color32, Color32)) {
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

/// A rendered crosshair, and where its centre is inside the texture.
#[derive(Clone)]
pub(super) struct Tex {
    pub(super) handle: TextureHandle,
    centre: f32, // texels; see render::Image::centre
}

/// Crosshair texture for `key`, re-rendered only when the crosshair changed.
pub(super) fn texture(
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
pub(super) fn draw_fit(p: &Painter, t: &Tex, at: Pos2, max: f32) -> f32 {
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
pub(super) fn thumb(ui: &mut egui::Ui, t: &Tex, size: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 6, Color32::from_rgb(10, 10, 10));
    p.rect_stroke(rect, 6, Stroke::new(1.0, pal().border), StrokeKind::Inside);
    draw_fit(&p, t, rect.center(), size - 10.0);
}
