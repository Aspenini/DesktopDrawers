//! Custom window messages and control identifiers.

use windows::Win32::UI::WindowsAndMessaging::WM_APP;

/// Posted (from the IPC thread) to the hidden message window to deliver a
/// forwarded [`crate::command::Command`]. `wparam` carries a `Box<Command>`
/// raw pointer; the handler takes ownership.
pub const WM_APP_IPC_COMMAND: u32 = WM_APP + 1;

/// Posted (from the icon worker thread) when an extracted icon is ready.
pub const WM_APP_ICON_READY: u32 = WM_APP + 2;

// ---- Menu / control command IDs ----------------------------------------

pub const ID_LISTVIEW: i32 = 1001;

// Manager drawer context menu.
pub const IDM_OPEN: usize = 2001;
pub const IDM_RENAME: usize = 2002;
pub const IDM_EDIT_LAYOUT: usize = 2003;
pub const IDM_CREATE_DESKTOP_SHORTCUT: usize = 2004;
pub const IDM_DUPLICATE: usize = 2005;
pub const IDM_OPEN_DATA_LOCATION: usize = 2006;
pub const IDM_DELETE: usize = 2007;
pub const IDM_NEW_DRAWER: usize = 2008;

// Drawer item context menu.
pub const IDM_ITEM_OPEN: usize = 3001;
pub const IDM_ITEM_OPEN_LOCATION: usize = 3002;
pub const IDM_ITEM_RENAME: usize = 3003;
pub const IDM_ITEM_REMOVE: usize = 3004;
pub const IDM_ITEM_PROPERTIES: usize = 3005;

// Drawer empty-space context menu.
pub const IDM_ADD_SHORTCUT: usize = 3101;
pub const IDM_LOCK_LAYOUT: usize = 3102;
pub const IDM_ARRANGE_LTR: usize = 3103;
pub const IDM_ARRANGE_TTB: usize = 3104;
pub const IDM_ARRANGE_REMOVE_GAPS: usize = 3105;
pub const IDM_ARRANGE_SORT_NAME: usize = 3106;
pub const IDM_DRAWER_SETTINGS: usize = 3107;
pub const IDM_DRAWER_CREATE_DESKTOP_SHORTCUT: usize = 3108;
pub const IDM_DRAWER_NEW: usize = 3109;
pub const IDM_DRAWER_OPEN_MANAGER: usize = 3110;
pub const IDM_DRAWER_CLOSE: usize = 3111;

// Create-drawer dialog controls.
pub const IDC_NAME_EDIT: i32 = 4001;
pub const IDC_COLS_EDIT: i32 = 4002;
pub const IDC_ROWS_EDIT: i32 = 4003;
pub const IDC_SIZE_COMBO: i32 = 4004;
pub const IDC_DESKTOP_CHECK: i32 = 4005;
pub const IDC_CREATE_BTN: i32 = 4006;
pub const IDC_CANCEL_BTN: i32 = 4007;
