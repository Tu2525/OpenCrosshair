//! Pictures for image crosshairs: decoding with the Windows Imaging Component that ships with
//! Windows (PNG, JPEG, BMP, GIF, TIFF, ICO, WebP), and the standard dialog for picking one.

use crate::render::Image;
use std::path::{Path, PathBuf};
use windows::Win32::Foundation::{GENERIC_READ, HWND};
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::System::Com::*;
use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows::Win32::UI::Shell::{
    FileOpenDialog, FileSaveDialog, IFileOpenDialog, IFileSaveDialog, SIGDN_FILESYSPATH,
};
use windows::core::{HSTRING, w};

thread_local! {
    // Both the overlay and the settings window decode pictures; COM must be set up per thread.
    static COM: () = unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    };
}

/// Decode a picture and scale it so its longer side is `size` pixels, centred on a transparent
/// square so it centres like every other crosshair. Premultiplied BGRA, as the overlay draws.
pub fn load(path: &Path, size: u32) -> Option<Image> {
    COM.with(|_| ());
    unsafe {
        let factory: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()?;
        let decoder = factory
            .CreateDecoderFromFilename(
                &HSTRING::from(path),
                None,
                GENERIC_READ,
                WICDecodeMetadataCacheOnDemand,
            )
            .ok()?;
        let frame = decoder.GetFrame(0).ok()?;
        let (mut w, mut h) = (0, 0);
        frame.GetSize(&mut w, &mut h).ok()?;
        if w == 0 || h == 0 {
            return None;
        }
        let k = size as f64 / w.max(h) as f64;
        let (tw, th) = (
            ((w as f64 * k).round() as u32).max(1),
            ((h as f64 * k).round() as u32).max(1),
        );
        // Blown up, keep pixel art crisp; shrunk, use the smooth filter.
        let filter = if k >= 1.0 {
            WICBitmapInterpolationModeNearestNeighbor
        } else {
            WICBitmapInterpolationModeHighQualityCubic
        };
        let scaler = factory.CreateBitmapScaler().ok()?;
        scaler.Initialize(&frame, tw, th, filter).ok()?;
        let bgra = WICConvertBitmapSource(&GUID_WICPixelFormat32bppPBGRA, &scaler).ok()?;
        let mut bytes = vec![0u8; (tw * th * 4) as usize];
        bgra.CopyPixels(std::ptr::null(), tw * 4, &mut bytes).ok()?;

        let side = tw.max(th) as usize;
        let (ox, oy) = ((side - tw as usize) / 2, (side - th as usize) / 2);
        let mut px = vec![0u32; side * side];
        for (y, row) in bytes.chunks_exact(tw as usize * 4).enumerate() {
            for (x, p) in row.chunks_exact(4).enumerate() {
                px[(y + oy) * side + x + ox] = u32::from_le_bytes([p[0], p[1], p[2], p[3]]);
            }
        }
        Some(Image {
            w: side,
            h: side,
            px,
            centre: side as f32 / 2.0,
        })
    }
}

/// The standard Windows open dialog, filtered to pictures. None if you cancel.
pub fn pick(owner: Option<HWND>) -> Option<PathBuf> {
    COM.with(|_| ());
    unsafe {
        let dialog: IFileOpenDialog =
            CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let filter = [COMDLG_FILTERSPEC {
            pszName: w!("Pictures"),
            pszSpec: w!("*.png;*.jpg;*.jpeg;*.bmp;*.gif;*.ico;*.tif;*.tiff;*.webp"),
        }];
        dialog.SetFileTypes(&filter).ok()?;
        dialog.SetTitle(w!("Choose a crosshair image")).ok()?;
        dialog.Show(owner).ok()?; // cancelling comes back as an error
        let name = dialog
            .GetResult()
            .ok()?
            .GetDisplayName(SIGDN_FILESYSPATH)
            .ok()?;
        let path = name.to_string().ok();
        CoTaskMemFree(Some(name.0 as _));
        path.map(PathBuf::from)
    }
}

