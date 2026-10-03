//! The overlay: a click-through, always-on-top layered window. Plain Win32, nothing is injected
//! into games. It only redraws when something changes, so it costs nothing while you play.

use crate::config::{Crosshair, Hotkey, Settings};
use crate::render::{self, Image};
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::{Arc, Mutex};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::Diagnostics::ToolHelp::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::*;
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook};
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{BOOL, PCWSTR, w};

pub const TITLE: &str = "OpenCrosshair"; // settings window title

pub static VISIBLE: AtomicBool = AtomicBool::new(true);
/// True while a game from the list is focused. The updater waits for this to clear.
pub static IN_GAME: AtomicBool = AtomicBool::new(false);
/// Whether each hotkey (toggle, menu) registered. False means another app already owns it.
pub static KEY_OK: [AtomicBool; 2] = [AtomicBool::new(true), AtomicBool::new(true)];
pub static STATUS: Mutex<Status> = Mutex::new(Status::Everywhere);

#[derive(Clone, PartialEq)]
pub enum Status {
    Everywhere,
    Hidden,
    Waiting,
    InGame(String),
}

static OVERLAY: AtomicIsize = AtomicIsize::new(0);
const WM_REFRESH: u32 = WM_APP + 1;
const HK_TOGGLE: i32 = 1;
const HK_MENU: i32 = 2;
// Sent between OpenCrosshair processes: a second launch asks us to show the settings,
// the installer and uninstaller ask us to quit.
const MSG_SHOW: PCWSTR = w!("OpenCrosshair.Show");
const MSG_QUIT: PCWSTR = w!("OpenCrosshair.Quit");

/// Ask the overlay thread to re-read settings and redraw.
pub fn refresh() {
    let h = OVERLAY.load(Ordering::Acquire);
    if h != 0 {
        unsafe {
            let _ = PostMessageW(Some(HWND(h as _)), WM_REFRESH, WPARAM(0), LPARAM(0));
        }
    }
}

/// Tell an already running OpenCrosshair to show its settings, or to quit.
pub fn signal_running(quit: bool) {
    unsafe {
        let msg = RegisterWindowMessageW(if quit { MSG_QUIT } else { MSG_SHOW });
        let _ = PostMessageW(Some(HWND_BROADCAST), msg, WPARAM(0), LPARAM(0));
    }
}

pub fn run(settings: Arc<Mutex<Settings>>) {
    unsafe {
        let inst: HINSTANCE = GetModuleHandleW(None).unwrap().into();
        let class = w!("OpenCrosshairOverlay");
        RegisterClassW(&WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: inst,
            lpszClassName: class,
            ..Default::default()
        });
        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            class,
            w!(""),
            WS_POPUP,
            0,
            0,
            1,
            1,
            None,
            None,
            Some(inst),
            None,
        )
        .unwrap();
        OVERLAY.store(hwnd.0 as isize, Ordering::Release);
        // Some games grab topmost when they gain focus; re-assert it once a second.
        SetTimer(Some(hwnd), 1, 1000, None);
        // Out-of-context WinEvent hook: Windows tells us when focus changes. No DLL, no polling.
        SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            None,
            Some(on_focus),
            0,
            0,
            WINEVENT_OUTOFCONTEXT,
        );
        let (show_msg, quit_msg) = (
            RegisterWindowMessageW(MSG_SHOW),
            RegisterWindowMessageW(MSG_QUIT),
        );

        let mut st = State {
            own_exe: own_exe(),
            ..Default::default()
        };
        apply(hwnd, &settings, &mut st);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            match msg.message {
                WM_HOTKEY if msg.wParam.0 as i32 == HK_TOGGLE => {
                    VISIBLE.fetch_xor(true, Ordering::Relaxed);
                    apply(hwnd, &settings, &mut st);
                }
                WM_HOTKEY if msg.wParam.0 as i32 == HK_MENU => toggle_settings(false),
                WM_REFRESH => apply(hwnd, &settings, &mut st),
                WM_TIMER => {
                    let _ = SetWindowPos(
                        hwnd,
                        Some(HWND_TOPMOST),
                        0,
                        0,
                        0,
                        0,
                        SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                    );
                }
                m if m == show_msg => toggle_settings(true),
                m if m == quit_msg => {
                    settings.lock().unwrap().save();
                    std::process::exit(0);
                }
                _ => {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
        }
    }
}

unsafe extern "system" fn on_focus(
    _: HWINEVENTHOOK,
    _: u32,
    _: HWND,
    _: i32,
    _: i32,
    _: u32,
    _: u32,
) {
    refresh();
}

#[derive(Default)]
struct State {
    own_exe: String,
    keys: [Hotkey; 2],
    drawn: Option<Crosshair>, // skip re-rendering when nothing changed
    size: (i32, i32),
    at: POINT,
}

