//! Draws the app icon at build time and embeds it (plus version info) into the exe.
//! Keeping the icon as code means there's no .ico file in the repo to keep in sync.

use std::path::Path;

const BG: [f32; 3] = [22.0, 22.0, 21.0];
const FG: [f32; 3] = [150.0, 205.0, 160.0];

fn main() {
    let out = std::env::var("OUT_DIR").unwrap();
    let out = Path::new(&out);

    let sizes = [16, 20, 24, 32, 40, 48, 64, 256];
    let images: Vec<(u32, Vec<u8>)> = sizes.iter().map(|&n| (n, draw(n))).collect();
    std::fs::write(out.join("icon.ico"), ico(&images)).unwrap();
    // Raw 64x64 RGBA for the window/taskbar icon, so the app needs no image decoder.
    std::fs::write(out.join("icon-64.rgba"), draw(64)).unwrap();

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon(out.join("icon.ico").to_str().unwrap())
            .set("ProductName", "OpenCrosshair")
            .set("FileDescription", "OpenCrosshair")
            .set("LegalCopyright", "MIT License")
            .compile()
            .unwrap();
    }
    println!("cargo:rerun-if-changed=build.rs");
}

/// Rounded dark tile with a ring, four ticks and a centre dot. Shapes are signed distance
/// fields, so every size is anti-aliased without supersampling.
fn draw(n: u32) -> Vec<u8> {
    let s = n as f32;
    let mut px = Vec::with_capacity((n * n * 4) as usize);
    for y in 0..n {
        for x in 0..n {
            // Pixel centre in [-0.5, 0.5], y up.
            let (u, v) = ((x as f32 + 0.5) / s - 0.5, 0.5 - (y as f32 + 0.5) / s);
            let cover = |d: f32| (0.5 - d * s).clamp(0.0, 1.0);

            let tile = cover(rounded_box(u, v, 0.5, 0.21));
            let r = (u * u + v * v).sqrt();
            let w = 0.042_f32.max(0.6 / s); // never thinner than ~1px
            let ring = (r - 0.25).abs() - w;
            let ticks = [(0.0, 1.0), (0.0, -1.0), (1.0, 0.0), (-1.0, 0.0)]
                .iter()
                .map(|&(dx, dy)| segment(u, v, dx * 0.12, dy * 0.12, dx * 0.38, dy * 0.38) - w)
                .fold(f32::MAX, f32::min);
            let dot = r - 0.05_f32.max(0.9 / s);
            let mark = cover(ring.min(ticks).min(dot));

            let rgb = [0, 1, 2].map(|i| BG[i] + (FG[i] - BG[i]) * mark);
            px.extend(rgb.map(|c| c.round() as u8));
            px.push((tile * 255.0).round() as u8);
        }
    }
    px
}

fn rounded_box(u: f32, v: f32, half: f32, radius: f32) -> f32 {
    let (qx, qy) = (u.abs() - half + radius, v.abs() - half + radius);
    (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt() + qx.max(qy).min(0.0) - radius
}

fn segment(u: f32, v: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let (pax, pay, bax, bay) = (u - ax, v - ay, bx - ax, by - ay);
    let h = ((pax * bax + pay * bay) / (bax * bax + bay * bay)).clamp(0.0, 1.0);
    ((pax - bax * h).powi(2) + (pay - bay * h).powi(2)).sqrt()
}

/// .ico with 32-bit BMP entries (top-down RGBA in, bottom-up BGRA out).
fn ico(images: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let mut dir = vec![0, 0, 1, 0];
    dir.extend((images.len() as u16).to_le_bytes());
    let mut data = Vec::new();
    let mut offset = 6 + 16 * images.len() as u32;
    for (n, rgba) in images {
        let mut bmp = Vec::new();
        for v in [40u32, *n, n * 2] {
            bmp.extend(v.to_le_bytes()); // header size, width, height (colour + mask)
        }
        bmp.extend(1u16.to_le_bytes());
        bmp.extend(32u16.to_le_bytes());
        bmp.extend([0u8; 24]);
        for row in rgba.chunks(*n as usize * 4).rev() {
            for p in row.chunks(4) {
                bmp.extend([p[2], p[1], p[0], p[3]]);
            }
        }
        bmp.extend(vec![0u8; (n.div_ceil(32) * 4 * n) as usize]); // AND mask, unused with alpha

        let side = if *n >= 256 { 0 } else { *n as u8 };
        dir.extend([side, side, 0, 0]);
        dir.extend(1u16.to_le_bytes());
        dir.extend(32u16.to_le_bytes());
        dir.extend((bmp.len() as u32).to_le_bytes());
        dir.extend(offset.to_le_bytes());
        offset += bmp.len() as u32;
        data.extend(bmp);
    }
    dir.extend(data);
    dir
}
