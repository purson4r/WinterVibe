#![windows_subsystem = "windows"]

mod app;
mod desktop;
mod dialog;
mod donation;
mod drift_settings;
mod halloween;
mod light;
mod log;
mod locale;
mod prefs;
mod render;
mod screen;
mod snow;
mod tracker;
mod update;
mod welcome;

use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext};
use windows::core::w;

fn main() {
    // What "Uninstall" in Settings runs; it removes everything, even with WinterVibe still running.
    if std::env::args().skip(1).any(|argument| argument == "--uninstall") {
        wintervibe::install::uninstall();
        return;
    }
    unsafe {
        let _instance = CreateMutexW(None, true, w!("Local\\WinterVibe"));
        if GetLastError() == ERROR_ALREADY_EXISTS {
            return;
        }
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    log::start();
    log!("WinterVibe {}", env!("CARGO_PKG_VERSION"));
    if let Err(error) = app::run() {
        log!("failed: {error}");
    }
}
