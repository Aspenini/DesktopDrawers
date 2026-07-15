//! A drawer window: a native ListView in icon mode laid out as a fixed grid.

use std::path::PathBuf;

use uuid::Uuid;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{GetStockObject, DEFAULT_GUI_FONT};
use windows::Win32::UI::Controls::{
    LVIF_IMAGE, LVIF_PARAM, LVIF_TEXT, LVITEMW, LVM_DELETEALLITEMS, LVM_EDITLABELW,
    LVM_GETNEXTITEM, LVM_INSERTITEMW, LVM_SETICONSPACING, LVM_SETIMAGELIST, LVM_SETITEMPOSITION,
    LVNI_SELECTED, LVN_BEGINLABELEDITW, LVN_ENDLABELEDITW, LVN_ITEMACTIVATE, LVSIL_NORMAL,
    LVS_ALIGNLEFT, LVS_EX_DOUBLEBUFFER, LVS_EX_SNAPTOGRID, LVS_EDITLABELS, LVS_ICON,
    LVS_SHOWSELALWAYS, LVS_SINGLESEL, LVM_SETEXTENDEDLISTVIEWSTYLE, NMHDR, NMITEMACTIVATE,
    NMLVDISPINFOW, NM_RCLICK,
};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::PCWSTR;

use crate::app::App;
use crate::error::Result;
use crate::icon_cache::IconList;
use crate::model::WindowPosition;
use crate::win32::messages::*;
use crate::win32::{from_wide, get_userdata, instance, set_userdata, wide};

struct DrawerState {
    app: *mut App,
    id: Uuid,
    listview: HWND,
    icons: IconList,
    /// ListView row index -> item id.
    item_ids: Vec<Uuid>,
}

fn state<'a>(hwnd: HWND) -> Option<&'a mut DrawerState> {
    let ptr = unsafe { get_userdata::<DrawerState>(hwnd) };
    if ptr.is_null() { None } else { Some(unsafe { &mut *ptr }) }
}

/// Create and show a drawer window sized to its grid.
///
/// `app` must be a valid pointer to the live `App` (it always is: `App` outlives
/// every window on the single UI thread).
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub fn create(app: *mut App, id: Uuid) -> Result<HWND> {
    let (name, cols, rows, icon_px, pos) = {
        let a = unsafe { &*app };
        let d = a.drawer(id).expect("drawer exists");
        (
            d.name.clone(),
            d.columns,
            d.rows,
            d.icon_size,
            d.window_position.clone(),
        )
    };

    let class = wide(crate::ui::DRAWER_CLASS);
    let title = wide(&name);

    // Tool-window style: caption + close button only (no min/max, no resize).
    let style = WS_CAPTION | WS_SYSMENU | WS_CLIPCHILDREN;
    let ex_style = WS_EX_TOOLWINDOW;

    let (x, y) = pos
        .as_ref()
        .map(|p| (p.x, p.y))
        .unwrap_or((CW_USEDEFAULT, CW_USEDEFAULT));

    // Box the id so WM_NCCREATE can recover both app + id.
    let create_ctx = Box::new((app, id));
    let hwnd = unsafe {
        CreateWindowExW(
            ex_style,
            PCWSTR(class.as_ptr()),
            PCWSTR(title.as_ptr()),
            style,
            x,
            y,
            400,
            300,
            HWND::default(),
            HMENU::default(),
            instance(),
            Some(Box::into_raw(create_ctx) as *const _),
        )?
    };

    // Size the window's client area to exactly fit the grid.
    size_to_grid(hwnd, cols, rows, icon_px);
    ensure_on_screen(hwnd);

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
                let ctx = Box::from_raw((*cs).lpCreateParams as *mut (*mut App, Uuid));
                let (app, id) = *ctx;
                let d = &*app;
                let icon_px = d.drawer(id).map(|dr| dr.icon_size).unwrap_or(48);
                let boxed = Box::new(DrawerState {
                    app,
                    id,
                    listview: HWND::default(),
                    icons: IconList::new(icon_px),
                    item_ids: Vec::new(),
                });
                set_userdata(hwnd, Box::into_raw(boxed));
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
            WM_CREATE => {
                if let Some(st) = state(hwnd) {
                    create_listview(hwnd, st);
                    populate(st);
                }
                LRESULT(0)
            }
            WM_SIZE => {
                if let Some(st) = state(hwnd) {
                    let mut rc = RECT::default();
                    let _ = GetClientRect(hwnd, &mut rc);
                    let _ = MoveWindow(st.listview, 0, 0, rc.right, rc.bottom, true);
                    reposition_items(st);
                }
                LRESULT(0)
            }
            WM_NOTIFY => {
                handle_notify(hwnd, lparam);
                LRESULT(0)
            }
            WM_CONTEXTMENU => {
                // Right-click that didn't hit an item (empty space).
                empty_space_menu(hwnd, lparam);
                LRESULT(0)
            }
            WM_KEYDOWN => {
                on_keydown(hwnd, wparam);
                LRESULT(0)
            }
            WM_MOVE => {
                save_position(hwnd);
                LRESULT(0)
            }
            WM_DESTROY => {
                if let Some(st) = state(hwnd) {
                    save_position(hwnd);
                    let id = st.id;
                    (*st.app).on_drawer_closed(id);
                }
                LRESULT(0)
            }
            WM_NCDESTROY => {
                let ptr = get_userdata::<DrawerState>(hwnd);
                if !ptr.is_null() {
                    drop(Box::from_raw(ptr));
                    set_userdata::<DrawerState>(hwnd, std::ptr::null_mut());
                }
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

fn create_listview(parent: HWND, st: &mut DrawerState) {
    let lv_class = wide("SysListView32");
    let base = LVS_ICON | LVS_SINGLESEL | LVS_SHOWSELALWAYS | LVS_EDITLABELS | LVS_ALIGNLEFT;
    let style = WS_CHILD | WS_VISIBLE | WINDOW_STYLE(base);
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
            LPARAM((LVS_EX_DOUBLEBUFFER | LVS_EX_SNAPTOGRID) as isize),
        );
        SendMessageW(
            listview,
            LVM_SETIMAGELIST,
            WPARAM(LVSIL_NORMAL as usize),
            LPARAM(st.icons.handle().0),
        );
    }
    set_font(listview);
    st.listview = listview;
}

