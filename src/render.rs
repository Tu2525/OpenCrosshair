use crate::config::{self, Crosshair, Mode};

/// Premultiplied BGRA pixels (u32 = 0xAARRGGBB), top-down.
pub struct Image {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u32>,
    /// Where the crosshair's centre is, in pixel-edge coordinates (same on both axes).
    /// A whole number is the corner between pixels, x.5 the middle of a pixel. It isn't always
    /// the middle of the image, so anything that centres the crosshair should use this.
    pub centre: f32,
}

pub fn render(c: &Crosshair) -> Image {
    match c.mode {
        Mode::Lines => lines(c),
        Mode::Pixels => pixels(c),
        Mode::Image => image(c),
    }
}

/// A picture the user imported. Nothing to draw if it's missing or won't decode.
fn image(c: &Crosshair) -> Image {
    let path = config::images_dir().join(&c.image);
    let loaded = (!c.image.is_empty())
        .then(|| crate::picture::load(&path, c.image_size))
        .flatten();
    let mut img = loaded.unwrap_or(Image {
        w: 1,
        h: 1,
        px: vec![0],
        centre: 0.5,
    });
    if c.image_opacity < 100 {
        // Premultiplied, so fading is just scaling every channel.
        let k = c.image_opacity as f32 / 100.0;
        for p in &mut img.px {
            *p = u32::from_le_bytes(p.to_le_bytes().map(|v| (v as f32 * k).round() as u8));
        }
    }
    img
}

fn pack(rgb: [f32; 3], a: f32) -> u32 {
    let b = |v: f32| (v * 255.0).round().clamp(0.0, 255.0) as u32;
    b(a) << 24 | b(rgb[0]) << 16 | b(rgb[1]) << 8 | b(rgb[2])
}

fn unit(c: [u8; 4]) -> ([f32; 3], f32) {
    let f = |v: u8| v as f32 / 255.0;
    ([f(c[0]), f(c[1]), f(c[2])], f(c[3]))
}

fn pixels(c: &Crosshair) -> Image {
    let (n, s) = (c.grid as usize, c.scale.max(1) as usize);
    let w = n * s;
    let mut px = vec![0; w * w];
    for y in 0..w {
        for x in 0..w {
            let (rgb, a) = unit(c.pixels.get((y / s) * n + x / s).copied().unwrap_or([0; 4]));
            px[y * w + x] = pack(rgb.map(|v| v * a), a);
        }
    }
    Image {
        w,
        h: w,
        px,
        centre: w as f32 / 2.0,
    }
}

/// Two coverage masks: the fill and the outline underneath it. Using max-coverage per mask
/// means overlapping shapes never double-blend.
struct Masks {
    w: i32,
    o: i32,
    fill: Vec<f32>,
    line: Vec<f32>,
}

impl Masks {
    fn put(v: &mut [f32], w: i32, x: i32, y: i32, a: f32) {
        if (0..w).contains(&x) && (0..w).contains(&y) {
            let p = &mut v[(y * w + x) as usize];
            *p = p.max(a);
        }
    }

    /// Half-open rect [x0,x1) x [y0,y1).
    fn rect(&mut self, x0: i32, y0: i32, x1: i32, y1: i32) {
        let o = self.o;
        if o > 0 {
            for y in y0 - o..y1 + o {
                for x in x0 - o..x1 + o {
                    Self::put(&mut self.line, self.w, x, y, 1.0);
                }
            }
        }
        for y in y0..y1 {
            for x in x0..x1 {
                Self::put(&mut self.fill, self.w, x, y, 1.0);
            }
        }
    }
}

