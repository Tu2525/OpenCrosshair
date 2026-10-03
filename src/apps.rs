//! Which app a window belongs to, and the name and icon to show for it in the games list.
//! Everything comes from Windows itself: the process list, the exe's version info and its icon.

use std::ffi::c_void;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::Storage::FileSystem::{
    GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
};
use windows::Win32::System::Diagnostics::ToolHelp::*;
use windows::Win32::System::Registry::*;
use windows::Win32::System::Threading::*;
use windows::Win32::UI::Shell::SHDefExtractIconW;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{BOOL, HSTRING, PCWSTR, PWSTR, w};

// Windows keeps compatibility settings per exe here, including the "Disable fullscreen
// optimizations" box on the exe's Properties > Compatibility tab.
const LAYERS: PCWSTR = w!(r"Software\Microsoft\Windows NT\CurrentVersion\AppCompatFlags\Layers");
const NO_FSO: &str = "DISABLEDXMAXIMIZEDWINDOWEDMODE";

/// Whether a game gets fullscreen optimizations. Without them its fullscreen is true exclusive
/// mode, which nothing can draw over short of hooking into the game.
#[derive(Clone, Copy, PartialEq)]
pub enum Fso {
    On,
    OffForYou,      // our own account's setting: we can switch it back on
    OffForEveryone, // set machine-wide: needs admin, so we can only point the way
}

fn layers(root: HKEY, path: &str) -> Option<String> {
    unsafe {
        let name = HSTRING::from(path);
        let mut len = 0u32;
        RegGetValueW(
            root,
            LAYERS,
            &name,
            RRF_RT_REG_SZ,
            None,
            None,
            Some(&mut len),
        )
        .ok()
        .ok()?;
        let mut buf = vec![0u16; len as usize / 2];
        let data = Some(buf.as_mut_ptr() as *mut c_void);
        RegGetValueW(
            root,
            LAYERS,
            &name,
            RRF_RT_REG_SZ,
            None,
            data,
            Some(&mut len),
        )
        .ok()
        .ok()?;
        Some(
            String::from_utf16_lossy(&buf)
                .trim_end_matches('\0')
                .to_string(),
        )
    }
}

pub fn fullscreen_optimizations(path: &str) -> Fso {
    let off = |root| {
        layers(root, path)
            .is_some_and(|v| v.split_whitespace().any(|f| f.eq_ignore_ascii_case(NO_FSO)))
    };
    if off(HKEY_CURRENT_USER) {
        Fso::OffForYou
    } else if off(HKEY_LOCAL_MACHINE) {
        Fso::OffForEveryone
    } else {
        Fso::On
    }
}

/// Untick "Disable fullscreen optimizations" for this exe, for our account only. Any other
/// compatibility settings on it are left alone.
pub fn enable_fullscreen_optimizations(path: &str) -> bool {
    let Some(value) = layers(HKEY_CURRENT_USER, path) else {
        return false;
    };
    let rest: Vec<&str> = value
        .split_whitespace()
        .filter(|f| !f.eq_ignore_ascii_case(NO_FSO))
        .collect();
    let name = HSTRING::from(path);
    unsafe {
        // A lone "~" only marks the entry as user-set; with nothing after it, drop the entry.
        if rest.iter().all(|f| *f == "~") {
            RegDeleteKeyValueW(HKEY_CURRENT_USER, LAYERS, &name).is_ok()
        } else {
            let data: Vec<u16> = rest.join(" ").encode_utf16().chain([0]).collect();
            let bytes = (data.len() * 2) as u32;
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                LAYERS,
                &name,
                REG_SZ.0,
                Some(data.as_ptr() as _),
                bytes,
            )
            .is_ok()
        }
    }
}

pub struct App {
    /// Lowercase file name, e.g. "cs2.exe". This is what games are matched on.
    pub exe: String,
    /// Full path, when Windows will tell us. Empty otherwise.
    pub path: String,
    /// Title of its main window (only filled in by `running`).
    pub title: String,
}

pub fn own_exe() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|p| Some(p.file_name()?.to_string_lossy().to_lowercase()))
        .unwrap_or_default()
}

/// The app that owns `hwnd`. Both fields are empty if it can't be found.
pub fn of_window(hwnd: HWND) -> App {
    unsafe {
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return App {
                exe: String::new(),
                path: String::new(),
                title: String::new(),
            };
        }
        if let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            let mut buf = [0u16; 1024];
            let mut len = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(
                h,
                PROCESS_NAME_WIN32,
                PWSTR(buf.as_mut_ptr()),
                &mut len,
            );
            let _ = CloseHandle(h);
            if ok.is_ok() {
                let path = String::from_utf16_lossy(&buf[..len as usize]);
                let exe = path.rsplit('\\').next().unwrap_or("").to_lowercase();
                return App {
                    exe,
                    path,
                    title: String::new(),
                };
            }
        }
        // Some anti-cheats refuse even a limited handle. The process list still has the name.
        App {
            exe: exe_by_pid(pid),
            path: String::new(),
            title: String::new(),
        }
    }
}

