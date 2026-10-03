//! Everything that touches Windows outside our own windows: one-instance lock, start with
//! Windows, and the installer/uninstaller.
//!
//! There's no separate installer program. OpenCrosshair-Setup.exe is the same binary as
//! OpenCrosshair.exe under another name: when its file name contains "setup" it copies itself
//! into %LOCALAPPDATA%\Programs\OpenCrosshair, adds a Start menu entry and registers with
//! Apps & features. Everything is per-user, so it never needs admin rights.

use crate::overlay;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use windows::Win32::Foundation::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::Registry::*;
use windows::Win32::System::Threading::*;
use windows::Win32::UI::Shell::{IShellLinkW, ShellExecuteW, ShellLink};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{HSTRING, Interface, PCWSTR, w};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const RUN_KEY: PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\Run");
const UNINSTALL_KEY: PCWSTR =
    w!(r"Software\Microsoft\Windows\CurrentVersion\Uninstall\OpenCrosshair");
const APP: PCWSTR = w!("OpenCrosshair");

/// Holds the one-copy-per-user lock for as long as it's alive.
pub struct Instance(HANDLE);

impl Instance {
    /// Waits up to `wait_ms` for a running copy to exit. None if one is still running.
    pub fn acquire(wait_ms: u32) -> Option<Self> {
        unsafe {
            let h = CreateMutexW(None, false, w!(r"Local\OpenCrosshair")).ok()?;
            // ABANDONED just means the previous owner exited without unlocking; it's ours now.
            match WaitForSingleObject(h, wait_ms) {
                WAIT_OBJECT_0 | WAIT_ABANDONED => Some(Self(h)),
                _ => {
                    let _ = CloseHandle(h);
                    None
                }
            }
        }
    }
}

impl Drop for Instance {
    fn drop(&mut self) {
        unsafe {
            let _ = ReleaseMutex(self.0);
            let _ = CloseHandle(self.0);
        }
    }
}

/// Open a web page in the default browser.
pub fn open_url(url: &str) {
    unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            &HSTRING::from(url),
            None,
            None,
            SW_SHOWNORMAL,
        );
    }
}

pub fn autostart() -> bool {
    unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            RUN_KEY,
            APP,
            RRF_RT_REG_SZ,
            None,
            None,
            None,
        )
        .is_ok()
    }
}

pub fn set_autostart(on: bool) {
    match std::env::current_exe() {
        Ok(exe) if on => set_run_key(&exe),
        _ => unsafe {
            let _ = RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN_KEY, APP);
        },
    }
}

fn set_run_key(exe: &Path) {
    set_string(RUN_KEY, APP, &format!("\"{}\" --startup", exe.display()));
}

fn set_string(key: PCWSTR, name: PCWSTR, value: &str) {
    let v: Vec<u16> = value.encode_utf16().chain([0]).collect();
    unsafe {
        let _ = RegSetKeyValueW(
            HKEY_CURRENT_USER,
            key,
            name,
            REG_SZ.0,
            Some(v.as_ptr() as _),
            (v.len() * 2) as u32,
        );
    }
}

fn set_dword(key: PCWSTR, name: PCWSTR, value: u32) {
    unsafe {
        let _ = RegSetKeyValueW(
            HKEY_CURRENT_USER,
            key,
            name,
            REG_DWORD.0,
            Some(&value as *const u32 as _),
            4,
        );
    }
}

fn install_dir() -> Option<PathBuf> {
    Some(
        PathBuf::from(std::env::var_os("LOCALAPPDATA")?)
            .join("Programs")
            .join("OpenCrosshair"),
    )
}

fn start_menu_link() -> Option<PathBuf> {
    let appdata = PathBuf::from(std::env::var_os("APPDATA")?);
    Some(appdata.join(r"Microsoft\Windows\Start Menu\Programs\OpenCrosshair.lnk"))
}

fn running_installed() -> bool {
    let exe = std::env::current_exe().ok();
    exe.as_deref()
        .and_then(Path::parent)
        .is_some_and(|d| Some(d) == install_dir().as_deref())
}

pub fn is_setup_exe() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|p| Some(p.file_stem()?.to_string_lossy().to_lowercase()))
        .is_some_and(|name| name.contains("setup"))
}

/// Keep the registry in step after an update: the version shown in Apps & features, and the
/// startup entry's path.
pub fn refresh_registration() {
    if !running_installed() {
        return;
    }
    set_string(UNINSTALL_KEY, w!("DisplayVersion"), VERSION);
    if autostart() {
        set_autostart(true);
    }
}

