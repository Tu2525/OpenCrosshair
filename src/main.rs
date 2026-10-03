#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // no console in release

mod apps;
mod config;
mod install;
mod overlay;
mod picture;
mod render;
mod ui;
mod update;

use std::sync::{Arc, Mutex};

fn main() -> eframe::Result {
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
    if install::is_setup_exe() {
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