/// Rebuild every item, its icon, and its grid position.
fn populate(st: &mut DrawerState) {
    unsafe {
        SendMessageW(st.listview, LVM_DELETEALLITEMS, WPARAM(0), LPARAM(0));
    }
    st.item_ids.clear();
    // Discard previously-extracted icons so the image list doesn't accumulate
    // orphans across repopulates (rename/add/arrange all rebuild the view).
    st.icons.reset();

    let (items, drawer_dir, icon_px) = {
        let app = unsafe { &*st.app };
        let Some(d) = app.drawer(st.id) else { return };
        (
            d.items.clone(),
            app.store.drawer_dir(st.id),
            d.icon_size,
        )
    };

    set_icon_spacing(st.listview, icon_px);

    for (row, item) in items.iter().enumerate() {
        let full = drawer_dir.join(&item.shortcut);
        let image = st.icons.add_for_path(&full);
        insert_item(st.listview, row as i32, &item.display_name, image);
        set_item_position(st.listview, row as i32, item.column, item.row, icon_px);
        st.item_ids.push(item.id);
    }
}

fn reposition_items(st: &DrawerState) {
    let app = unsafe { &*st.app };
    let Some(d) = app.drawer(st.id) else { return };
    for (row, item) in d.items.iter().enumerate() {
        set_item_position(st.listview, row as i32, item.column, item.row, d.icon_size);
    }
}

