//! The drawer manager window: a compact list of drawers with a "New Drawer"
//! button and per-drawer context actions.

use uuid::Uuid;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{GetStockObject, DEFAULT_GUI_FONT, HGDIOBJ};
use windows::Win32::UI::Controls::{
    LVCFMT_LEFT, LVCF_FMT, LVCF_SUBITEM, LVCF_TEXT, LVCF_WIDTH, LVCOLUMNW, LVIF_PARAM, LVIF_TEXT,
    LVITEMW, LVM_DELETEALLITEMS, LVM_EDITLABELW, LVM_GETNEXTITEM, LVM_INSERTCOLUMNW,
    LVM_INSERTITEMW, LVM_SETEXTENDEDLISTVIEWSTYLE, LVM_SETITEMTEXTW, LVNI_SELECTED,
    LVN_ENDLABELEDITW, LVN_ITEMACTIVATE, LVS_EDITLABELS, LVS_EX_DOUBLEBUFFER,
    LVS_EX_FULLROWSELECT, LVS_REPORT, LVS_SHOWSELALWAYS, LVS_SINGLESEL, NMHDR, NMITEMACTIVATE,
    NMLVDISPINFOW, NM_RCLICK,
};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::PCWSTR;

use crate::app::App;
use crate::error::Result;
use crate::win32::messages::*;
use crate::win32::{from_wide, get_userdata, instance, set_userdata, wide};

/// State attached to the manager window.
struct ManagerState {
    app: *mut App,
    listview: HWND,
    button: HWND,
    /// Row index -> drawer id.
    ids: Vec<Uuid>,
}

fn state<'a>(hwnd: HWND) -> Option<&'a mut ManagerState> {
    let ptr = unsafe { get_userdata::<ManagerState>(hwnd) };
    if ptr.is_null() { None } else { Some(unsafe { &mut *ptr }) }
}

/// Create and show the manager window.
pub fn create(app: *mut App) -> Result<HWND> {
    let class = wide(crate::ui::MANAGER_CLASS);
    let title = wide("DesktopDrawers");
    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            PCWSTR(class.as_ptr()),
            PCWSTR(title.as_ptr()),
            WS_OVERLAPPEDWINDOW & !WS_MAXIMIZEBOX,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            360,
            420,
            HWND::default(),
            HMENU::default(),
            instance(),
            Some(app as *const _),
        )?
    };
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOW);
    }
    Ok(hwnd)
}

pub extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_NCCREATE => {
                let cs = lparam.0 as *const CREATESTRUCTW;
                let app = (*cs).lpCreateParams as *mut App;
                let boxed = Box::new(ManagerState {
                    app,
                    listview: HWND::default(),
                    button: HWND::default(),
                    ids: Vec::new(),
                });
                set_userdata(hwnd, Box::into_raw(boxed));
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
            WM_CREATE => {
                if let Some(st) = state(hwnd) {
                    create_children(hwnd, st);
                    populate(st);
                }
                LRESULT(0)
            }
            WM_SIZE => {
                if let Some(st) = state(hwnd) {
                    layout(hwnd, st);
                }
                LRESULT(0)
            }
            WM_COMMAND => {
                let id = (wparam.0 & 0xFFFF) as i32;
                if id == ID_NEW_BUTTON {
                    on_new_drawer(hwnd);
                }
                LRESULT(0)
            }
            WM_NOTIFY => {
                handle_notify(hwnd, lparam);
                LRESULT(0)
            }
            WM_DESTROY => {
                if let Some(st) = state(hwnd) {
                    (*st.app).on_manager_closed();
                }
                LRESULT(0)
            }
            WM_NCDESTROY => {
                let ptr = get_userdata::<ManagerState>(hwnd);
                if !ptr.is_null() {
                    drop(Box::from_raw(ptr));
                    set_userdata::<ManagerState>(hwnd, std::ptr::null_mut());
                }
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

const ID_NEW_BUTTON: i32 = 1500;

fn create_children(parent: HWND, st: &mut ManagerState) {
    let lv_class = wide("SysListView32");
    let style = WS_CHILD
        | WS_VISIBLE
        | WINDOW_STYLE(LVS_REPORT | LVS_SINGLESEL | LVS_SHOWSELALWAYS | LVS_EDITLABELS);
    let listview = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            PCWSTR(lv_class.as_ptr()),
            PCWSTR::null(),
            style,
            0,
            0,
            0,
            0,
            parent,
            HMENU(ID_LISTVIEW as isize as *mut _),
            instance(),
            None,
        )
    }
    .unwrap_or_default();

    unsafe {
        SendMessageW(
            listview,
            LVM_SETEXTENDEDLISTVIEWSTYLE,
            WPARAM(0),
            LPARAM((LVS_EX_FULLROWSELECT | LVS_EX_DOUBLEBUFFER) as isize),
        );
    }
    set_font(listview);

    // Columns.
    insert_column(listview, 0, "Drawer", 210);
    insert_column(listview, 1, "Items", 90);

    // "New Drawer" button.
    let btn_class = wide("BUTTON");
    let btn_text = wide("+  New Drawer");
    let button = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            PCWSTR(btn_class.as_ptr()),
            PCWSTR(btn_text.as_ptr()),
            WS_CHILD | WS_VISIBLE | WINDOW_STYLE(BS_PUSHBUTTON as u32),
            0,
            0,
            0,
            0,
            parent,
            HMENU(ID_NEW_BUTTON as isize as *mut _),
            instance(),
            None,
        )
    }
    .unwrap_or_default();
    set_font(button);

    st.listview = listview;
    st.button = button;
}

