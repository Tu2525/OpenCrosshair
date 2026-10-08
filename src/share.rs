//! Preset files: export a set of presets (plus the pictures they use) to one JSON file, and
//! import such a file back. The UI calls these; nothing here touches the window.

use crate::config::{self, Crosshair, Mode};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;

const VERSION: u64 = 1;
const MAX_TEXT: usize = 5 << 20;
const MAX_PRESETS: usize = 100;
const IMAGE_EXTS: [&str; 9] = [
    "png", "jpg", "jpeg", "bmp", "gif", "ico", "tif", "tiff", "webp",
];
const NOT_OURS: &str = "That isn't an OpenCrosshair file.";

/// Pretty JSON: {"opencrosshair":1,"presets":{..},"images":{"<file>":"<base64>"}}.
pub fn export(presets: &BTreeMap<String, Crosshair>) -> String {
    let mut images: BTreeMap<String, String> = BTreeMap::new();
    for c in presets.values().filter(|c| c.mode == Mode::Image) {
        let Some(file) = Path::new(&c.image).file_name().and_then(|f| f.to_str()) else {
            continue;
        };
        if images.contains_key(file) {
            continue;
        }
        // A picture that has gone missing just isn't included.
        if let Ok(bytes) = std::fs::read(config::images_dir().join(file)) {
            images.insert(file.to_string(), base64_encode(&bytes));
        }
    }
    let doc = serde_json::json!({
        "opencrosshair": VERSION,
        "presets": presets,
        "images": images,
    });
    serde_json::to_string_pretty(&doc).unwrap_or_default()
}

/// The presets in `text` that can be added alongside `existing`. Nothing is saved by this
/// except the pictures, which are copied into our images folder.
pub fn import(
    text: &str,
    existing: &BTreeMap<String, Crosshair>,
) -> Result<BTreeMap<String, Crosshair>, String> {
    if text.len() > MAX_TEXT {
        return Err("That file is over 5 MB, so it can't be an OpenCrosshair file.".into());
    }
    let doc: Value = serde_json::from_str(text).map_err(|_| NOT_OURS.to_string())?;
    let version = doc
        .get("opencrosshair")
        .and_then(Value::as_u64)
        .filter(|&v| v >= 1)
        .ok_or_else(|| NOT_OURS.to_string())?;
    if version > VERSION {
        return Err(
            "That file was made by a newer version of OpenCrosshair. Update OpenCrosshair to open it."
                .into(),
        );
    }
    let images: BTreeMap<String, String> = doc
        .get("images")
        .and_then(|i| serde_json::from_value(i.clone()).ok())
        .unwrap_or_default();

    let mut out: BTreeMap<String, Crosshair> = BTreeMap::new();
    let presets = doc.get("presets").and_then(Value::as_object);
    for (raw_name, value) in presets.into_iter().flatten() {
        if out.len() >= MAX_PRESETS {
            break;
        }
        let base = raw_name.trim();
        if base.is_empty() {
            continue;
        }
        let Ok(mut c) = serde_json::from_value::<Crosshair>(value.clone()) else {
            continue;
        };
        c.sanitize();
        if c.mode == Mode::Image {
            c.image = embedded_image(&images, &c.image);
        }
        let mut name = base.to_string();
        let mut n = 1;
        while existing.contains_key(&name) || out.contains_key(&name) {
            n += 1;
            name = format!("{base} ({n})");
        }
        out.insert(name, c);
    }
    if out.is_empty() {
        return Err("That file doesn't contain any presets.".into());
    }
    Ok(out)
}

/// Save the picture a preset refers to. Empty when it's missing, not a picture, or of a type we
/// don't take.
fn embedded_image(images: &BTreeMap<String, String>, file: &str) -> String {
    let ext = Path::new(file)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    if !IMAGE_EXTS.contains(&ext.as_str()) {
        return String::new();
    }
    let Some(bytes) = images.get(file).and_then(|b| base64_decode(b)) else {
        return String::new();
    };
    if !looks_like_image(&bytes) {
        return String::new();
    }
    config::import_bytes(bytes, &ext).unwrap_or_default()
}