fn ask(text: &str) -> bool {
    unsafe { MessageBoxW(None, &HSTRING::from(text), APP, MB_YESNO | MB_ICONQUESTION) == IDYES }
}

fn tell(text: &str, error: bool) {
    let icon = if error {
        MB_ICONERROR
    } else {
        MB_ICONINFORMATION
    };
    unsafe {
        MessageBoxW(None, &HSTRING::from(text), APP, MB_OK | icon);
    }
}

pub fn install(silent: bool) {
    let tell = |text: &str, error: bool| {
        if !silent {
            tell(text, error)
        }
    };
    let Some(dir) = install_dir() else {
        return tell("Couldn't find your AppData folder.", true);
    };
    let target = dir.join("OpenCrosshair.exe");
    let prompt = format!(
        "Install OpenCrosshair {VERSION}?\n\nIt installs just for your account, so no admin rights are needed:\n{}",
        dir.display()
    );
    if !silent && !ask(&prompt) {
        return;
    }

    // An older copy may be running; ask it to quit so its exe can be replaced.
    overlay::signal_running(true);
    let Some(lock) = Instance::acquire(10_000) else {
        return tell(
            "OpenCrosshair is still running. Close it and run the installer again.",
            true,
        );
    };
    let copied = std::fs::create_dir_all(&dir)
        .and_then(|_| std::fs::copy(std::env::current_exe()?, &target));
    if let Err(e) = copied {
        return tell(
            &format!("Couldn't copy OpenCrosshair to {}:\n{e}", dir.display()),
            true,
        );
    }
    if let Some(link) = start_menu_link() {
        let _ = create_shortcut(&link, &target);
    }
    register_uninstall(&target);
    if autostart() {
        set_run_key(&target);
    }
    drop(lock);
    let _ = Command::new(&target).spawn();
}

fn create_shortcut(link: &Path, target: &Path) -> windows::core::Result<()> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let shell_link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        shell_link.SetPath(&HSTRING::from(target))?;
        shell_link.SetDescription(w!("Crosshair overlay for games"))?;
        shell_link
            .cast::<IPersistFile>()?
            .Save(&HSTRING::from(link), true)
    }
}

fn register_uninstall(exe: &Path) {
    let exe_str = exe.display().to_string();
    let k = UNINSTALL_KEY;
    set_string(k, w!("DisplayName"), "OpenCrosshair");
    set_string(k, w!("DisplayVersion"), VERSION);
    set_string(k, w!("Publisher"), "OpenCrosshair");
    set_string(k, w!("DisplayIcon"), &format!("{exe_str},0"));
    set_string(
        k,
        w!("UninstallString"),
        &format!("\"{exe_str}\" --uninstall"),
    );
    set_string(
        k,
        w!("InstallLocation"),
        &exe.parent().unwrap_or(exe).display().to_string(),
    );
    set_string(k, w!("URLInfoAbout"), env!("CARGO_PKG_REPOSITORY"));
    set_dword(k, w!("NoModify"), 1);
    set_dword(k, w!("NoRepair"), 1);
    let kb = std::fs::metadata(exe).map(|m| m.len() / 1024).unwrap_or(0);
    set_dword(k, w!("EstimatedSize"), kb as u32);
}

pub fn uninstall(silent: bool) {
    if !silent
        && !ask(
            "Uninstall OpenCrosshair?\n\nYour crosshairs and settings are kept in %APPDATA%\\OpenCrosshair in case you come back.",
        )
    {
        return;
    }
    overlay::signal_running(true);
    let _lock = Instance::acquire(10_000);

    set_autostart(false);
    if let Some(link) = start_menu_link() {
        let _ = std::fs::remove_file(link);
    }
    unsafe {
        let _ = RegDeleteTreeW(HKEY_CURRENT_USER, UNINSTALL_KEY);
    }
    if !silent {
        tell("OpenCrosshair has been removed.", false);
    }
    // A running exe can't delete itself, so leave the folder to cmd once we've exited.
    // Only ever our own install folder, never wherever a portable copy happens to live.
    if running_installed()
        && let Some(dir) = install_dir()
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let _ = Command::new("cmd")
            .raw_arg(format!(
                "/C \"ping 127.0.0.1 -n 3 >nul & rmdir /s /q \"{}\"\"",
                dir.display()
            ))
            .creation_flags(CREATE_NO_WINDOW)
            .spawn();
    }
}
