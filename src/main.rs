#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // no console in release

mod apps;
mod config;
mod install;
mod overlay;
mod picture;
mod render;
mod share;
mod ui;
mod update;

use std::io::Write;
use std::sync::{Arc, Mutex};

fn main() -> eframe::Result {
    // Release builds have no console and abort on panic, so a crash would vanish without this.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log_panic(info);
        default_hook(info);
    }));
    // Physical pixels, so the crosshair is crisp and centred on scaled displays.
    unsafe {
        use windows::Win32::UI::HiDpi::*;
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    let flag = |f: &str| std::env::args().any(|a| a == f);
    // --silent skips the prompts, for scripted installs (winget and the like).
    if flag("--uninstall") {
        install::uninstall(flag("--silent"));
        return Ok(());
    }
    // A silent run always means install, since winget may rename the download.
    if install::is_setup_exe() || flag("--silent") {
        install::install(flag("--silent"));
        return Ok(());
    }

    // One copy at a time: launching it again just opens the running one's settings.
    // Right after an update the new copy waits for the old one to finish exiting.
    let Some(_instance) = install::Instance::acquire(if flag("--updated") { 15_000 } else { 0 })
    else {
        overlay::signal_running(false);
        return Ok(());
    };
    if !cfg!(debug_assertions) {
        install::refresh_registration();
        update::cleanup();
    }

    let settings = Arc::new(Mutex::new(config::Settings::load()));
    let s = settings.clone();
    std::thread::spawn(move || overlay::run(s));
    let updates = update::start(settings.clone());
    // --startup is how Windows launches us at login: overlay and tray icon, settings hidden.
    ui::run(settings, updates, flag("--startup"))
}

/// Append a panic to the crash log. Nothing in here may panic, so every error is ignored.
fn log_panic(info: &std::panic::PanicHookInfo<'_>) {
    let payload = info.payload();
    let message = payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("unknown");
    let location = info
        .location()
        .map(|l| format!("{}:{}", l.file(), l.line()));
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let entry = crash_entry(
        secs,
        env!("CARGO_PKG_VERSION"),
        std::thread::current().name().unwrap_or("unnamed"),
        message,
        location,
    );

    let path = config::crash_log();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    // Keep the file small: past 64 KB, start it over rather than growing it.
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > 64 << 10) {
        let _ = std::fs::write(&path, entry);
    } else if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = f.write_all(entry.as_bytes());
    }
}

fn crash_entry(
    secs: u64,
    version: &str,
    thread: &str,
    message: &str,
    location: Option<String>,
) -> String {
    format!(
        "[{secs}] OpenCrosshair {version}, thread '{thread}': {message}\n  at {}\n\n",
        location.as_deref().unwrap_or("unknown location")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_crash_entry_names_the_version_thread_message_and_place() {
        let e = crash_entry(0, "0.5.0", "main", "boom", Some("src/render.rs:42".into()));
        for part in ["0.5.0", "main", "boom", "src/render.rs:42"] {
            assert!(e.contains(part), "{part} missing from {e:?}");
        }
        assert!(e.ends_with('\n'));
    }
}