/// Checks the file's first bytes against the formats we accept.
fn looks_like_image(b: &[u8]) -> bool {
    b.starts_with(b"\x89PNG")
        || b.starts_with(&[0xFF, 0xD8, 0xFF])
        || b.starts_with(b"BM")
        || b.starts_with(b"GIF8")
        || b.starts_with(&[0, 0, 1, 0])
        || b.starts_with(b"II*\0")
        || b.starts_with(b"MM\0*")
        || (b.starts_with(b"RIFF") && b.get(8..12) == Some(&b"WEBP"[..]))
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64_encode(input: &[u8]) -> String {
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = b0 << 16 | b1 << 8 | b2;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(B64[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let bytes = text.as_bytes();
    if !bytes.len().is_multiple_of(4) {
        return None;
    }
    let value = |c: u8| -> Option<u32> {
        let v: u8 = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        };
        Some(v as u32)
    };
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    for quad in bytes.chunks(4) {
        let pad = quad.iter().rev().take_while(|&&c| c == b'=').count();
        if pad > 2 {
            return None;
        }
        let mut n = 0u32;
        for (i, &c) in quad.iter().enumerate() {
            // Padding can only be trailing, so anything before it has to be a real digit.
            let v = if i >= 4 - pad { 0 } else { value(c)? };
            n = n << 6 | v;
        }
        out.push((n >> 16) as u8);
        if pad < 2 {
            out.push((n >> 8) as u8);
        }
        if pad < 1 {
            out.push(n as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn presets(list: &[(&str, Crosshair)]) -> BTreeMap<String, Crosshair> {
        list.iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }

    #[test]
    fn base64_round_trips_every_padding_case() {
        assert_eq!(base64_encode(b"M"), "TQ==");
        assert_eq!(base64_encode(b"Ma"), "TWE=");
        assert_eq!(base64_encode(b"Man"), "TWFu");
        for len in 0..=9 {
            let data: Vec<u8> = (0..len).map(|i| (i * 37 + 200) as u8).collect();
            let text = base64_encode(&data);
            assert_eq!(text.len() % 4, 0);
            assert_eq!(
                base64_decode(&text).as_deref(),
                Some(&data[..]),
                "len {len}"
            );
        }
        assert!(base64_decode("TW=u").is_none());
        assert!(base64_decode("TWFu!").is_none());
    }

    #[test]
    fn export_then_import_keeps_lines_and_pixels_presets() {
        let lines = Crosshair::default();
        let mut pixels = Crosshair {
            mode: Mode::Pixels,
            ..Default::default()
        };
        pixels.pixels[0] = [255, 0, 0, 255];
        let original = presets(&[("Classic", lines), ("Pixel", pixels)]);

        let text = export(&original);
        let back = import(&text, &BTreeMap::new()).unwrap();
        assert!(back == original, "round trip changed a preset");
    }

    #[test]
    fn name_clashes_get_a_number() {
        let existing = presets(&[("Classic", Crosshair::default())]);
        let text = export(&presets(&[
            ("Classic", Crosshair::default()),
            (" Classic ", Crosshair::default()),
            ("   ", Crosshair::default()),
        ]));
        let got = import(&text, &existing).unwrap();
        let names: Vec<&str> = got.keys().map(String::as_str).collect();
        assert_eq!(names, ["Classic (2)", "Classic (3)"]);
    }

    #[test]
    fn garbage_and_newer_versions_are_refused() {
        let none = BTreeMap::new();
        assert_eq!(import("not json", &none).err().as_deref(), Some(NOT_OURS));
        assert_eq!(
            import(r#"{"presets":{}}"#, &none).err().as_deref(),
            Some(NOT_OURS)
        );
        assert!(import(r#"{"opencrosshair":2,"presets":{"a":{}}}"#, &none).is_err());
        assert!(import(r#"{"opencrosshair":1,"presets":{}}"#, &none).is_err());
    }

    #[test]
    fn a_broken_embedded_picture_is_dropped_not_fatal() {
        // Not a picture at all, so nothing gets written to disk.
        let text = format!(
            r#"{{"opencrosshair":1,"presets":{{"Pic":{{"mode":"Image","image":"x.png"}}}},"images":{{"x.png":"{}"}}}}"#,
            base64_encode(b"not an image")
        );
        let got = import(&text, &BTreeMap::new()).unwrap();
        assert_eq!(got["Pic"].image, "");
    }
}