fn layout(hwnd: HWND, st: &ManagerState) {
    let mut rc = RECT::default();
    unsafe {
        let _ = GetClientRect(hwnd, &mut rc);
    }
    let btn_h = 34;
    let pad = 8;
    unsafe {
        let _ = MoveWindow(st.listview, 0, 0, rc.right, rc.bottom - btn_h - pad, true);
        let _ = MoveWindow(
            st.button,
            pad,
            rc.bottom - btn_h,
            rc.right - pad * 2,
            btn_h - pad,
            true,
        );
    }
}

/// Rebuild the list from the app's drawers.
fn populate(st: &mut ManagerState) {
    unsafe {
        SendMessageW(st.listview, LVM_DELETEALLITEMS, WPARAM(0), LPARAM(0));
    }
    st.ids.clear();
    let app = unsafe { &*st.app };
    for (row, d) in app.drawers.iter().enumerate() {
        st.ids.push(d.id);
        insert_row(st.listview, row as i32, &d.name, d.items.len());
    }
}

fn insert_column(listview: HWND, index: i32, text: &str, width: i32) {
    let mut t = wide(text);
    let mut col = LVCOLUMNW {
        mask: LVCF_TEXT | LVCF_WIDTH | LVCF_FMT | LVCF_SUBITEM,
        fmt: LVCFMT_LEFT,
        cx: width,
        pszText: windows::core::PWSTR(t.as_mut_ptr()),
        iSubItem: index,
        ..Default::default()
    };
    unsafe {
        SendMessageW(
            listview,
            LVM_INSERTCOLUMNW,
            WPARAM(index as usize),
            LPARAM(&mut col as *mut _ as isize),
        );
    }
}

fn insert_row(listview: HWND, row: i32, name: &str, count: usize) {
    let mut name_w = wide(name);
    let mut item = LVITEMW {
        mask: LVIF_TEXT | LVIF_PARAM,
        iItem: row,
        iSubItem: 0,
        pszText: windows::core::PWSTR(name_w.as_mut_ptr()),
        lParam: LPARAM(row as isize),
        ..Default::default()
    };
    unsafe {
        SendMessageW(
            listview,
            LVM_INSERTITEMW,
            WPARAM(0),
            LPARAM(&mut item as *mut _ as isize),
        );
    }
    let count_text = format!("{count} item{}", if count == 1 { "" } else { "s" });
    let mut ct = wide(&count_text);
    let mut sub = LVITEMW {
        iItem: row,
        iSubItem: 1,
        pszText: windows::core::PWSTR(ct.as_mut_ptr()),
        ..Default::default()
    };
    unsafe {
        SendMessageW(
            listview,
            LVM_SETITEMTEXTW,
            WPARAM(row as usize),
            LPARAM(&mut sub as *mut _ as isize),
        );
    }
}

