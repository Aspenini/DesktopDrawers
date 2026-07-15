//! Thin, self-contained wrappers around the raw Win32 surface. Keeping the
//! `unsafe` here lets the rest of the crate stay readable.

pub mod messages;

use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowLongPtrW, SetWindowLongPtrW, GWLP_USERDATA,
};

/// Convert a Rust string to a NUL-terminated UTF-16 buffer.
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Decode a NUL-terminated UTF-16 buffer back to a `String`.
pub fn from_wide(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

/// The module handle for this executable, as an `HINSTANCE`.
pub fn instance() -> HINSTANCE {
    // GetModuleHandleW(None) never fails for the current process.
    let h = unsafe { GetModuleHandleW(None) }.unwrap_or_default();
    HINSTANCE(h.0)
}

/// Extract the signed X coordinate packed into an `LPARAM` (e.g. `WM_CONTEXTMENU`).
pub fn get_x_lparam(lp: LPARAM) -> i32 {
    (lp.0 & 0xFFFF) as i16 as i32
}
/// Extract the signed Y coordinate packed into an `LPARAM`.
pub fn get_y_lparam(lp: LPARAM) -> i32 {
    ((lp.0 >> 16) & 0xFFFF) as i16 as i32
}

/// Store a raw pointer in a window's `GWLP_USERDATA` slot.
///
/// # Safety
/// `hwnd` must be valid and the pointer must outlive the window (or be cleared
/// on `WM_NCDESTROY`).
pub unsafe fn set_userdata<T>(hwnd: HWND, ptr: *mut T) {
    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, ptr as isize);
    }
}

/// Read the pointer previously stored with [`set_userdata`].
///
/// # Safety
/// `hwnd` must be valid, and the stored pointer must currently be a `*mut T`
/// (or null) written by [`set_userdata`].
pub unsafe fn get_userdata<T>(hwnd: HWND) -> *mut T {
    unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut T }
}
