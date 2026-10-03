//! Updates itself from the latest GitHub release.
//!
//! Windows won't let you overwrite a running exe, but it will let you rename one. So the new
//! build is written next to the old one, the two swap names, and the app restarts into the new
//! version. It never restarts while you're in a game; it waits until you leave it. The leftover
//! `.old` file is deleted on the next start.

use crate::config::Settings;
use crate::overlay;
use serde::Deserialize;
use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use windows::Win32::Networking::WinHttp::*;
use windows::core::{HSTRING, PCWSTR, w};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
const REPO: &str = "Tu2525/OpenCrosshair";
const ASSET: &str = "OpenCrosshair.exe";
const CHECK_EVERY: Duration = Duration::from_secs(6 * 60 * 60);
const RETRY_AFTER: Duration = Duration::from_secs(10 * 60); // e.g. no network yet at boot

#[derive(Clone, PartialEq)]
pub enum Status {
    Idle,
    DevBuild,
    Checking,
    UpToDate,
    Downloading(String),
    WaitingForGame(String),
    Failed(String),
}

pub static STATUS: Mutex<Status> = Mutex::new(Status::Idle);

fn set(s: Status) {
    *STATUS.lock().unwrap() = s;
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    size: u64,
}

/// Remove what the last update left behind. Right after an update the old process can still be
/// shutting down and holding its exe for a moment, so keep trying for a few seconds.
pub fn cleanup() {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    std::thread::spawn(move || {
        let gone = |p: PathBuf| match std::fs::remove_file(p) {
            Ok(()) => true,
            Err(e) => e.kind() == std::io::ErrorKind::NotFound,
        };
        for _ in 0..20 {
            // `&` rather than `&&`: always try both.
            if gone(exe.with_extension("exe.old")) & gone(exe.with_extension("exe.new")) {
                return;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    });
}

/// Starts the background checker. Sending on the returned channel checks right away
/// (the "Check now" button), even when automatic updates are off.
pub fn start(settings: Arc<Mutex<Settings>>) -> mpsc::Sender<()> {
    let (tx, rx) = mpsc::channel();
    if cfg!(debug_assertions) {
        set(Status::DevBuild); // don't replace the exe cargo just built
        return tx;
    }
    std::thread::spawn(move || {
        let mut manual = false;
        loop {
            let mut wait = CHECK_EVERY;
            if manual || settings.lock().unwrap().auto_update {
                set(Status::Checking);
                match check_and_install() {
                    Ok(Some((exe, version))) => restart(&exe, &version, &settings),
                    Ok(None) => set(Status::UpToDate),
                    Err(e) => {
                        set(Status::Failed(e));
                        wait = RETRY_AFTER;
                    }
                }
            }
            manual = match rx.recv_timeout(wait) {
                Ok(()) => true,
                Err(RecvTimeoutError::Timeout) => false,
                Err(RecvTimeoutError::Disconnected) => return,
            };
        }
    });
    tx
}

/// "v1.2.3" -> (1, 2, 3). Anything unparsable counts as 0, so it never looks newer.
fn parse(v: &str) -> (u32, u32, u32) {
    let mut n = v
        .trim_start_matches('v')
        .split(['.', '-'])
        .map(|p| p.parse().unwrap_or(0));
    (
        n.next().unwrap_or(0),
        n.next().unwrap_or(0),
        n.next().unwrap_or(0),
    )
}

/// Downloads and swaps in a newer release if there is one. Returns the exe path and version.
fn check_and_install() -> Result<Option<(PathBuf, String)>, String> {
    let (status, body) = get(&format!(
        "https://api.github.com/repos/{REPO}/releases/latest"
    ))?;
    if status == 404 {
        return Ok(None); // no releases published yet
    }
    if status != 200 {
        return Err(format!("GitHub returned HTTP {status}"));
    }
    let release: Release =
        serde_json::from_slice(&body).map_err(|e| format!("unexpected reply from GitHub: {e}"))?;
    if parse(&release.tag_name) <= parse(VERSION) {
        return Ok(None);
    }
    let asset = release
        .assets
        .iter()
        .find(|a| a.name == ASSET)
        .ok_or("the release has no OpenCrosshair.exe")?;

    set(Status::Downloading(release.tag_name.clone()));
    let (status, exe_bytes) = get(&asset.browser_download_url)?;
    // A cut-off download would leave an exe that doesn't start, so check it's all there.
    if status != 200 || exe_bytes.len() as u64 != asset.size || !exe_bytes.starts_with(b"MZ") {
        return Err("the download was incomplete".into());
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    swap(&exe, &exe_bytes).map_err(|e| format!("couldn't replace {}: {e}", exe.display()))?;
    Ok(Some((exe, release.tag_name)))
}

fn swap(exe: &Path, new_bytes: &[u8]) -> std::io::Result<()> {
    let (new, old) = (exe.with_extension("exe.new"), exe.with_extension("exe.old"));
    std::fs::write(&new, new_bytes)?;
    let _ = std::fs::remove_file(&old);
    std::fs::rename(exe, &old)?; // allowed while running
    if let Err(e) = std::fs::rename(&new, exe) {
        let _ = std::fs::rename(&old, exe); // put the working version back
        return Err(e);
    }
    Ok(())
}

fn restart(exe: &Path, version: &str, settings: &Mutex<Settings>) {
    while overlay::IN_GAME.load(std::sync::atomic::Ordering::Relaxed) {
        set(Status::WaitingForGame(version.into()));
        std::thread::sleep(Duration::from_secs(15));
    }
    settings.lock().unwrap().save();
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("--updated"); // the new copy waits for this one to exit
    if !overlay::settings_visible() {
        cmd.arg("--startup");
    }
    match cmd.spawn() {
        Ok(_) => std::process::exit(0),
        Err(e) => set(Status::Failed(format!(
            "updated, but couldn't restart: {e}"
        ))),
    }
}

/// Minimal HTTPS GET on WinHTTP, which ships with Windows: no TLS library to bundle.
/// Follows redirects (GitHub sends downloads to its CDN). Returns status and body.
fn get(url: &str) -> Result<(u32, Vec<u8>), String> {
    struct Handle(*mut c_void);
    impl Drop for Handle {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe {
                    let _ = WinHttpCloseHandle(self.0);
                }
            }
        }
    }
    let err = |what: &str| format!("{what}: {}", windows::core::Error::from_thread());
    let offline = |e: windows::core::Error| format!("couldn't reach GitHub ({})", e.message());

    let rest = url
        .strip_prefix("https://")
        .ok_or("only https URLs are allowed")?;
    let (host, path) = rest
        .split_once('/')
        .map_or((rest, "/".to_string()), |(h, p)| (h, format!("/{p}")));
    unsafe {
        let session = Handle(WinHttpOpen(
            &HSTRING::from(format!("OpenCrosshair/{VERSION}")), // GitHub's API requires a User-Agent
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            PCWSTR::null(),
            PCWSTR::null(),
            0,
        ));
        if session.0.is_null() {
            return Err(err("WinHttpOpen"));
        }
        let conn = Handle(WinHttpConnect(
            session.0,
            &HSTRING::from(host),
            INTERNET_DEFAULT_HTTPS_PORT,
            0,
        ));
        if conn.0.is_null() {
            return Err(err("WinHttpConnect"));
        }
        let req = Handle(WinHttpOpenRequest(
            conn.0,
            w!("GET"),
            &HSTRING::from(path),
            PCWSTR::null(),
            PCWSTR::null(),
            std::ptr::null_mut(),
            WINHTTP_FLAG_SECURE,
        ));
        if req.0.is_null() {
            return Err(err("WinHttpOpenRequest"));
        }
        WinHttpSendRequest(req.0, None, None, 0, 0, 0).map_err(offline)?;
        WinHttpReceiveResponse(req.0, std::ptr::null_mut()).map_err(offline)?;

        let mut status = 0u32;
        let mut len = 4u32;
        WinHttpQueryHeaders(
            req.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some(&mut status as *mut u32 as *mut c_void),
            &mut len,
            std::ptr::null_mut(),
        )
        .map_err(|e| e.message())?;

        let mut body = Vec::new();
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let mut n = 0u32;
            WinHttpReadData(
                req.0,
                buf.as_mut_ptr() as *mut c_void,
                buf.len() as u32,
                &mut n,
            )
            .map_err(offline)?;
            if n == 0 {
                break;
            }
            body.extend_from_slice(&buf[..n as usize]);
        }
        Ok((status, body))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn versions() {
        use super::parse;
        assert_eq!(parse("v1.2.3"), (1, 2, 3));
        assert!(parse("v0.10.0") > parse("0.9.9"));
        assert!(parse("v0.2.0-beta") > parse("0.1.9"));
        assert_eq!(parse("garbage"), (0, 0, 0));
    }
}
