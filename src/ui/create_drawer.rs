//! A small modal "Create Drawer" dialog implemented as a custom window with a
//! nested modal message loop (no resource-script dialog template needed).

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{GetStockObject, DEFAULT_GUI_FONT};
use windows::Win32::UI::Controls::BST_CHECKED;
use windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::PCWSTR;

use crate::model::IconSize;
use crate::win32::messages::*;
use crate::win32::{from_wide, get_userdata, instance, set_userdata, wide};

/// Parameters gathered from the dialog.
pub struct NewDrawerParams {
    pub name: String,
    pub columns: u32,
    pub rows: u32,
    pub icon_size: IconSize,
    pub create_desktop_shortcut: bool,
}

struct DialogState {
    result: Option<NewDrawerParams>,
    done: bool,
}

/// Show the dialog modally over `parent`. Returns the parameters, or `None` if
/// cancelled.
// `st.done` is flipped by `finish` via the window procedure (reached through
// DispatchMessageW), which clippy cannot see from the loop body.
#[allow(clippy::while_immutable_condition)]
pub fn show(parent: HWND) -> Option<NewDrawerParams> {
    let mut st = DialogState {
        result: None,
        done: false,
    };

    let class = wide(crate::ui::CREATE_CLASS);
    let title = wide("Create Drawer");
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
            290,
            parent,
            HMENU::default(),
            instance(),
            Some(&mut st as *mut DialogState as *const _),
        )
    }
    .ok()?;

    center_over(hwnd, parent);
    unsafe {
        let _ = EnableWindow(parent, false);
        let _ = ShowWindow(hwnd, SW_SHOW);
    }

    // Nested modal loop.
    let mut msg = MSG::default();
    unsafe {
        while !st.done {
            let r = GetMessageW(&mut msg, HWND::default(), 0, 0);
            if r.0 <= 0 {
                // WM_QUIT: re-post so the outer loop also exits.
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

    st.result
}

fn state<'a>(hwnd: HWND) -> Option<&'a mut DialogState> {
    let ptr = unsafe { get_userdata::<DialogState>(hwnd) };
    if ptr.is_null() { None } else { Some(unsafe { &mut *ptr }) }
}

pub extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_NCCREATE => {
                let cs = lparam.0 as *const CREATESTRUCTW;
                set_userdata(hwnd, (*cs).lpCreateParams as *mut DialogState);
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
            WM_CREATE => {
                build_controls(hwnd);
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

fn build_controls(hwnd: HWND) {
    let font = unsafe { GetStockObject(DEFAULT_GUI_FONT) };
    let mk = |class: &str, text: &str, style: WINDOW_STYLE, x: i32, y: i32, w: i32, h: i32, id: i32| -> HWND {
        let c = wide(class);
        let t = wide(text);
        let hwnd_ctl = unsafe {
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
                HMENU(id as isize as *mut _),
                instance(),
                None,
            )
        }
        .unwrap_or_default();
        unsafe {
            SendMessageW(hwnd_ctl, WM_SETFONT, WPARAM(font.0 as usize), LPARAM(1));
        }
        hwnd_ctl
    };

    let label = |text: &str, x: i32, y: i32| {
        mk("STATIC", text, WINDOW_STYLE(0), x, y, 90, 20, -1);
    };
    let edit_style = WINDOW_STYLE((ES_AUTOHSCROLL) as u32) | WS_BORDER | WS_TABSTOP;

    label("Name:", 16, 18);
    mk("EDIT", "New Drawer", edit_style, 110, 16, 180, 22, IDC_NAME_EDIT);

    label("Columns:", 16, 50);
    mk("EDIT", "4", edit_style, 110, 48, 60, 22, IDC_COLS_EDIT);

    label("Rows:", 16, 82);
    mk("EDIT", "3", edit_style, 110, 80, 60, 22, IDC_ROWS_EDIT);

    label("Icon size:", 16, 114);
    let combo = mk(
        "COMBOBOX",
        "",
        WINDOW_STYLE(CBS_DROPDOWNLIST as u32) | WS_TABSTOP | WS_VSCROLL,
        110,
        112,
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
    unsafe {
        SendMessageW(combo, CB_SETCURSEL, WPARAM(1), LPARAM(0)); // Medium
    }

    mk(
        "BUTTON",
        "Create desktop shortcut",
        WINDOW_STYLE(BS_AUTOCHECKBOX as u32) | WS_TABSTOP,
        16,
        150,
        260,
        22,
        IDC_DESKTOP_CHECK,
    );

    mk(
        "BUTTON",
        "Create",
        WINDOW_STYLE(BS_DEFPUSHBUTTON as u32) | WS_TABSTOP,
        110,
        200,
        85,
        28,
        IDC_CREATE_BTN,
    );
    mk(
        "BUTTON",
        "Cancel",
        WINDOW_STYLE(BS_PUSHBUTTON as u32) | WS_TABSTOP,
        205,
        200,
        85,
        28,
        IDC_CANCEL_BTN,
    );
}

fn finish(hwnd: HWND, accept: bool) {
    let Some(st) = state(hwnd) else { return };
    if accept {
        if let Some(params) = read_params(hwnd) {
            st.result = Some(params);
        } else {
            // Validation failed; keep the dialog open.
            crate::ui::message_box(Some(hwnd), "Please enter a name and valid row/column counts.", "Create Drawer");
            return;
        }
    }
    st.done = true;
}

fn read_params(hwnd: HWND) -> Option<NewDrawerParams> {
    let name = get_text(hwnd, IDC_NAME_EDIT);
    let name = name.trim().to_string();
    if name.is_empty() {
        return None;
    }
    let columns: u32 = get_text(hwnd, IDC_COLS_EDIT).trim().parse().ok().filter(|&c| (1..=16).contains(&c))?;
    let rows: u32 = get_text(hwnd, IDC_ROWS_EDIT).trim().parse().ok().filter(|&r| (1..=16).contains(&r))?;

    let combo = unsafe { GetDlgItem(hwnd, IDC_SIZE_COMBO) }.unwrap_or_default();
    let sel = unsafe { SendMessageW(combo, CB_GETCURSEL, WPARAM(0), LPARAM(0)).0 };
    let icon_size = IconSize::ALL.get(sel.max(0) as usize).copied().unwrap_or(IconSize::Medium);

    let check = unsafe { GetDlgItem(hwnd, IDC_DESKTOP_CHECK) }.unwrap_or_default();
    let checked = unsafe { SendMessageW(check, BM_GETCHECK, WPARAM(0), LPARAM(0)).0 } == BST_CHECKED.0 as isize;

    Some(NewDrawerParams {
        name,
        columns,
        rows,
        icon_size,
        create_desktop_shortcut: checked,
    })
}

fn get_text(hwnd: HWND, id: i32) -> String {
    let ctl = unsafe { GetDlgItem(hwnd, id) }.unwrap_or_default();
    let mut buf = [0u16; 512];
    let len = unsafe { GetWindowTextW(ctl, &mut buf) };
    from_wide(&buf[..len as usize])
}

fn center_over(hwnd: HWND, parent: HWND) {
    use windows::Win32::Foundation::RECT;
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