fn selected_row(listview: HWND) -> Option<i32> {
    let idx = unsafe {
        SendMessageW(
            listview,
            LVM_GETNEXTITEM,
            WPARAM(usize::MAX),
            LPARAM(LVNI_SELECTED as isize),
        )
        .0
    };
    if idx < 0 { None } else { Some(idx as i32) }
}

fn handle_notify(hwnd: HWND, lparam: LPARAM) {
    let nmhdr = unsafe { &*(lparam.0 as *const NMHDR) };
    if nmhdr.idFrom as i32 != ID_LISTVIEW {
        return;
    }
    match nmhdr.code {
        code if code == LVN_ITEMACTIVATE => {
            let nia = unsafe { &*(lparam.0 as *const NMITEMACTIVATE) };
            open_row(hwnd, nia.iItem);
        }
        code if code == NM_RCLICK => {
            show_context_menu(hwnd);
        }
        code if code == LVN_ENDLABELEDITW => {
            commit_rename(hwnd, lparam);
        }
        _ => {}
    }
}

fn open_row(hwnd: HWND, row: i32) {
    if row < 0 {
        return;
    }
    if let Some(st) = state(hwnd)
        && let Some(&id) = st.ids.get(row as usize) {
            unsafe {
                let _ = (*st.app).open_drawer(id);
            }
        }
}

fn commit_rename(hwnd: HWND, lparam: LPARAM) {
    let info = unsafe { &*(lparam.0 as *const NMLVDISPINFOW) };
    if info.item.pszText.is_null() {
        return; // edit cancelled
    }
    let text = unsafe { pwstr_to_string(info.item.pszText) };
    let text = text.trim().to_string();
    if text.is_empty() {
        return;
    }
    if let Some(st) = state(hwnd)
        && let Some(&id) = st.ids.get(info.item.iItem as usize) {
            unsafe {
                if let Some(d) = (*st.app).drawer_mut(id) {
                    d.name = text.clone();
                    let _ = (*st.app).save_drawer(id);
                }
            }
            // Update the visible label + retitle any open drawer window.
            populate(st);
        }
}

unsafe fn pwstr_to_string(p: windows::core::PWSTR) -> String {
    unsafe {
        let mut len = 0usize;
        while *p.0.add(len) != 0 {
            len += 1;
        }
        let slice = std::slice::from_raw_parts(p.0, len + 1);
        from_wide(slice)
    }
}

fn on_new_drawer(hwnd: HWND) {
    let st_ptr = state(hwnd).map(|s| s as *mut ManagerState);
    let Some(st_ptr) = st_ptr else { return };
    let app = unsafe { (*st_ptr).app };
    if let Some(params) = crate::ui::create_drawer::show(hwnd) {
        let drawer = crate::model::Drawer::new(
            params.name,
            params.columns,
            params.rows,
            params.icon_size,
        );
        let id = drawer.id;
        unsafe {
            match (*app).add_drawer(drawer) {
                Ok(_) => {
                    if params.create_desktop_shortcut {
                        let name = (*app).drawer(id).map(|d| d.name.clone()).unwrap_or_default();
                        let _ = crate::shortcut::create_desktop_shortcut_for_drawer(
                            &id.to_string(),
                            &name,
                        );
                    }
                    if let Some(st) = state(hwnd) {
                        populate(st);
                    }
                    let _ = (*app).open_drawer(id);
                }
                Err(e) => crate::ui::message_box(Some(hwnd), &format!("{e}"), "DesktopDrawers"),
            }
        }
    }
}

