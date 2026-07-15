//! DesktopDrawers — a tiny native Win32 utility for compact, persistent desktop
//! drawers of shortcuts. See `README.md` for an overview of the architecture.

pub mod app;
pub mod command;
pub mod error;
pub mod icon_cache;
pub mod ipc;
pub mod model;
pub mod shortcut;
pub mod storage;
pub mod ui;
pub mod win32;

pub use error::{Error, Result};

/// Run the application. Secondary instances forward their command and return
/// immediately; the primary instance runs the message loop until all windows
/// are closed.
pub fn run() -> Result<()> {
    app::run()
}

/// Report a fatal startup/runtime error to the user. Uses a message box because
/// release builds run without a console (`#![windows_subsystem = "windows"]`).
pub fn report_fatal_error(error: &Error) {
    use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
    use windows::core::PCWSTR;

    let text = win32::wide(&format!("DesktopDrawers could not start:\n\n{error}"));
    let title = win32::wide("DesktopDrawers");
    unsafe {
        MessageBoxW(
            None,
            PCWSTR(text.as_ptr()),
            PCWSTR(title.as_ptr()),
            MB_OK | MB_ICONERROR,
        );
    }
}