unsafe fn exe_by_pid(pid: u32) -> String {
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return String::new();
        };
        let mut e = PROCESSENTRY32W {
            dwSize: size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut name = String::new();
        let mut more = Process32FirstW(snap, &mut e).is_ok();
        while more {
            if e.th32ProcessID == pid {
                let n = e
                    .szExeFile
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(e.szExeFile.len());
                name = String::from_utf16_lossy(&e.szExeFile[..n]).to_lowercase();
                break;
            }
            more = Process32NextW(snap, &mut e).is_ok();
        }
        let _ = CloseHandle(snap);
        name
    }
}

/// Apps with a visible top-level window, for the "add game" picker.
pub fn running() -> Vec<App> {
    unsafe extern "system" fn each(h: HWND, out: LPARAM) -> BOOL {
        unsafe {
            let out = &mut *(out.0 as *mut Vec<App>);
            // Skip our own windows before asking for titles: for windows in this process that
            // means a message to their thread, which might be busy.
            let mut pid = 0;
            GetWindowThreadProcessId(h, Some(&mut pid));
            if pid != GetCurrentProcessId()
                && IsWindowVisible(h).as_bool()
                && GetWindowTextLengthW(h) > 0
                && GetWindow(h, GW_OWNER).is_err()
            {
                let mut app = of_window(h);
                if !app.exe.is_empty() && !out.iter().any(|a| a.exe == app.exe) {
                    let mut buf = [0u16; 256];
                    let n = GetWindowTextW(h, &mut buf) as usize;
                    app.title = String::from_utf16_lossy(&buf[..n]);
                    out.push(app);
                }
            }
            true.into()
        }
    }
    let mut out: Vec<App> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM(&mut out as *mut _ as isize));
    }
    let skip = [
        own_exe().as_str(),
        "explorer.exe",
        "applicationframehost.exe",
        "textinputhost.exe",
    ]
    .map(String::from);
    out.retain(|a| !skip.contains(&a.exe));
    out.sort_by(|a, b| a.exe.cmp(&b.exe));
    out
}

