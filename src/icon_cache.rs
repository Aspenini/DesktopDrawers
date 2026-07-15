//! Per-drawer icon image list. Icons are extracted with the Shell
//! (`SHGetFileInfoW`) and inserted into an `HIMAGELIST` sized to the drawer's
//! icon size. The list is disposable and rebuilt on demand.

use std::path::Path;

use windows::Win32::UI::Controls::{
    HIMAGELIST, ILC_COLOR32, ILC_MASK, ImageList_Create, ImageList_Destroy, ImageList_Remove,
    ImageList_ReplaceIcon,
};
use windows::Win32::UI::Shell::{
    SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON, SHGFI_USEFILEATTRIBUTES,
};
use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, HICON};
use windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES;
use windows::core::PCWSTR;

use crate::win32::wide;

/// Owns an `HIMAGELIST` and the icons added to it.
pub struct IconList {
    handle: HIMAGELIST,
}

impl IconList {
    /// Create an image list for `size`-pixel icons. Index 0 is always a generic
    /// fallback icon so broken shortcuts still render.
    pub fn new(size: u32) -> IconList {
        let size = size as i32;
        let handle = unsafe { ImageList_Create(size, size, ILC_COLOR32 | ILC_MASK, 8, 8) };
        let mut list = IconList { handle };
        // Slot 0: generic document icon as a fallback.
        list.push_generic_fallback();
        list
    }

    pub fn handle(&self) -> HIMAGELIST {
        self.handle
    }

    /// The index of the generic fallback icon.
    fn fallback_index(&self) -> i32 {
        0
    }

    /// Drop every extracted icon and restore just the fallback (index 0). Called
    /// before a full repopulate so the image list can't grow without bound.
    pub fn reset(&mut self) {
        unsafe {
            let _ = ImageList_Remove(self.handle, -1); // -1 removes all images
        }
        self.push_generic_fallback();
    }

    /// Extract the icon for `path` and add it, returning its image index. On any
    /// failure returns [`fallback_index`](Self::fallback_index).
    pub fn add_for_path(&mut self, path: &Path) -> i32 {
        match extract_icon(path) {
            Some(icon) => {
                let idx = unsafe { ImageList_ReplaceIcon(self.handle, -1, icon) };
                unsafe {
                    let _ = DestroyIcon(icon);
                }
                if idx < 0 { self.fallback_index() } else { idx }
            }
            None => self.fallback_index(),
        }
    }

    fn push_generic_fallback(&mut self) {
        // A generic file icon (SHGFI_USEFILEATTRIBUTES needs no real file) that
        // occupies index 0 as the fallback for broken/unresolvable shortcuts.
        if let Some(icon) = extract_generic() {
            unsafe {
                ImageList_ReplaceIcon(self.handle, -1, icon);
                let _ = DestroyIcon(icon);
            }
        }
    }
}

impl Drop for IconList {
    fn drop(&mut self) {
        if self.handle.0 != 0 {
            unsafe {
                let _ = ImageList_Destroy(self.handle);
            }
        }
    }
}

/// Extract an `HICON` for a filesystem path via the Shell.
fn extract_icon(path: &Path) -> Option<HICON> {
    let path_w = wide(&path.to_string_lossy());
    let mut info = SHFILEINFOW::default();
    let flags = SHGFI_ICON | SHGFI_LARGEICON;
    let res = unsafe {
        SHGetFileInfoW(
            PCWSTR(path_w.as_ptr()),
            FILE_FLAGS_AND_ATTRIBUTES(0),
            Some(&mut info),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            flags,
        )
    };
    if res != 0 && !info.hIcon.0.is_null() {
        Some(info.hIcon)
    } else {
        None
    }
}

/// Generic icon for an unknown/broken file (uses file attributes so no real
/// file is needed).
fn extract_generic() -> Option<HICON> {
    let name = wide("file");
    let mut info = SHFILEINFOW::default();
    let flags = SHGFI_ICON | SHGFI_LARGEICON | SHGFI_USEFILEATTRIBUTES;
    let res = unsafe {
        SHGetFileInfoW(
            PCWSTR(name.as_ptr()),
            FILE_FLAGS_AND_ATTRIBUTES(0x80), // FILE_ATTRIBUTE_NORMAL
            Some(&mut info),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            flags,
        )
    };
    if res != 0 && !info.hIcon.0.is_null() {
        Some(info.hIcon)
    } else {
        None
    }
}