fn lines(c: &Crosshair) -> Image {
    let (o, t, gap, len) = (
        c.outline as i32,
        c.thickness.max(1) as i32,
        c.gap as i32,
        c.length as i32,
    );
    let (r, ct) = (c.circle_radius as f32, c.circle_thickness.max(1) as f32);
    let mut half = 1;
    if len > 0 {
        half = half.max(gap + len + t);
    }
    if c.circle {
        half = half.max((r + ct) as i32 + 1);
    }
    if c.dot {
        half = half.max(c.dot_size as i32);
    }
    let half = half + o + 1;
    let w = 2 * half + 1;
    let cx = half;
    let mut m = Masks {
        w,
        o,
        fill: vec![0.0; (w * w) as usize],
        line: vec![0.0; (w * w) as usize],
    };

    let s = cx - t / 2; // centre band is [s, s+t)
    if len > 0 {
        m.rect(s + t + gap, s, s + t + gap + len, s + t); // right
        m.rect(s - gap - len, s, s - gap, s + t); // left
        m.rect(s, s + t + gap, s + t, s + t + gap + len); // down
        if !c.t_style {
            m.rect(s, s - gap - len, s + t, s - gap); // up
        }
        if gap == 0 {
            m.rect(s, s, s + t, s + t);
        }
    }

    // The dot and circle are drawn around the exact centre of the lines: a pixel corner for
    // even thickness, a pixel's middle for odd. (Centring them on pixel cx put them half a
    // pixel off whenever the line thickness was even.) Without lines, the dot sets the centre.
    let d = c.dot_size.max(1) as i32;
    let basis = if len > 0 {
        t
    } else if c.dot {
        d
    } else {
        2
    };
    let centre = (cx - basis / 2) as f32 + basis as f32 / 2.0;
    if c.dot || c.circle {
        for y in 0..w {
            for x in 0..w {
                // Distance from this pixel's middle to the nearest shape edge, negative inside.
                let (dx, dy) = (x as f32 + 0.5 - centre, y as f32 + 0.5 - centre);
                let mut e = f32::MAX;
                if c.dot {
                    e = e.min(dx.abs().max(dy.abs()) - d as f32 / 2.0);
                }
                if c.circle {
                    e = e.min(((dx * dx + dy * dy).sqrt() - r).abs() - ct / 2.0);
                }
                // Edges that land on pixel boundaries come out crisp; anything else is anti-aliased.
                Masks::put(&mut m.fill, w, x, y, (0.5 - e).clamp(0.0, 1.0));
                if o > 0 {
                    Masks::put(&mut m.line, w, x, y, (0.5 - e + o as f32).clamp(0.0, 1.0));
                }
            }
        }
    }

    let (fc, fa) = unit(c.color);
    let (lc, la) = unit(c.outline_color);
    let px = m
        .fill
        .iter()
        .zip(&m.line)
        .map(|(&f, &l)| {
            let (f, l) = (f * fa, l * la * (1.0 - f * fa));
            pack([0, 1, 2].map(|i| fc[i] * f + lc[i] * l), f + l)
        })
        .collect();
    Image {
        w: w as usize,
        h: w as usize,
        px,
        centre,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders() {
        // Lines: centre is a hole (gap), arm is green, pixel beside the arm is black outline.
        let c = Crosshair::default();
        let img = render(&c);
        let (w, cx) = (img.w, img.w / 2);
        let at = |x: usize, y: usize| img.px[y * w + x];
        assert_eq!(at(cx, cx), 0);
        assert_eq!(at(cx + 5, cx), 0xFF00FF00);
        assert_eq!(at(cx + 5, cx - 2), 0xFF000000);
        assert_eq!(at(0, 0), 0);

        // Pixels: 2x2 grid scaled x2 -> 4x4, half-transparent red premultiplied.
        // Indexes below are row * width + column.
        let mut c = Crosshair {
            mode: Mode::Pixels,
            grid: 2,
            scale: 2,
            ..Default::default()
        };
        c.pixels = vec![[255, 0, 0, 128], [0; 4], [0; 4], [0; 4]];
        let img = render(&c);
        assert_eq!((img.w, img.h), (4, 4));
        assert_eq!(img.px[4 + 1], 0x80800000);
        assert_eq!(img.px[4 + 2], 0);

        // Grid resize keeps the drawing centred.
        c.resize_grid(4);
        assert_eq!(c.pixels[4 + 1], [255, 0, 0, 128]);
    }

    #[test]
    fn dot_and_circle_share_the_lines_centre() {
        // Mirror the whole image across both axes; any shape off-centre breaks the match.
        for (t, d) in [(2, 2), (2, 3), (2, 4), (3, 3), (3, 2), (1, 1), (4, 1)] {
            let c = Crosshair {
                thickness: t,
                dot: true,
                dot_size: d,
                circle: true,
                circle_radius: 9,
                ..Default::default()
            };
            let img = render(&c);
            let (w, cx) = (img.w as isize, img.w as isize / 2);
            // Axis: the corner before pixel cx for even thickness, pixel cx itself for odd.
            let mirror = |i: isize| {
                if t % 2 == 0 {
                    2 * cx - 1 - i
                } else {
                    2 * cx - i
                }
            };
            // ...which is what the image reports as its centre.
            let axis = if t % 2 == 0 {
                cx as f32
            } else {
                cx as f32 + 0.5
            };
            assert_eq!(img.centre, axis, "thickness {t}");
            let at = |x: isize, y: isize| img.px[(y * w + x) as usize];
            for y in 0..w {
                for x in 0..w {
                    let (mx, my) = (mirror(x), mirror(y));
                    if (0..w).contains(&mx) && (0..w).contains(&my) {
                        assert_eq!(
                            at(x, y),
                            at(mx, y),
                            "thickness {t}, dot {d}: ({x},{y}) vs ({mx},{y})"
                        );
                        assert_eq!(
                            at(x, y),
                            at(x, my),
                            "thickness {t}, dot {d}: ({x},{y}) vs ({x},{my})"
                        );
                    }
                }
            }
        }
    }
}