fn insert_item(listview: HWND, row: i32, name: &str, image: i32) {
    let mut name_w = wide(name);
    let mut item = LVITEMW {
        mask: LVIF_TEXT | LVIF_IMAGE | LVIF_PARAM,
        iItem: row,
        iSubItem: 0,
        pszText: windows::core::PWSTR(name_w.as_mut_ptr()),
        iImage: image,
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
}

fn cell_size(icon_px: u32) -> (i32, i32) {
    let w = icon_px as i32 + 28;
    let h = icon_px as i32 + 40;
    (w, h)
}

fn set_icon_spacing(listview: HWND, icon_px: u32) {
    let (w, h) = cell_size(icon_px);
    let packed = ((h as u32) << 16) | (w as u32 & 0xFFFF);
    unsafe {
        SendMessageW(
            listview,
            LVM_SETICONSPACING,
            WPARAM(0),
            LPARAM(packed as i32 as isize),
        );
    }
}

fn set_item_position(listview: HWND, row: i32, col: u32, grid_row: u32, icon_px: u32) {
    let (w, h) = cell_size(icon_px);
    let margin = 8;
    let x = margin + col as i32 * w;
    let y = margin + grid_row as i32 * h;
    let packed = ((y as u32 & 0xFFFF) << 16) | (x as u32 & 0xFFFF);
    unsafe {
        SendMessageW(
            listview,
            LVM_SETITEMPOSITION,
            WPARAM(row as usize),
            LPARAM(packed as i32 as isize),
        );
    }
}

/// Resize the window's client area to exactly fit the grid.
fn size_to_grid(hwnd: HWND, cols: u32, rows: u32, icon_px: u32) {
    let (cw, ch) = cell_size(icon_px);
    let margin = 8;
    let client_w = margin * 2 + cols as i32 * cw;
    let client_h = margin * 2 + rows as i32 * ch;

    let mut rc = RECT {
        left: 0,
        top: 0,
        right: client_w,
        bottom: client_h,
    };
    unsafe {
        let style = WINDOW_STYLE(GetWindowLongW(hwnd, GWL_STYLE) as u32);
        let ex = WINDOW_EX_STYLE(GetWindowLongW(hwnd, GWL_EXSTYLE) as u32);
        let _ = AdjustWindowRectEx(&mut rc, style, false, ex);
        let _ = SetWindowPos(hwnd, HWND::default(),
            0,
            0,
            rc.right - rc.left,
            rc.bottom - rc.top,
            SWP_NOMOVE | SWP_NOZORDER,
        );
    }
}

/// Keep the window within a visible monitor.
fn ensure_on_screen(hwnd: HWND) {
    unsafe {
        let mut rc = RECT::default();
        let _ = GetWindowRect(hwnd, &mut rc);
        let vx = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let vy = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let vw = GetSystemMetrics(SM_CXVIRTUALSCREEN);
        let vh = GetSystemMetrics(SM_CYVIRTUALSCREEN);
        let mut x = rc.left;
        let mut y = rc.top;
        let w = rc.right - rc.left;
        let h = rc.bottom - rc.top;
        if x < vx || x + w > vx + vw {
            x = vx + (vw - w).max(0) / 2;
        }
        if y < vy || y + h > vy + vh {
            y = vy + (vh - h).max(0) / 2;
        }
        if (x, y) != (rc.left, rc.top) {
            let _ = SetWindowPos(hwnd, HWND::default(), x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER);
        }
    }
}

fn save_position(hwnd: HWND) {
    let Some(st) = state(hwnd) else { return };
    let mut rc = RECT::default();
    unsafe {
        if GetWindowRect(hwnd, &mut rc).is_err() {
            return;
        }
    }
    let id = st.id;
    unsafe {
        if let Some(d) = (*st.app).drawer_mut(id) {
            d.window_position = Some(WindowPosition {
                x: rc.left,
                y: rc.top,
                monitor: String::new(),
            });
            let _ = (*st.app).save_drawer(id);
        }
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
            launch_row(hwnd, nia.iItem);
        }
        code if code == NM_RCLICK => {
            let nia = unsafe { &*(lparam.0 as *const NMITEMACTIVATE) };
            if nia.iItem >= 0 {
                item_context_menu(hwnd, nia.iItem);
            } else {
                empty_space_menu(hwnd, LPARAM(-1));
            }
        }
        code if code == LVN_BEGINLABELEDITW => { /* allow */ }
        code if code == LVN_ENDLABELEDITW => {
            commit_rename(hwnd, lparam);
        }
        _ => {}
    }
}

fn launch_row(hwnd: HWND, row: i32) {
    if row < 0 {
        return;
    }
    if let Some(st) = state(hwnd)
        && let Some(path) = item_path(st, row)
            && let Err(e) = crate::shortcut::launch(&path) {
                crate::ui::message_box(Some(hwnd), &format!("Could not open item: {e}"), "DesktopDrawers");
            }
}

fn item_path(st: &DrawerState, row: i32) -> Option<PathBuf> {
    let id = *st.item_ids.get(row as usize)?;
    let app = unsafe { &*st.app };
    let d = app.drawer(st.id)?;
    let item = d.items.iter().find(|i| i.id == id)?;
    Some(app.store.drawer_dir(st.id).join(&item.shortcut))
}