/// The open dialog for importing a crosshair share file. None if you cancel.
pub fn pick_share(owner: Option<HWND>) -> Option<PathBuf> {
    COM.with(|_| ());
    unsafe {
        let dialog: IFileOpenDialog =
            CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let filter = [
            COMDLG_FILTERSPEC {
                pszName: w!("OpenCrosshair files"),
                pszSpec: w!("*.opencrosshair"),
            },
            COMDLG_FILTERSPEC {
                pszName: w!("All files"),
                pszSpec: w!("*.*"),
            },
        ];
        dialog.SetFileTypes(&filter).ok()?;
        dialog.SetTitle(w!("Import crosshairs")).ok()?;
        dialog.Show(owner).ok()?; // cancelling comes back as an error
        let name = dialog
            .GetResult()
            .ok()?
            .GetDisplayName(SIGDN_FILESYSPATH)
            .ok()?;
        let path = name.to_string().ok();
        CoTaskMemFree(Some(name.0 as _));
        path.map(PathBuf::from)
    }
}

/// The save dialog for exporting a crosshair share file, suggesting `default_name`. The path
/// gets the .opencrosshair extension if you typed none. None if you cancel.
pub fn save_share(owner: Option<HWND>, default_name: &str) -> Option<PathBuf> {
    COM.with(|_| ());
    unsafe {
        let dialog: IFileSaveDialog =
            CoCreateInstance(&FileSaveDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let filter = [COMDLG_FILTERSPEC {
            pszName: w!("OpenCrosshair files"),
            pszSpec: w!("*.opencrosshair"),
        }];
        dialog.SetFileTypes(&filter).ok()?;
        dialog.SetFileName(&HSTRING::from(default_name)).ok()?;
        dialog.SetDefaultExtension(w!("opencrosshair")).ok()?;
        dialog.SetTitle(w!("Export crosshairs")).ok()?;
        dialog.Show(owner).ok()?; // cancelling comes back as an error
        let name = dialog
            .GetResult()
            .ok()?
            .GetDisplayName(SIGDN_FILESYSPATH)
            .ok()?;
        let text = name.to_string().ok();
        CoTaskMemFree(Some(name.0 as _));
        let mut path = PathBuf::from(text?);
        if path.extension().is_none() {
            path.set_extension("opencrosshair");
        }
        Some(path)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn loads_and_centres_a_picture() {
        // A 4x2 red 24-bit BMP, written by hand so the test needs no image files.
        let (w, h) = (4u32, 2u32);
        let row = (w * 3).div_ceil(4) * 4;
        let mut bmp = b"BM".to_vec();
        for v in [54 + row * h, 0, 54, 40, w, h] {
            bmp.extend(v.to_le_bytes());
        }
        bmp.extend(1u16.to_le_bytes());
        bmp.extend(24u16.to_le_bytes());
        bmp.extend([0u8; 24]);
        for _ in 0..h {
            for _ in 0..w {
                bmp.extend([0, 0, 255]); // BGR
            }
            bmp.extend(vec![0u8; (row - w * 3) as usize]);
        }
        let path = std::env::temp_dir().join("opencrosshair-test.bmp");
        std::fs::write(&path, bmp).unwrap();

        // Scaled to 8 wide it's 8x4, centred in an 8x8 square: rows 2..6 red, the rest clear.
        let img = super::load(&path, 8).expect("WIC decodes BMP");
        let _ = std::fs::remove_file(&path);
        assert_eq!((img.w, img.h, img.centre), (8, 8, 4.0));
        assert_eq!(img.px[8 + 3], 0, "row 1 is padding");
        assert_eq!(img.px[2 * 8 + 3], 0xFFFF0000, "row 2 is opaque red");
        assert_eq!(img.px[5 * 8 + 7], 0xFFFF0000, "row 5 is opaque red");
        assert_eq!(img.px[6 * 8], 0, "row 6 is padding");
    }
}
