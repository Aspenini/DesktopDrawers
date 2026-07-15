//! OLE `IDropTarget` so files, folders, and shortcuts can be dragged from
//! Explorer (or the desktop) straight onto a drawer.

use std::cell::Cell;
use std::path::PathBuf;

use windows::Win32::Foundation::{HWND, POINTL, S_OK};
use windows::Win32::System::Com::{
    DVASPECT_CONTENT, FORMATETC, IDataObject, TYMED_HGLOBAL,
};
use windows::Win32::System::Ole::{
    CF_HDROP, DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_NONE, IDropTarget, IDropTarget_Impl,
    ReleaseStgMedium,
};
use windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS;
use windows::Win32::UI::Shell::{DragQueryFileW, HDROP};
use windows::core::{Result, implement};

/// A drop target bound to a drawer. Dropped paths are handed to
/// [`crate::ui::drawer::add_paths`] on the owning drawer window.
#[implement(IDropTarget)]
pub struct DropTarget {
    /// The drawer window whose state receives the items.
    owner: HWND,
    /// Whether the current drag carries file data (decided in `DragEnter`,
    /// reused by `DragOver`, which isn't given the data object).
    accept: Cell<bool>,
}

impl DropTarget {
    /// Build a COM drop target for `owner` (the drawer window).
    pub fn create(owner: HWND) -> IDropTarget {
        DropTarget {
            owner,
            accept: Cell::new(false),
        }
        .into()
    }
}

// The `*mut DROPEFFECT` out-parameters are dictated by the COM vtable; writing
// through them is the documented contract of `IDropTarget`.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
impl IDropTarget_Impl for DropTarget_Impl {
    fn DragEnter(
        &self,
        data: Option<&IDataObject>,
        _keys: MODIFIERKEYS_FLAGS,
        _pt: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> Result<()> {
        let ok = has_files(data);
        self.accept.set(ok);
        unsafe { *effect = if ok { DROPEFFECT_COPY } else { DROPEFFECT_NONE } };
        Ok(())
    }

    fn DragOver(
        &self,
        _keys: MODIFIERKEYS_FLAGS,
        _pt: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> Result<()> {
        unsafe { *effect = if self.accept.get() { DROPEFFECT_COPY } else { DROPEFFECT_NONE } };
        Ok(())
    }

    fn DragLeave(&self) -> Result<()> {
        Ok(())
    }

    fn Drop(
        &self,
        data: Option<&IDataObject>,
        _keys: MODIFIERKEYS_FLAGS,
        _pt: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> Result<()> {
        let paths = extract_paths(data);
        if !paths.is_empty() {
            crate::ui::drawer::add_paths(self.owner, &paths);
        }
        unsafe { *effect = DROPEFFECT_COPY };
        Ok(())
    }
}

/// A `FORMATETC` describing a `CF_HDROP` file list in global memory.
fn hdrop_format() -> FORMATETC {
    FORMATETC {
        cfFormat: CF_HDROP.0,
        ptd: std::ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT.0,
        lindex: -1,
        tymed: TYMED_HGLOBAL.0 as u32,
    }
}

/// Does the data object carry a file list?
fn has_files(data: Option<&IDataObject>) -> bool {
    let Some(data) = data else { return false };
    let fmt = hdrop_format();
    // QueryGetData returns S_OK only when the exact format is available
    // (S_FALSE otherwise, which is still "success", so compare explicitly).
    unsafe { data.QueryGetData(&fmt) == S_OK }
}

/// Pull the dropped file paths out of the data object's `CF_HDROP`.
fn extract_paths(data: Option<&IDataObject>) -> Vec<PathBuf> {
    let Some(data) = data else { return Vec::new() };
    let fmt = hdrop_format();
    let mut medium = match unsafe { data.GetData(&fmt) } {
        Ok(m) => m,
        Err(_) => return Vec::new(),
    };

    let hdrop = HDROP(unsafe { medium.u.hGlobal }.0);
    let count = unsafe { DragQueryFileW(hdrop, 0xFFFF_FFFF, None) };
    let mut out = Vec::with_capacity(count as usize);
    for i in 0..count {
        let needed = unsafe { DragQueryFileW(hdrop, i, None) } as usize;
        if needed == 0 {
            continue;
        }
        let mut buf = vec![0u16; needed + 1];
        let len = unsafe { DragQueryFileW(hdrop, i, Some(&mut buf)) } as usize;
        if len > 0 {
            out.push(PathBuf::from(String::from_utf16_lossy(&buf[..len])));
        }
    }

    unsafe { ReleaseStgMedium(&mut medium) };
    out
}