unsafe fn apply(hwnd: HWND, settings: &Mutex<Settings>, st: &mut State) {
    let fg = unsafe { GetForegroundWindow() };
    let exe = unsafe { exe_of(fg) };
    let ours = exe == st.own_exe;
    let visible = VISIBLE.load(Ordering::Relaxed);
    let (c, keys, show, in_game, windowed) = {
        let s = settings.lock().unwrap();
        let game = s.games.iter().find(|g| g.exe == exe).filter(|_| !ours);
        // While our own window is focused, preview the crosshair being edited.
        let c = game
            .and_then(|g| s.presets.get(&g.preset))
            .unwrap_or(&s.crosshair)
            .clone();
        let show = visible && (!s.only_games || game.is_some() || ours);
        *STATUS.lock().unwrap() = match game {
            _ if !visible => Status::Hidden,
            Some(g) => Status::InGame(g.exe.clone()),
            None if s.only_games => Status::Waiting,
            None => Status::Everywhere,
        };
        let windowed = game.is_some_and(|g| g.windowed);
        (
            c,
            [s.toggle_key, s.menu_key],
            show,
            game.is_some(),
            windowed,
        )
    };
    IN_GAME.store(in_game, Ordering::Relaxed);

    for (i, hk) in keys.into_iter().enumerate() {
        if st.keys[i] != hk {
            let id = i as i32 + 1;
            unsafe {
                let _ = UnregisterHotKey(Some(hwnd), id);
                let ok = hk.vk == 0
                    || RegisterHotKey(
                        Some(hwnd),
                        id,
                        HOT_KEY_MODIFIERS(hk.mods) | MOD_NOREPEAT,
                        hk.vk,
                    )
                    .is_ok();
                KEY_OK[i].store(ok, Ordering::Relaxed);
            }
            st.keys[i] = hk;
        }
    }

    unsafe {
        let centre = target(fg, in_game, windowed);
        if st.drawn.as_ref() != Some(&c) {
            let img = render::render(&c);
            st.size = (img.w as i32, img.h as i32);
            st.at = top_left(centre, st.size, &c);
            draw(hwnd, &img, st.at);
            st.drawn = Some(c);
        } else if top_left(centre, st.size, &c) != st.at {
            // Same image, different spot (a game on another monitor): just move the window.
            st.at = top_left(centre, st.size, &c);
            let flags = SWP_NOSIZE | SWP_NOACTIVATE;
            let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), st.at.x, st.at.y, 0, 0, flags);
        }
        let _ = ShowWindow(hwnd, if show { SW_SHOWNOACTIVATE } else { SW_HIDE });
    }
}

fn top_left(centre: POINT, (w, h): (i32, i32), c: &Crosshair) -> POINT {
    POINT {
        x: centre.x - w / 2 + c.offset_x,
        y: centre.y - h / 2 + c.offset_y,
    }
}

/// Screen point the crosshair is centred on: the middle of the monitor the focused game is on,
/// or of the main monitor when no game is focused. Only games marked as windowed centre on their
/// window, since that's where their own crosshair is. (Centring on whatever window was focused
/// made it drift: a maximized window stops at the taskbar, so its middle is above the screen's.)
unsafe fn target(fg: HWND, in_game: bool, windowed: bool) -> POINT {
    unsafe {
        let mut r = RECT::default();
        if windowed && GetClientRect(fg, &mut r).is_ok() && r.right > 0 && r.bottom > 0 {
            let mut p = POINT {
                x: r.right / 2,
                y: r.bottom / 2,
            };
            if ClientToScreen(fg, &mut p).as_bool() {
                return p;
            }
        }
        let mut mi = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let monitor = if in_game {
            MonitorFromWindow(fg, MONITOR_DEFAULTTOPRIMARY)
        } else {
            // (0, 0) is always on the main monitor.
            MonitorFromPoint(POINT::default(), MONITOR_DEFAULTTOPRIMARY)
        };
        if GetMonitorInfoW(monitor, &mut mi).as_bool() {
            let m = mi.rcMonitor;
            return POINT {
                x: (m.left + m.right) / 2,
                y: (m.top + m.bottom) / 2,
            };
        }
        POINT {
            x: GetSystemMetrics(SM_CXSCREEN) / 2,
            y: GetSystemMetrics(SM_CYSCREEN) / 2,
        }
    }
}

unsafe fn draw(hwnd: HWND, img: &Image, pos: POINT) {
    unsafe {
        let screen = GetDC(None);
        let mem = CreateCompatibleDC(Some(screen));
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: img.w as i32,
                biHeight: -(img.h as i32), // top-down
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        if let Ok(bmp) = CreateDIBSection(Some(mem), &bmi, DIB_RGB_COLORS, &mut bits, None, 0) {
            std::ptr::copy_nonoverlapping(img.px.as_ptr(), bits as *mut u32, img.px.len());
            let old = SelectObject(mem, bmp.into());
            let size = SIZE {
                cx: img.w as i32,
                cy: img.h as i32,
            };
            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };
            let _ = UpdateLayeredWindow(
                hwnd,
                Some(screen),
                Some(&pos),
                Some(&size),
                Some(mem),
                Some(&POINT::default()),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );
            SelectObject(mem, old);
            let _ = DeleteObject(bmp.into());
        }
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);
    }
}

