//! The overlay: a click-through, always-on-top layered window. Plain Win32, nothing is injected
//! into games. It only redraws when something changes, so it costs nothing while you play.

use crate::apps;
use crate::config::{Aim, Crosshair, Hotkey, Settings};
use crate::render::{self, Image};
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::*;
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook};
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFY_ICON_MESSAGE, NOTIFYICONDATAW,
    QUNS_RUNNING_D3D_FULL_SCREEN, SHQueryUserNotificationState, Shell_NotifyIconW,
};
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
    /// In a game that Windows says is running Direct3D fullscreen. Usually that still works
    /// (fullscreen optimizations), but true exclusive fullscreen can't be drawn over.
    Fullscreen(String),
}

static OVERLAY: AtomicIsize = AtomicIsize::new(0);
const WM_REFRESH: u32 = WM_APP + 1;
// The tray icon gets its own hidden window: the overlay is click-through and can't take focus,
// which a tray menu needs in order to close when you click elsewhere.
static TRAY: AtomicIsize = AtomicIsize::new(0);
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);
const WM_TRAY: u32 = WM_APP + 2;
const AIM_TIMER: usize = 2; // the topmost timer is 1
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
        // We were just launched by you, so we're allowed to bring a window to the front. Pass
        // that on, or the running copy's window would open behind whatever you're doing.
        let _ = AllowSetForegroundWindow(ASFW_ANY);
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
        let tray_class = w!("OpenCrosshairTray");
        RegisterClassW(&WNDCLASSW {
            lpfnWndProc: Some(tray_proc),
            hInstance: inst,
            lpszClassName: tray_class,
            ..Default::default()
        });
        let style = WINDOW_EX_STYLE::default();
        let tray = CreateWindowExW(
            style,
            tray_class,
            w!(""),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(inst),
            None,
        );
        if let Ok(tray) = tray {
            TRAY.store(tray.0 as isize, Ordering::Release);
            TASKBAR_CREATED.store(
                RegisterWindowMessageW(w!("TaskbarCreated")),
                Ordering::Relaxed,
            );
            tray_icon(NIM_ADD);
        }
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
            own_exe: apps::own_exe(),
            ..Default::default()
        };
        apply(hwnd, &settings, &mut st);
        let mut hidden_for = 0; // seconds the settings window has been hidden

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            match msg.message {
                WM_HOTKEY if msg.wParam.0 as i32 == HK_TOGGLE => {
                    VISIBLE.fetch_xor(true, Ordering::Relaxed);
                    apply(hwnd, &settings, &mut st);
                }
                WM_HOTKEY if msg.wParam.0 as i32 == HK_MENU => toggle_settings(false),
                WM_REFRESH => apply(hwnd, &settings, &mut st),
                WM_TIMER if msg.wParam.0 == AIM_TIMER => {
                    // Aiming is read by checking the button about 64 times a second, and only
                    // while it matters: no input hooks, and nothing at all outside games.
                    let down = GetAsyncKeyState(st.aim_vk) as u16 & 0x8000 != 0;
                    let aiming = next_aim(st.aim_toggle, down, st.aim_down, st.aiming);
                    st.aim_down = down;
                    if aiming != st.aiming {
                        st.aiming = aiming;
                        apply(hwnd, &settings, &mut st);
                    }
                }
                WM_TIMER => {
                    // Games can switch to fullscreen after they've taken focus.
                    if IN_GAME.load(Ordering::Relaxed) && d3d_fullscreen() != st.fullscreen {
                        apply(hwnd, &settings, &mut st);
                    }
                    let _ = SetWindowPos(
                        hwnd,
                        Some(HWND_TOPMOST),
                        0,
                        0,
                        0,
                        0,
                        SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                    );
                    // Once the settings window has stayed hidden a few seconds (it starts that way
                    // at login), give its memory back. By then it has finished whatever it was
                    // doing, like building itself at startup.
                    let hidden = settings_window().is_some_and(|h| !IsWindowVisible(h).as_bool());
                    hidden_for = if hidden { hidden_for + 1 } else { 0 };
                    if hidden_for == 3 {
                        trim_memory();
                    }
                }
                m if m == show_msg => toggle_settings(true),
                m if m == quit_msg => {
                    settings.lock().unwrap().save();
                    remove_tray();
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
    last_exe: String, // which app was in front last time apply() ran
    aiming: bool,
    aim_down: bool, // button state last time, for toggle mode
    aim_vk: i32,
    aim_toggle: bool,
    polling: bool,
    fullscreen: bool,
}

/// Hold mode: aiming while the button is down. Toggle mode: each press flips it.
fn next_aim(toggle: bool, down: bool, was_down: bool, aiming: bool) -> bool {
    if toggle {
        aiming ^ (down && !was_down)
    } else {
        down
    }
}

/// Windows reports a Direct3D game running fullscreen.
fn d3d_fullscreen() -> bool {
    unsafe { SHQueryUserNotificationState().is_ok_and(|s| s == QUNS_RUNNING_D3D_FULL_SCREEN) }
}

unsafe fn apply(hwnd: HWND, settings: &Mutex<Settings>, st: &mut State) {
    let fg = unsafe { GetForegroundWindow() };
    let exe = apps::of_window(fg).exe;
    // The settings window previews the crosshair, but only while you can actually see it.
    let ours = exe == st.own_exe && settings_visible();
    let visible = VISIBLE.load(Ordering::Relaxed);
    let (c, keys, show, in_game, windowed, aim_on) = {
        let s = settings.lock().unwrap();
        let game = s.games.iter().find(|g| g.exe == exe).filter(|_| !ours);
        // While our own window is focused, preview the crosshair being edited.
        let mut c = game
            .and_then(|g| s.presets.get(&g.preset))
            .unwrap_or(&s.crosshair)
            .clone();
        let mut show = visible && (!s.only_games || game.is_some() || ours);
        st.fullscreen = game.is_some() && d3d_fullscreen();
        *STATUS.lock().unwrap() = match game {
            _ if !visible => Status::Hidden,
            Some(g) if st.fullscreen => Status::Fullscreen(g.exe.clone()),
            Some(g) => Status::InGame(g.exe.clone()),
            None if s.only_games => Status::Waiting,
            None => Status::Everywhere,
        };
        // The game's own aiming rule if it has one, otherwise the default.
        let rule = s.aim_rule_for(game);
        // Switching apps ends an aim that was in progress (a toggled one would carry over).
        if st.last_exe != exe {
            st.last_exe = exe.clone();
            st.aiming = false;
            st.aim_down = false;
        }
        // Aiming only counts while the crosshair is up somewhere it should react to it.
        let aim_on = rule.action != Aim::Keep && show && !ours;
        if !aim_on {
            st.aiming = false;
        }
        (st.aim_vk, st.aim_toggle) = (rule.button.vk(), rule.toggle);
        if st.aiming {
            match &rule.action {
                Aim::Hide => show = false,
                Aim::Preset(name) => {
                    if let Some(p) = s.presets.get(name) {
                        c = p.clone();
                    }
                }
                Aim::Keep => {}
            }
        }
        let windowed = game.is_some_and(|g| g.windowed);
        (
            c,
            [s.toggle_key, s.menu_key],
            show,
            game.is_some(),
            windowed,
            aim_on,
        )
    };
    IN_GAME.store(in_game, Ordering::Relaxed);
    if aim_on != st.polling {
        unsafe {
            if aim_on {
                SetTimer(Some(hwnd), AIM_TIMER, 15, None);
            } else {
                let _ = KillTimer(Some(hwnd), AIM_TIMER);
            }
        }
        st.polling = aim_on;
    }

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
        if show {
            // A game that just took focus may have put itself on top; get back above it now
            // rather than on the next tick of the topmost timer.
            let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE;
            let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, flags);
        }
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

/// Our settings window: the top-level window in this process titled TITLE.
pub fn settings_window() -> Option<HWND> {
    unsafe extern "system" fn each(h: HWND, out: LPARAM) -> BOOL {
        unsafe {
            let mut pid = 0;
            GetWindowThreadProcessId(h, Some(&mut pid));
            // InternalGetWindowText, unlike GetWindowText, never sends the window a message.
            // The window's thread may be busy, and waiting on it from here could hang us.
            let mut buf = [0u16; 64];
            let n = InternalGetWindowText(h, &mut buf) as usize;
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

/// Hide the settings window. It stays alive so it comes back instantly; its memory is handed
/// back to Windows until then.
pub fn hide_settings() {
    if let Some(h) = settings_window() {
        unsafe {
            let _ = ShowWindow(h, SW_HIDE);
        }
    }
    // Hiding a window doesn't always move focus anywhere, so no focus event would come to tell
    // the overlay the preview is over. Check now instead of on your next click.
    refresh();
    trim_memory();
}

/// Give the settings window an owner (our hidden tray window). Windows leaves owned windows out
/// of Alt+Tab and the taskbar; the tray icon and the hotkey are how you get to it.
pub fn adopt_settings_window() {
    let tray = TRAY.load(Ordering::Acquire);
    if let Some(h) = settings_window()
        && tray != 0
    {
        unsafe {
            SetWindowLongPtrW(h, GWLP_HWNDPARENT, tray);
        }
    }
}

/// Let Windows take back the RAM we aren't using right now. While the settings window is hidden
/// most of the app (the UI, its graphics context, the fonts) sits untouched, so its pages can go.
/// They're paged back in if the window is opened again.
pub fn trim_memory() {
    unsafe {
        let _ = SetProcessWorkingSetSize(GetCurrentProcess(), usize::MAX, usize::MAX);
    }
}

/// F9, the tray icon and launching the app again all end up here.
fn toggle_settings(force_show: bool) {
    let Some(h) = settings_window() else { return };
    unsafe {
        if !force_show && IsWindowVisible(h).as_bool() && GetForegroundWindow() == h {
            hide_settings();
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

fn tray_icon(action: NOTIFY_ICON_MESSAGE) {
    let h = TRAY.load(Ordering::Acquire);
    if h == 0 {
        return;
    }
    unsafe {
        let mut data = NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: HWND(h as _),
            uID: 1,
            uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
            uCallbackMessage: WM_TRAY,
            ..Default::default()
        };
        if action != NIM_DELETE {
            // Resource 1 is the app icon that build.rs embeds.
            let inst = GetModuleHandleW(None).ok().map(HINSTANCE::from);
            let (cx, cy) = (GetSystemMetrics(SM_CXSMICON), GetSystemMetrics(SM_CYSMICON));
            if let Ok(icon) = LoadImageW(inst, PCWSTR(1 as _), IMAGE_ICON, cx, cy, LR_SHARED) {
                data.hIcon = HICON(icon.0);
            }
            let tip: Vec<u16> = TITLE.encode_utf16().collect();
            data.szTip[..tip.len()].copy_from_slice(&tip);
        }
        let _ = Shell_NotifyIconW(action, &data);
    }
}

/// Take the tray icon down before exiting, or it lingers until the mouse passes over it.
pub fn remove_tray() {
    tray_icon(NIM_DELETE);
}

unsafe extern "system" fn tray_proc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    match m {
        WM_TRAY => match l.0 as u32 {
            WM_LBUTTONUP => toggle_settings(true),
            WM_RBUTTONUP => unsafe { tray_menu(h) },
            _ => {}
        },
        // Explorer restarted and took the tray with it.
        m if m != 0 && m == TASKBAR_CREATED.load(Ordering::Relaxed) => tray_icon(NIM_ADD),
        _ => return unsafe { DefWindowProcW(h, m, w, l) },
    }
    LRESULT(0)
}

unsafe fn tray_menu(h: HWND) {
    const OPEN: usize = 1;
    const TOGGLE: usize = 2;
    const QUIT: usize = 3;
    unsafe {
        let Ok(menu) = CreatePopupMenu() else { return };
        let shown = if VISIBLE.load(Ordering::Relaxed) {
            MF_CHECKED
        } else {
            MF_UNCHECKED
        };
        let _ = AppendMenuW(menu, MF_STRING, OPEN, w!("Open settings"));
        let _ = AppendMenuW(menu, MF_STRING | shown, TOGGLE, w!("Show crosshair"));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(menu, MF_STRING, QUIT, w!("Quit"));
        let mut at = POINT::default();
        let _ = GetCursorPos(&mut at);
        // The menu only closes on an outside click if its window is in front.
        let _ = SetForegroundWindow(h);
        let flags = TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY;
        let picked = TrackPopupMenu(menu, flags, at.x, at.y, None, h, None);
        let _ = DestroyMenu(menu);
        match picked.0 as usize {
            OPEN => toggle_settings(true),
            TOGGLE => {
                VISIBLE.fetch_xor(true, Ordering::Relaxed);
                refresh();
            }
            QUIT => {
                // Same path as the installer's quit request: save, tidy up, exit.
                let overlay = HWND(OVERLAY.load(Ordering::Acquire) as _);
                let quit = RegisterWindowMessageW(MSG_QUIT);
                let _ = PostMessageW(Some(overlay), quit, WPARAM(0), LPARAM(0));
            }
            _ => {}
        }
    }
}

unsafe extern "system" fn wndproc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    unsafe { DefWindowProcW(h, m, w, l) }
}

#[cfg(test)]
mod tests {
    use super::next_aim;

    #[test]
    fn aim_hold_and_toggle() {
        // Hold: follows the button.
        assert!(next_aim(false, true, false, false));
        assert!(!next_aim(false, false, true, true));
        // Toggle: a press flips it, holding or releasing doesn't.
        assert!(next_aim(true, true, false, false), "press starts aiming");
        assert!(next_aim(true, true, true, true), "still held, still aiming");
        assert!(next_aim(true, false, true, true), "release keeps aiming");
        assert!(!next_aim(true, true, false, true), "second press stops");
    }
}
