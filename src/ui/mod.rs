//! Window-class registration and shared UI helpers.

pub mod create_drawer;
pub mod drawer;
pub mod drawer_settings;
pub mod manager;

use windows::Win32::Foundation::{HWND, WPARAM};
use windows::Win32::Graphics::Gdi::{GetSysColorBrush, HBRUSH, COLOR_WINDOW, COLOR_3DFACE};
use windows::Win32::UI::WindowsAndMessaging::{
    LoadCursorW, MessageBoxW, RegisterClassExW, SendMessageW, IDC_ARROW, MB_ICONINFORMATION,
    MB_OK, WNDCLASSEXW, CS_HREDRAW, CS_VREDRAW, CS_DBLCLKS, WNDPROC,
};
use windows::core::PCWSTR;

use crate::error::{Error, Result};
use crate::win32::{instance, wide};

pub const MANAGER_CLASS: &str = "DesktopDrawers.Manager";
pub const DRAWER_CLASS: &str = "DesktopDrawers.Drawer";
pub const MESSAGE_CLASS: &str = "DesktopDrawers.Message";
pub const CREATE_CLASS: &str = "DesktopDrawers.Create";
pub const SETTINGS_CLASS: &str = "DesktopDrawers.Settings";

/// Register every window class once at startup.
pub fn register_all_classes() -> Result<()> {
    register_class(MANAGER_CLASS, Some(manager::wndproc), COLOR_WINDOW)?;
    register_class(DRAWER_CLASS, Some(drawer::wndproc), COLOR_WINDOW)?;
    register_class(MESSAGE_CLASS, Some(crate::app::message_wndproc), COLOR_WINDOW)?;
    register_class(CREATE_CLASS, Some(create_drawer::wndproc), COLOR_3DFACE)?;
    register_class(SETTINGS_CLASS, Some(drawer_settings::wndproc), COLOR_3DFACE)?;
    Ok(())
}

fn register_class(name: &str, wndproc: WNDPROC, bg: windows::Win32::Graphics::Gdi::SYS_COLOR_INDEX) -> Result<()> {
    let class_name = wide(name);
    let cursor = unsafe { LoadCursorW(None, IDC_ARROW) }.unwrap_or_default();
    let brush: HBRUSH = unsafe { GetSysColorBrush(bg) };
    let wc = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        style: CS_HREDRAW | CS_VREDRAW | CS_DBLCLKS,
        lpfnWndProc: wndproc,
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: instance(),
        hIcon: Default::default(),
        hCursor: cursor,
        hbrBackground: brush,
        lpszMenuName: PCWSTR::null(),
        lpszClassName: PCWSTR(class_name.as_ptr()),
        hIconSm: Default::default(),
    };
    let atom = unsafe { RegisterClassExW(&wc) };
    if atom == 0 {
        // Class may already be registered (idempotent startup) — treat the
        // "class already exists" error as success.
        let err = windows::core::Error::from_win32();
        // 1410 = ERROR_CLASS_ALREADY_EXISTS
        if err.code().0 as u32 & 0xFFFF == 1410 {
            return Ok(());
        }
        return Err(Error::Other(format!("RegisterClassExW({name}) failed: {err}")));
    }
    Ok(())
}

/// Show a simple informational message box.
pub fn message_box(owner: Option<HWND>, text: &str, title: &str) {
    let t = wide(text);
    let c = wide(title);
    unsafe {
        MessageBoxW(
            owner.unwrap_or_default(),
            PCWSTR(t.as_ptr()),
            PCWSTR(c.as_ptr()),
            MB_OK | MB_ICONINFORMATION,
        );
    }
}

/// Send a message to a control and return the raw LRESULT value.
pub fn send(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: windows::Win32::Foundation::LPARAM) -> isize {
    unsafe { SendMessageW(hwnd, msg, wparam, lparam).0 }
}
