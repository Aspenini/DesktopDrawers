//! Windows shortcut (`.lnk`) creation, launching, and target resolution via the
//! Shell COM interfaces. All functions assume COM is initialized on the caller
//! thread (see [`ComApartment`]).

use std::path::{Path, PathBuf};

use windows::Win32::Foundation::MAX_PATH;
use windows::Win32::System::Com::{
    CoCreateInstance, CoTaskMemFree, IPersistFile, CLSCTX_INPROC_SERVER,
};
use windows::Win32::System::Ole::{OleInitialize, OleUninitialize};
use windows::Win32::UI::Shell::{
    IShellLinkW, SHGetKnownFolderPath, SHELLEXECUTEINFOW_0, ShellExecuteExW, ShellLink,
    FOLDERID_Desktop, KF_FLAG_DEFAULT, SHELLEXECUTEINFOW,
};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::{Interface, PCWSTR, PWSTR};

use crate::error::{Error, Result};
use crate::win32::{from_wide, wide};

/// RAII guard for the UI thread's OLE apartment. `OleInitialize` sets up an STA
/// (like `CoInitializeEx(APARTMENTTHREADED)`) *and* the OLE services required by
/// `RegisterDragDrop`, the Shell, and clipboard.
pub struct ComApartment;

impl ComApartment {
    pub fn init_sta() -> Result<ComApartment> {
        unsafe { OleInitialize(None)? };
        Ok(ComApartment)
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        unsafe { OleUninitialize() };
    }
}

/// Create a `.lnk` at `lnk_path` pointing at `target`.
pub fn create_lnk(
    lnk_path: &Path,
    target: &Path,
    args: Option<&str>,
    working_dir: Option<&Path>,
    description: Option<&str>,
    icon: Option<(&Path, i32)>,
) -> Result<()> {
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;

        let target_w = wide(&target.to_string_lossy());
        link.SetPath(PCWSTR(target_w.as_ptr()))?;

        if let Some(a) = args {
            let a_w = wide(a);
            link.SetArguments(PCWSTR(a_w.as_ptr()))?;
        }
        if let Some(wd) = working_dir {
            let wd_w = wide(&wd.to_string_lossy());
            link.SetWorkingDirectory(PCWSTR(wd_w.as_ptr()))?;
        }
        if let Some(d) = description {
            let d_w = wide(d);
            link.SetDescription(PCWSTR(d_w.as_ptr()))?;
        }
        if let Some((icon_path, index)) = icon {
            let ip_w = wide(&icon_path.to_string_lossy());
            link.SetIconLocation(PCWSTR(ip_w.as_ptr()), index)?;
        }

        let persist: IPersistFile = link.cast()?;
        let lnk_w = wide(&lnk_path.to_string_lossy());
        persist.Save(PCWSTR(lnk_w.as_ptr()), true)?;
    }
    Ok(())
}

/// Resolve the target path a `.lnk` points at (best-effort; `None` if it can't
/// be read or is not a filesystem target).
pub fn resolve_target(lnk_path: &Path) -> Option<PathBuf> {
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
        let persist: IPersistFile = link.cast().ok()?;
        let lnk_w = wide(&lnk_path.to_string_lossy());
        persist
            .Load(PCWSTR(lnk_w.as_ptr()), windows::Win32::System::Com::STGM_READ)
            .ok()?;

        let mut buf = [0u16; MAX_PATH as usize];
        link.GetPath(&mut buf, std::ptr::null_mut(), 0).ok()?;
        let s = from_wide(&buf);
        if s.is_empty() {
            None
        } else {
            Some(PathBuf::from(s))
        }
    }
}

/// Launch a `.lnk` (or any shell-openable path) via `ShellExecuteExW`, letting
/// Windows resolve executables, documents, folders, URLs, arguments, working
/// directories, and elevation.
pub fn launch(lnk_path: &Path) -> Result<()> {
    let file = wide(&lnk_path.to_string_lossy());
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: windows::Win32::UI::Shell::SEE_MASK_FLAG_NO_UI,
        lpFile: PCWSTR(file.as_ptr()),
        nShow: SW_SHOWNORMAL.0,
        Anonymous: SHELLEXECUTEINFOW_0::default(),
        ..Default::default()
    };
    unsafe { ShellExecuteExW(&mut info)? };
    Ok(())
}

/// Launch with an explicit verb (e.g. `"runas"` for elevation, `"properties"`).
pub fn launch_verb(lnk_path: &Path, verb: &str) -> Result<()> {
    let file = wide(&lnk_path.to_string_lossy());
    let verb_w = wide(verb);
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: windows::Win32::UI::Shell::SEE_MASK_INVOKEIDLIST,
        lpVerb: PCWSTR(verb_w.as_ptr()),
        lpFile: PCWSTR(file.as_ptr()),
        nShow: SW_SHOWNORMAL.0,
        Anonymous: SHELLEXECUTEINFOW_0::default(),
        ..Default::default()
    };
    unsafe { ShellExecuteExW(&mut info)? };
    Ok(())
}

/// The current user's Desktop directory.
pub fn desktop_dir() -> Result<PathBuf> {
    unsafe {
        let pwstr: PWSTR = SHGetKnownFolderPath(&FOLDERID_Desktop, KF_FLAG_DEFAULT, None)?;
        let s = pwstr.to_string().map_err(|e| Error::Other(e.to_string()))?;
        CoTaskMemFree(Some(pwstr.0 as *const _));
        Ok(PathBuf::from(s))
    }
}

/// Create a desktop shortcut that opens a specific drawer:
/// `DesktopDrawers.exe --open <id>`.
pub fn create_desktop_shortcut_for_drawer(drawer_id: &str, drawer_name: &str) -> Result<PathBuf> {
    let exe = std::env::current_exe().map_err(|e| Error::io("current_exe", e))?;
    let desktop = desktop_dir()?;
    let safe = sanitize_filename(drawer_name);
    let lnk = desktop.join(format!("{safe}.lnk"));
    let args = format!("--open \"{drawer_id}\"");
    let workdir = exe.parent().map(Path::to_path_buf);
    create_lnk(
        &lnk,
        &exe,
        Some(&args),
        workdir.as_deref(),
        Some(&format!("Open the {drawer_name} drawer")),
        Some((&exe, 0)),
    )?;
    Ok(lnk)
}

/// Strip characters not allowed in Windows filenames.
pub fn sanitize_filename(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if (c as u32) < 0x20 => '_',
            c => c,
        })
        .collect();
    let trimmed = cleaned.trim().trim_end_matches('.').trim();
    if trimmed.is_empty() {
        "Drawer".to_string()
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_bad_chars() {
        assert_eq!(sanitize_filename("Emu/lation:*?"), "Emu_lation___");
        assert_eq!(sanitize_filename("   "), "Drawer");
        assert_eq!(sanitize_filename("Games."), "Games");
    }
}