fn own_exe() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|p| Some(p.file_name()?.to_string_lossy().to_lowercase()))
        .unwrap_or_default()
}

/// Lowercase exe name of the process that owns `hwnd`, or "" if unknown.
unsafe fn exe_of(hwnd: HWND) -> String {
    unsafe {
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return String::new();
        }
        if let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            let mut buf = [0u16; 1024];
            let mut len = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(
                h,
                PROCESS_NAME_WIN32,
                windows::core::PWSTR(buf.as_mut_ptr()),
                &mut len,
            );
            let _ = CloseHandle(h);
            if ok.is_ok() {
                let path = String::from_utf16_lossy(&buf[..len as usize]);
                return path.rsplit('\\').next().unwrap_or("").to_lowercase();
            }
        }
        // Some anti-cheats refuse even a limited handle. The process list still has the name.
        exe_by_pid(pid)
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

/// Exe names of apps with a visible top-level window, for the "add game" picker.
pub fn running_apps() -> Vec<String> {
    unsafe extern "system" fn each(h: HWND, out: LPARAM) -> BOOL {
        unsafe {
            let out = &mut *(out.0 as *mut Vec<String>);
            if IsWindowVisible(h).as_bool()
                && GetWindowTextLengthW(h) > 0
                && GetWindow(h, GW_OWNER).is_err()
            {
                let exe = exe_of(h);
                if !exe.is_empty() && !out.contains(&exe) {
                    out.push(exe);
                }
            }
            true.into()
        }
    }
    let mut out = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM(&mut out as *mut _ as isize));
    }
    let skip = [
        own_exe(),
        "explorer.exe".into(),
        "applicationframehost.exe".into(),
        "textinputhost.exe".into(),
    ];
    out.retain(|e| !skip.contains(e));
    out.sort();
    out
}

/// Our settings window: the top-level window in this process titled TITLE.
fn settings_window() -> Option<HWND> {
    unsafe extern "system" fn each(h: HWND, out: LPARAM) -> BOOL {
        unsafe {
            let mut pid = 0;
            GetWindowThreadProcessId(h, Some(&mut pid));
            let mut buf = [0u16; 64];
            let n = GetWindowTextW(h, &mut buf) as usize;
            if pid == GetCurrentProcessId() && String::from_utf16_lossy(&buf[..n]) == TITLE {
                *(out.0 as *mut HWND) = h;
                return false.into(); // found it, stop
            }
            true.into()
        }
    }
    let mut found = HWND::default();
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM(&mut found as *mut _ as isize));
    }
    (!found.0.is_null()).then_some(found)
}

pub fn settings_visible() -> bool {
    settings_window().is_some_and(|h| unsafe { IsWindowVisible(h).as_bool() })
}

fn toggle_settings(force_show: bool) {
    let Some(h) = settings_window() else { return };
    unsafe {
        if !force_show && IsWindowVisible(h).as_bool() && GetForegroundWindow() == h {
            let _ = ShowWindow(h, SW_HIDE);
        } else {
            let _ = ShowWindow(h, SW_SHOW);
            let _ = ShowWindow(h, SW_RESTORE);
            let _ = SetForegroundWindow(h);
        }
    }
}

/// Poll the physical keyboard for a pressed key (used for rebinding).
/// Returns Some(None) on Escape, which cancels.
pub fn capture_key() -> Option<Option<Hotkey>> {
    let down = |vk: i32| unsafe { GetAsyncKeyState(vk) as u16 & 0x8000 != 0 };
    for vk in 0x08..=0xFE {
        // Modifiers on their own aren't a binding; they're read below.
        if matches!(vk, 0x10..=0x12 | 0x5B | 0x5C | 0xA0..=0xA5) || !down(vk) {
            continue;
        }
        if vk == 0x1B {
            return Some(None);
        }
        let mods = (down(0x12) as u32) | (down(0x11) as u32) << 1 | (down(0x10) as u32) << 2;
        return Some(Some(Hotkey {
            vk: vk as u32,
            mods,
        }));
    }
    None
}

pub fn key_name(hk: Hotkey) -> String {
    if hk.vk == 0 {
        return "Not set".into();
    }
    let mut s = String::new();
    for (bit, name) in [(2, "Ctrl+"), (4, "Shift+"), (1, "Alt+")] {
        if hk.mods & bit != 0 {
            s += name;
        }
    }
    unsafe {
        let sc = MapVirtualKeyW(hk.vk, MAPVK_VK_TO_VSC_EX);
        let lparam = ((sc & 0xFF) << 16) | if sc & 0xE000 != 0 { 1 << 24 } else { 0 };
        let mut buf = [0u16; 32];
        let n = GetKeyNameTextW(lparam as i32, &mut buf);
        if n > 0 {
            s += &String::from_utf16_lossy(&buf[..n as usize]);
        } else {
            s += &format!("Key {:#04X}", hk.vk);
        }
    }
    s
}

unsafe extern "system" fn wndproc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    unsafe { DefWindowProcW(h, m, w, l) }
}