fn on_keydown(hwnd: HWND, wparam: WPARAM) {
    let vk = wparam.0 as u16;
    const VK_DELETE: u16 = 0x2E;
    const VK_F2: u16 = 0x71;
    const VK_RETURN: u16 = 0x0D;
    match vk {
        VK_DELETE => {
            if let Some(st) = state(hwnd)
                && let Some(row) = selected_row(st.listview) {
                    remove_row(hwnd, row);
                }
        }
        VK_F2 => {
            if let Some(st) = state(hwnd)
                && let Some(row) = selected_row(st.listview) {
                    unsafe {
                        SendMessageW(st.listview, LVM_EDITLABELW, WPARAM(row as usize), LPARAM(0));
                    }
                }
        }
        VK_RETURN => {
            if let Some(st) = state(hwnd)
                && let Some(row) = selected_row(st.listview) {
                    launch_row(hwnd, row);
                }
        }
        _ => {}
    }
}

fn remove_row(hwnd: HWND, row: i32) {
    let Some(st) = state(hwnd) else { return };
    let Some(&item_id) = st.item_ids.get(row as usize) else {
        return;
    };
    let id = st.id;
    unsafe {
        if let Some(d) = (*st.app).drawer_mut(id) {
            if let Some(removed) = d.remove_item(item_id) {
                // Delete only the managed .lnk file — never the target.
                let full = (*st.app).store.drawer_dir(id).join(&removed.shortcut);
                let _ = std::fs::remove_file(full);
            }
            let _ = (*st.app).save_drawer(id);
        }
    }
    populate(st);
}

fn commit_rename(hwnd: HWND, lparam: LPARAM) {
    let info = unsafe { &*(lparam.0 as *const NMLVDISPINFOW) };
    if info.item.pszText.is_null() {
        return;
    }
    let text = unsafe { pwstr_to_string(info.item.pszText) };
    let text = text.trim().to_string();
    if text.is_empty() {
        return;
    }
    let Some(st) = state(hwnd) else { return };
    let Some(&item_id) = st.item_ids.get(info.item.iItem as usize) else {
        return;
    };
    let id = st.id;
    unsafe {
        if let Some(d) = (*st.app).drawer_mut(id) {
            if let Some(it) = d.items.iter_mut().find(|i| i.id == item_id) {
                it.display_name = text;
            }
            let _ = (*st.app).save_drawer(id);
        }
    }
    populate(st);
}

// ---- context menus ------------------------------------------------------

fn item_context_menu(hwnd: HWND, row: i32) {
    let Some(st) = state(hwnd) else { return };
    let app = st.app;
    let Some(&item_id) = st.item_ids.get(row as usize) else {
        return;
    };
    let mut pt = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut pt);
    }
    let menu = unsafe { CreatePopupMenu() }.unwrap_or_default();
    unsafe {
        append(menu, IDM_ITEM_OPEN, "Open");
        append(menu, IDM_ITEM_OPEN_LOCATION, "Open File Location");
        append_sep(menu);
        append(menu, IDM_ITEM_RENAME, "Rename in Drawer");
        append(menu, IDM_ITEM_PROPERTIES, "Properties");
        append_sep(menu);
        append(menu, IDM_ITEM_REMOVE, "Remove from Drawer");
    }
    let choice = unsafe {
        TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON, pt.x, pt.y, 0, hwnd, None)
    };
    unsafe {
        let _ = DestroyMenu(menu);
    }
    let path = item_path(unsafe { &*(st as *const DrawerState) }, row);
    match choice.0 as usize {
        IDM_ITEM_OPEN => launch_row(hwnd, row),
        IDM_ITEM_OPEN_LOCATION => {
            if let Some(p) = path.as_ref().and_then(|p| crate::shortcut::resolve_target(p))
                && let Some(parent) = p.parent() {
                    let _ = crate::shortcut::launch(parent);
                }
        }
        IDM_ITEM_RENAME => unsafe {
            SendMessageW(st.listview, LVM_EDITLABELW, WPARAM(row as usize), LPARAM(0));
        },
        IDM_ITEM_PROPERTIES => {
            if let Some(p) = path {
                let _ = crate::shortcut::launch_verb(&p, "properties");
            }
        }
        IDM_ITEM_REMOVE => remove_row(hwnd, row),
        _ => {}
    }
    let _ = (app, item_id);
}

