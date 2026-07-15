//! "Drawer Settings / Edit Layout" modal dialog: edit columns, rows, icon size,
//! and label visibility for an existing drawer.
//!
//! Changes to row/column counts take full effect the next time the drawer is
//! opened (the window re-sizes to the new grid); item positions reflow
//! immediately.

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{GetStockObject, DEFAULT_GUI_FONT};
use windows::Win32::UI::Controls::BST_CHECKED;
use windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::PCWSTR;

use uuid::Uuid;

use crate::app::App;
use crate::model::IconSize;
use crate::win32::messages::*;
use crate::win32::{from_wide, get_userdata, instance, set_userdata, wide};

struct SettingsState {
    app: *mut App,
    id: Uuid,
    done: bool,
}

/// Show the settings dialog modally.
// `st.done` is flipped by `finish` via the window procedure (reached through
// DispatchMessageW), which clippy cannot see from the loop body.
#[allow(clippy::while_immutable_condition)]
pub fn show(parent: HWND, app: *mut App, id: Uuid) {
    let mut st = SettingsState { app, id, done: false };

    let class = wide(crate::ui::SETTINGS_CLASS);
    let title = wide("Drawer Settings");
    let style = WS_POPUP | WS_CAPTION | WS_SYSMENU | WS_VISIBLE;

    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_DLGMODALFRAME,
            PCWSTR(class.as_ptr()),
            PCWSTR(title.as_ptr()),
            style,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            320,
            250,
            parent,
            HMENU::default(),
            instance(),
            Some(&mut st as *mut SettingsState as *const _),
        )
    };
    let Ok(hwnd) = hwnd else { return };

    center_over(hwnd, parent);
    unsafe {
        let _ = EnableWindow(parent, false);
        let _ = ShowWindow(hwnd, SW_SHOW);
    }

    let mut msg = MSG::default();
    unsafe {
        while !st.done {
            let r = GetMessageW(&mut msg, HWND::default(), 0, 0);
            if r.0 <= 0 {
                if r.0 == 0 {
                    PostQuitMessage(msg.wParam.0 as i32);
                }
                break;
            }
            if !IsDialogMessageW(hwnd, &msg).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        let _ = EnableWindow(parent, true);
        let _ = SetForegroundWindow(parent);
        let _ = DestroyWindow(hwnd);
    }
}

fn state<'a>(hwnd: HWND) -> Option<&'a mut SettingsState> {
    let ptr = unsafe { get_userdata::<SettingsState>(hwnd) };
    if ptr.is_null() { None } else { Some(unsafe { &mut *ptr }) }
}

pub extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_NCCREATE => {
                let cs = lparam.0 as *const CREATESTRUCTW;
                set_userdata(hwnd, (*cs).lpCreateParams as *mut SettingsState);
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
            WM_CREATE => {
                if let Some(st) = state(hwnd) {
                    build_controls(hwnd, st.app, st.id);
                }
                LRESULT(0)
            }
            WM_COMMAND => {
                let id = (wparam.0 & 0xFFFF) as i32;
                match id {
                    IDC_CREATE_BTN => finish(hwnd, true),
                    IDC_CANCEL_BTN => finish(hwnd, false),
                    _ => {}
                }
                LRESULT(0)
            }
            WM_CLOSE => {
                finish(hwnd, false);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

fn build_controls(hwnd: HWND, app: *mut App, id: Uuid) {
    let (cols, rows, icon_px, show_labels) = unsafe {
        (*app)
            .drawer(id)
            .map(|d| (d.columns, d.rows, d.icon_size, d.show_labels))
            .unwrap_or((4, 3, 48, true))
    };

    let font = unsafe { GetStockObject(DEFAULT_GUI_FONT) };
    let mk = |class: &str, text: &str, style: WINDOW_STYLE, x: i32, y: i32, w: i32, h: i32, cid: i32| -> HWND {
        let c = wide(class);
        let t = wide(text);
        let ctl = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                PCWSTR(c.as_ptr()),
                PCWSTR(t.as_ptr()),
                WS_CHILD | WS_VISIBLE | style,
                x,
                y,
                w,
                h,
                hwnd,
                HMENU(cid as isize as *mut _),
                instance(),
                None,
            )
        }
        .unwrap_or_default();
        unsafe {
            SendMessageW(ctl, WM_SETFONT, WPARAM(font.0 as usize), LPARAM(1));
        }
        ctl
    };

    let edit_style = WINDOW_STYLE(ES_AUTOHSCROLL as u32) | WS_BORDER | WS_TABSTOP;
    mk("STATIC", "Columns:", WINDOW_STYLE(0), 16, 18, 90, 20, -1);
    mk("EDIT", &cols.to_string(), edit_style, 110, 16, 60, 22, IDC_COLS_EDIT);
    mk("STATIC", "Rows:", WINDOW_STYLE(0), 16, 50, 90, 20, -1);
    mk("EDIT", &rows.to_string(), edit_style, 110, 48, 60, 22, IDC_ROWS_EDIT);

    mk("STATIC", "Icon size:", WINDOW_STYLE(0), 16, 82, 90, 20, -1);
    let combo = mk(
        "COMBOBOX",
        "",
        WINDOW_STYLE(CBS_DROPDOWNLIST as u32) | WS_TABSTOP | WS_VSCROLL,
        110,
        80,
        120,
        140,
        IDC_SIZE_COMBO,
    );
    for size in IconSize::ALL {
        let s = wide(size.label());
        unsafe {
            SendMessageW(combo, CB_ADDSTRING, WPARAM(0), LPARAM(s.as_ptr() as isize));
        }
    }
    let sel = IconSize::ALL
        .iter()
        .position(|s| s.px() == IconSize::from_px(icon_px).px())
        .unwrap_or(1);
    unsafe {
        SendMessageW(combo, CB_SETCURSEL, WPARAM(sel), LPARAM(0));
    }

    let labels = mk(
        "BUTTON",
        "Show labels",
        WINDOW_STYLE(BS_AUTOCHECKBOX as u32) | WS_TABSTOP,
        16,
        114,
        200,
        22,
        IDC_DESKTOP_CHECK,
    );
    if show_labels {
        unsafe {
            SendMessageW(labels, BM_SETCHECK, WPARAM(BST_CHECKED.0 as usize), LPARAM(0));
        }
    }

    mk("BUTTON", "Save", WINDOW_STYLE(BS_DEFPUSHBUTTON as u32) | WS_TABSTOP, 110, 165, 85, 28, IDC_CREATE_BTN);
    mk("BUTTON", "Cancel", WINDOW_STYLE(BS_PUSHBUTTON as u32) | WS_TABSTOP, 205, 165, 85, 28, IDC_CANCEL_BTN);
}