fn show_context_menu(hwnd: HWND) {
    let Some(st) = state(hwnd) else { return };
    let Some(row) = selected_row(st.listview) else {
        return;
    };
    let Some(&id) = st.ids.get(row as usize) else {
        return;
    };
    let app = st.app;

    let mut pt = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut pt);
    }

    let menu = unsafe { CreatePopupMenu() }.unwrap_or_default();
    unsafe {
        append(menu, IDM_OPEN, "Open");
        append(menu, IDM_RENAME, "Rename");
        append(menu, IDM_EDIT_LAYOUT, "Edit Layout…");
        append_sep(menu);
        append(menu, IDM_CREATE_DESKTOP_SHORTCUT, "Create Desktop Shortcut");
        append(menu, IDM_DUPLICATE, "Duplicate");
        append(menu, IDM_OPEN_DATA_LOCATION, "Open Data Location");
        append_sep(menu);
        append(menu, IDM_DELETE, "Delete");
    }

    let choice = unsafe {
        TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_RIGHTBUTTON,
            pt.x,
            pt.y,
            0,
            hwnd,
            None,
        )
    };
    unsafe {
        let _ = DestroyMenu(menu);
    }

    let choice = choice.0 as usize;
    unsafe {
        match choice {
            IDM_OPEN => {
                let _ = (*app).open_drawer(id);
            }
            IDM_RENAME => {
                SendMessageW(st.listview, LVM_EDITLABELW, WPARAM(row as usize), LPARAM(0));
            }
            IDM_EDIT_LAYOUT => {
                crate::ui::drawer_settings::show(hwnd, app, id);
                if let Some(st) = state(hwnd) {
                    populate(st);
                }
            }
            IDM_CREATE_DESKTOP_SHORTCUT => {
                let name = (*app).drawer(id).map(|d| d.name.clone()).unwrap_or_default();
                match crate::shortcut::create_desktop_shortcut_for_drawer(&id.to_string(), &name) {
                    Ok(_) => crate::ui::message_box(Some(hwnd), "Desktop shortcut created.", "DesktopDrawers"),
                    Err(e) => crate::ui::message_box(Some(hwnd), &format!("{e}"), "DesktopDrawers"),
                }
            }
            IDM_DUPLICATE => {
                if let Some(orig) = (*app).drawer(id).cloned() {
                    let dup = orig.duplicated(format!("{} (copy)", orig.name));
                    let new_id = dup.id;
                    if (*app).add_drawer(dup).is_ok() {
                        // Copy managed .lnk files.
                        let _ = copy_items(&*app, id, new_id);
                        if let Some(st) = state(hwnd) {
                            populate(st);
                        }
                    }
                }
            }
            IDM_OPEN_DATA_LOCATION => {
                let dir = (*app).data_location(id);
                let _ = crate::shortcut::launch(&dir);
            }
            IDM_DELETE
                if confirm_delete(hwnd) => {
                    let _ = (*app).delete_drawer(id);
                    if let Some(st) = state(hwnd) {
                        populate(st);
                    }
                }
            _ => {}
        }
    }
}

/// Copy managed `.lnk` files from one drawer directory to another (used by
/// Duplicate).
fn copy_items(app: &App, from: Uuid, to: Uuid) -> std::io::Result<()> {
    let src = app.store.drawer_items_dir(from);
    let dst = app.store.drawer_items_dir(to);
    std::fs::create_dir_all(&dst)?;
    if let Ok(entries) = std::fs::read_dir(&src) {
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().and_then(|s| s.to_str()) == Some("lnk")
                && let Some(name) = p.file_name() {
                    let _ = std::fs::copy(&p, dst.join(name));
                }
        }
    }
    Ok(())
}

fn confirm_delete(hwnd: HWND) -> bool {
    let text = wide("Delete this drawer? Its shortcut targets (programs, files, folders) are NOT deleted.");
    let title = wide("Delete Drawer");
    let r = unsafe {
        MessageBoxW(
            hwnd,
            PCWSTR(text.as_ptr()),
            PCWSTR(title.as_ptr()),
            MB_YESNO | MB_ICONWARNING,
        )
    };
    r == IDYES
}

// ---- small helpers ------------------------------------------------------

fn set_font(hwnd: HWND) {
    let font = unsafe { GetStockObject(DEFAULT_GUI_FONT) };
    unsafe {
        SendMessageW(
            hwnd,
            WM_SETFONT,
            WPARAM(font.0 as usize),
            LPARAM(1),
        );
    }
    let _ = HGDIOBJ::default();
}

unsafe fn append(menu: HMENU, id: usize, text: &str) {
    let t = wide(text);
    unsafe {
        let _ = AppendMenuW(menu, MF_STRING, id, PCWSTR(t.as_ptr()));
    }
}
unsafe fn append_sep(menu: HMENU) {
    unsafe {
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
    }
}