/// What to call an app: the description in its version info ("Discord", "Brave Browser"), which
/// is what Task Manager shows. Plenty of games leave that empty but name their window after
/// themselves ("Counter-Strike 2"), so the window title comes next. Last resort: the file name
/// without ".exe".
pub fn display_name(exe: &str, path: &str, title: &str) -> String {
    if !path.is_empty()
        && let Some(name) = description(path)
    {
        return name;
    }
    let title = title.trim();
    if !title.is_empty() && title.chars().count() <= 40 {
        return title.to_string();
    }
    let stem = exe.strip_suffix(".exe").unwrap_or(exe);
    let mut chars = stem.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

fn description(path: &str) -> Option<String> {
    unsafe {
        let file = HSTRING::from(path);
        let size = GetFileVersionInfoSizeW(&file, None);
        if size == 0 {
            return None;
        }
        let mut data = vec![0u8; size as usize];
        GetFileVersionInfoW(&file, None, size, data.as_mut_ptr() as *mut c_void).ok()?;
        let query = |key: &str| -> Option<(*const c_void, u32)> {
            let (mut ptr, mut len) = (std::ptr::null_mut(), 0u32);
            let found = VerQueryValueW(
                data.as_ptr() as *const c_void,
                &HSTRING::from(key),
                &mut ptr,
                &mut len,
            );
            (found.as_bool() && len > 0).then_some((ptr as *const c_void, len))
        };
        // Strings are stored per language; use the first language the file lists.
        let (langs, _) = query(r"\VarFileInfo\Translation")?;
        let [lang, codepage] = *(langs as *const [u16; 2]);
        let (text, len) = query(&format!(
            r"\StringFileInfo\{lang:04x}{codepage:04x}\FileDescription"
        ))?;
        let name =
            String::from_utf16_lossy(std::slice::from_raw_parts(text as *const u16, len as usize));
        let name = name
            .trim_matches(|c: char| c == '\0' || c.is_whitespace())
            .to_string();
        (!name.is_empty()).then_some(name)
    }
}

/// The exe's icon as unmultiplied RGBA, about `size` pixels square (whatever the closest size
/// the exe ships is).
pub fn icon(path: &str, size: u32) -> Option<(Vec<u8>, [usize; 2])> {
    unsafe {
        let mut icon = HICON::default();
        SHDefExtractIconW(&HSTRING::from(path), 0, 0, Some(&mut icon), None, size)
            .ok()
            .ok()?;
        if icon.is_invalid() {
            return None;
        }
        let rgba = icon_rgba(icon);
        let _ = DestroyIcon(icon);
        rgba
    }
}

unsafe fn icon_rgba(icon: HICON) -> Option<(Vec<u8>, [usize; 2])> {
    unsafe {
        let mut info = ICONINFO::default();
        GetIconInfo(icon, &mut info).ok()?;
        let mut bm = BITMAP::default();
        let has_colour = !info.hbmColor.is_invalid()
            && GetObjectW(
                info.hbmColor.into(),
                size_of::<BITMAP>() as i32,
                Some(&mut bm as *mut _ as *mut c_void),
            ) != 0;
        let (w, h) = (bm.bmWidth, bm.bmHeight);
        let read = |bitmap: HBITMAP| {
            let mut bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w,
                    biHeight: -h, // top-down
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut px = vec![0u8; (w * h * 4) as usize];
            let dc = GetDC(None);
            GetDIBits(
                dc,
                bitmap,
                0,
                h as u32,
                Some(px.as_mut_ptr() as *mut c_void),
                &mut bmi,
                DIB_RGB_COLORS,
            );
            ReleaseDC(None, dc);
            px
        };
        let result = has_colour.then(|| {
            let mut px = read(info.hbmColor);
            if px.chunks(4).all(|p| p[3] == 0) {
                // Older icons have no alpha channel; their mask says which pixels are see-through.
                let mask = read(info.hbmMask);
                for (p, m) in px.chunks_mut(4).zip(mask.chunks(4)) {
                    p[3] = if m[0] == 0 { 255 } else { 0 };
                }
            }
            for p in px.chunks_mut(4) {
                p.swap(0, 2); // BGRA -> RGBA
            }
            (px, [w as usize, h as usize])
        });
        let _ = DeleteObject(info.hbmColor.into());
        let _ = DeleteObject(info.hbmMask.into());
        result
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn switches_fullscreen_optimizations_back_on() {
        use super::*;
        // A made-up exe, so the test can't touch a real game's settings.
        let exe = r"C:\OpenCrosshair test\not-a-real-game.exe";
        let name = HSTRING::from(exe);
        struct Cleanup<'a>(&'a HSTRING);
        impl Drop for Cleanup<'_> {
            fn drop(&mut self) {
                unsafe {
                    let _ = RegDeleteKeyValueW(HKEY_CURRENT_USER, LAYERS, self.0);
                }
            }
        }
        let _cleanup = Cleanup(&name);
        let set = |v: &str| {
            let data: Vec<u16> = v.encode_utf16().chain([0]).collect();
            let bytes = (data.len() * 2) as u32;
            unsafe {
                RegSetKeyValueW(
                    HKEY_CURRENT_USER,
                    LAYERS,
                    &name,
                    REG_SZ.0,
                    Some(data.as_ptr() as _),
                    bytes,
                )
                .ok()
                .unwrap();
            }
        };

        assert!(fullscreen_optimizations(exe) == Fso::On);
        set("~ DISABLEDXMAXIMIZEDWINDOWEDMODE HIGHDPIAWARE");
        assert!(fullscreen_optimizations(exe) == Fso::OffForYou);
        assert!(enable_fullscreen_optimizations(exe));
        assert!(fullscreen_optimizations(exe) == Fso::On);
        assert_eq!(
            layers(HKEY_CURRENT_USER, exe).as_deref(),
            Some("~ HIGHDPIAWARE"),
            "other settings stay"
        );

        set("~ DISABLEDXMAXIMIZEDWINDOWEDMODE");
        assert!(enable_fullscreen_optimizations(exe));
        assert_eq!(
            layers(HKEY_CURRENT_USER, exe),
            None,
            "an entry with nothing left is removed"
        );
    }

    #[test]
    fn names_and_icons_from_windows() {
        // explorer.exe ships with every copy of Windows, so it makes a stable example.
        let windir = std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".into());
        let explorer = format!(r"{windir}\explorer.exe");
        // Its description depends on the Windows language, but it's never the file-name fallback.
        assert_ne!(
            super::display_name("explorer.exe", &explorer, ""),
            "Explorer"
        );
        // No version info: the window title, then the file name.
        let title = "Counter-Strike 2";
        assert_eq!(
            super::display_name("cs2.exe", "", title),
            "Counter-Strike 2"
        );
        assert_eq!(super::display_name("deadlock.exe", "", ""), "Deadlock");
        let (rgba, [w, h]) = super::icon(&explorer, 32).expect("explorer has an icon");
        assert_eq!(rgba.len(), w * h * 4);
        assert!(
            rgba.chunks(4).any(|p| p[3] > 0),
            "icon shouldn't be fully transparent"
        );
    }
}