fn finish(hwnd: HWND, accept: bool) {
    let Some(st) = state(hwnd) else { return };
    if accept {
        apply(hwnd, st.app, st.id);
    }
    st.done = true;
}

fn apply(hwnd: HWND, app: *mut App, id: Uuid) {
    let cols: Option<u32> = get_text(hwnd, IDC_COLS_EDIT).trim().parse().ok().filter(|&c| (1..=16).contains(&c));
    let rows: Option<u32> = get_text(hwnd, IDC_ROWS_EDIT).trim().parse().ok().filter(|&r| (1..=16).contains(&r));

    let combo = unsafe { GetDlgItem(hwnd, IDC_SIZE_COMBO) }.unwrap_or_default();
    let sel = unsafe { SendMessageW(combo, CB_GETCURSEL, WPARAM(0), LPARAM(0)).0 };
    let icon = IconSize::ALL.get(sel.max(0) as usize).copied().unwrap_or(IconSize::Medium);

    let check = unsafe { GetDlgItem(hwnd, IDC_DESKTOP_CHECK) }.unwrap_or_default();
    let show_labels = unsafe { SendMessageW(check, BM_GETCHECK, WPARAM(0), LPARAM(0)).0 } == BST_CHECKED.0 as isize;

    unsafe {
        if let Some(d) = (*app).drawer_mut(id) {
            if let Some(c) = cols {
                d.columns = c;
            }
            if let Some(r) = rows {
                d.rows = r;
            }
            d.icon_size = icon.px();
            d.show_labels = show_labels;
            // Clamp any items that now fall outside the grid.
            let (mc, mr) = (d.columns, d.rows);
            for it in d.items.iter_mut() {
                if it.column >= mc {
                    it.column = mc - 1;
                }
                if it.row >= mr {
                    it.row = mr - 1;
                }
            }
            let _ = (*app).save_drawer(id);
        }
    }
}

fn get_text(hwnd: HWND, id: i32) -> String {
    let ctl = unsafe { GetDlgItem(hwnd, id) }.unwrap_or_default();
    let mut buf = [0u16; 128];
    let len = unsafe { GetWindowTextW(ctl, &mut buf) };
    from_wide(&buf[..len as usize])
}

fn center_over(hwnd: HWND, parent: HWND) {
    unsafe {
        let mut pr = RECT::default();
        let mut wr = RECT::default();
        if GetWindowRect(parent, &mut pr).is_err() || GetWindowRect(hwnd, &mut wr).is_err() {
            return;
        }
        let w = wr.right - wr.left;
        let h = wr.bottom - wr.top;
        let x = pr.left + ((pr.right - pr.left) - w) / 2;
        let y = pr.top + ((pr.bottom - pr.top) - h) / 2;
        let _ = SetWindowPos(hwnd, HWND::default(), x.max(0), y.max(0), 0, 0, SWP_NOSIZE | SWP_NOZORDER);
    }
}