fn empty_space_menu(hwnd: HWND, lparam: LPARAM) {
    let Some(st) = state(hwnd) else { return };
    let app = st.app;
    let id = st.id;
    let locked = unsafe { (*app).drawer(id).map(|d| d.layout_locked).unwrap_or(false) };

    let (x, y) = if lparam.0 == -1 || lparam.0 == 0 {
        let mut pt = POINT::default();
        unsafe {
            let _ = GetCursorPos(&mut pt);
        }
        (pt.x, pt.y)
    } else {
        (crate::win32::get_x_lparam(lparam), crate::win32::get_y_lparam(lparam))
    };

    let menu = unsafe { CreatePopupMenu() }.unwrap_or_default();
    unsafe {
        append(menu, IDM_ADD_SHORTCUT, "Add Shortcut…");
        append_sep(menu);
        append_checked(menu, IDM_LOCK_LAYOUT, "Lock Layout", locked);
        // Arrange submenu, flattened for simplicity.
        append(menu, IDM_ARRANGE_LTR, "Fill Left to Right");
        append(menu, IDM_ARRANGE_TTB, "Fill Top to Bottom");
        append(menu, IDM_ARRANGE_REMOVE_GAPS, "Remove Empty Spaces");
        append(menu, IDM_ARRANGE_SORT_NAME, "Sort by Name");
        append_sep(menu);
        append(menu, IDM_DRAWER_SETTINGS, "Drawer Settings…");
        append(menu, IDM_DRAWER_CREATE_DESKTOP_SHORTCUT, "Create Desktop Shortcut");
        append_sep(menu);
        append(menu, IDM_DRAWER_NEW, "New Drawer…");
        append(menu, IDM_DRAWER_OPEN_MANAGER, "Open Manager");
        append(menu, IDM_DRAWER_CLOSE, "Close Drawer");
    }
    let choice = unsafe {
        TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON, x, y, 0, hwnd, None)
    };
    unsafe {
        let _ = DestroyMenu(menu);
    }

    match choice.0 as usize {
        IDM_ADD_SHORTCUT => add_shortcut(hwnd),
        IDM_LOCK_LAYOUT => unsafe {
            if let Some(d) = (*app).drawer_mut(id) {
                d.layout_locked = !d.layout_locked;
                let _ = (*app).save_drawer(id);
            }
        },
        IDM_ARRANGE_LTR => arrange(hwnd, |d| d.fill_left_to_right()),
        IDM_ARRANGE_TTB => arrange(hwnd, |d| d.fill_top_to_bottom()),
        IDM_ARRANGE_REMOVE_GAPS => arrange(hwnd, |d| d.remove_empty_spaces()),
        IDM_ARRANGE_SORT_NAME => arrange(hwnd, |d| d.sort_by_name()),
        IDM_DRAWER_SETTINGS => {
            crate::ui::drawer_settings::show(hwnd, app, id);
            if let Some(st) = state(hwnd) {
                populate(st);
            }
        }
        IDM_DRAWER_CREATE_DESKTOP_SHORTCUT => unsafe {
            let name = (*app).drawer(id).map(|d| d.name.clone()).unwrap_or_default();
            match crate::shortcut::create_desktop_shortcut_for_drawer(&id.to_string(), &name) {
                Ok(_) => crate::ui::message_box(Some(hwnd), "Desktop shortcut created.", "DesktopDrawers"),
                Err(e) => crate::ui::message_box(Some(hwnd), &format!("{e}"), "DesktopDrawers"),
            }
        },
        IDM_DRAWER_NEW => {
            if let Some(params) = crate::ui::create_drawer::show(hwnd) {
                unsafe {
                    let d = crate::model::Drawer::new(params.name, params.columns, params.rows, params.icon_size);
                    let new_id = d.id;
                    if (*app).add_drawer(d).is_ok() {
                        let _ = (*app).open_drawer(new_id);
                    }
                }
            }
        }
        IDM_DRAWER_OPEN_MANAGER => unsafe {
            let _ = (*app).open_manager();
        },
        IDM_DRAWER_CLOSE => unsafe {
            let _ = DestroyWindow(hwnd);
        },
        _ => {}
    }
}

fn arrange(hwnd: HWND, f: impl FnOnce(&mut crate::model::Drawer)) {
    let Some(st) = state(hwnd) else { return };
    let id = st.id;
    unsafe {
        if let Some(d) = (*st.app).drawer_mut(id) {
            f(d);
            let _ = (*st.app).save_drawer(id);
        }
    }
    populate(st);
}

/// Add a shortcut via the file picker (drag-drop from Explorer arrives here too
/// once the drop target is wired up).
fn add_shortcut(hwnd: HWND) {
    let Some(path) = pick_file(hwnd) else { return };
    add_path_to_drawer(hwnd, &path);
}

/// Copy/create a managed `.lnk` for `path` and add it to the drawer.
pub fn add_path_to_drawer(hwnd: HWND, path: &std::path::Path) {
    let Some(st) = state(hwnd) else { return };
    let id = st.id;
    let items_dir = unsafe { (*st.app).store.drawer_items_dir(id) };
    if std::fs::create_dir_all(&items_dir).is_err() {
        return;
    }

    let item_id = Uuid::new_v4();
    let lnk_name = format!("{item_id}.lnk");
    let dest = items_dir.join(&lnk_name);
    let rel = format!("Items/{lnk_name}");

    let display = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "Shortcut".into());

    let is_lnk = path.extension().and_then(|s| s.to_str()).map(|e| e.eq_ignore_ascii_case("lnk")).unwrap_or(false);

    let ok = if is_lnk {
        std::fs::copy(path, &dest).is_ok()
    } else {
        let workdir = path.parent();
        crate::shortcut::create_lnk(&dest, path, None, workdir, Some(&display), None).is_ok()
    };
    if !ok {
        crate::ui::message_box(Some(hwnd), "Could not add that shortcut.", "DesktopDrawers");
        return;
    }

    unsafe {
        if let Some(d) = (*st.app).drawer_mut(id) {
            if d.add_item(rel, display, item_id).is_err() {
                crate::ui::message_box(Some(hwnd), "This drawer is full.", "DesktopDrawers");
                let _ = std::fs::remove_file(&dest);
                return;
            }
            let _ = (*st.app).save_drawer(id);
        }
    }
    populate(st);
}

fn pick_file(hwnd: HWND) -> Option<PathBuf> {
    use windows::Win32::UI::Controls::Dialogs::{
        GetOpenFileNameW, OFN_FILEMUSTEXIST, OFN_HIDEREADONLY, OPENFILENAMEW,
    };
    let mut buf = [0u16; 1024];
    let filter: Vec<u16> = "All Files\0*.*\0Shortcuts\0*.lnk;*.url\0Programs\0*.exe\0\0"
        .encode_utf16()
        .collect();
    let mut ofn = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: hwnd,
        lpstrFilter: PCWSTR(filter.as_ptr()),
        lpstrFile: windows::core::PWSTR(buf.as_mut_ptr()),
        nMaxFile: buf.len() as u32,
        Flags: OFN_FILEMUSTEXIST | OFN_HIDEREADONLY,
        ..Default::default()
    };
    let ok = unsafe { GetOpenFileNameW(&mut ofn).as_bool() };
    if ok {
        Some(PathBuf::from(from_wide(&buf)))
    } else {
        None
    }
}

// ---- helpers ------------------------------------------------------------

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

fn set_font(hwnd: HWND) {
    let font = unsafe { GetStockObject(DEFAULT_GUI_FONT) };
    unsafe {
        SendMessageW(hwnd, WM_SETFONT, WPARAM(font.0 as usize), LPARAM(1));
    }
}

unsafe fn append(menu: HMENU, id: usize, text: &str) {
    let t = wide(text);
    unsafe {
        let _ = AppendMenuW(menu, MF_STRING, id, PCWSTR(t.as_ptr()));
    }
}
unsafe fn append_checked(menu: HMENU, id: usize, text: &str, checked: bool) {
    let t = wide(text);
    let flags = if checked { MF_STRING | MF_CHECKED } else { MF_STRING };
    unsafe {
        let _ = AppendMenuW(menu, flags, id, PCWSTR(t.as_ptr()));
    }
}
unsafe fn append_sep(menu: HMENU) {
    unsafe {
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
    }
}
