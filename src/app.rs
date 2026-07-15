//! Primary-instance application state and the Win32 message loop.
//!
//! Everything here runs on the single UI thread. Window procedures recover a
//! `&mut App` through a raw pointer stored in each window's `GWLP_USERDATA`;
//! this is sound because there is exactly one thread and the `App` outlives all
//! windows (it lives for the duration of [`run`]).

use std::collections::HashMap;
use std::path::PathBuf;

use uuid::Uuid;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Controls::{
    InitCommonControlsEx, ICC_LISTVIEW_CLASSES, ICC_STANDARD_CLASSES, INITCOMMONCONTROLSEX,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DefWindowProcW, DispatchMessageW, GetMessageW, PostQuitMessage,
    ShowWindow, TranslateMessage, CreateWindowExW, DestroyWindow, SetForegroundWindow,
    IsIconic, ShowWindowAsync, HMENU, HWND_MESSAGE, MSG, SW_RESTORE, SW_SHOW, WINDOW_EX_STYLE,
    WINDOW_STYLE, WS_OVERLAPPED,
};
use windows::core::PCWSTR;

use crate::command::Command;
use crate::error::{Error, Result};
use crate::ipc::{self, PrimaryGuard};
use crate::model::{Drawer, DrawerIndex};
use crate::shortcut::ComApartment;
use crate::storage::Store;
use crate::ui;
use crate::win32::messages::WM_APP_IPC_COMMAND;
use crate::win32::{get_userdata, set_userdata, wide};

/// Primary-instance application. Owns configuration and tracks open windows.
pub struct App {
    pub store: Store,
    pub index: DrawerIndex,
    pub drawers: Vec<Drawer>,
    pub manager: Option<HWND>,
    pub open_drawers: HashMap<Uuid, HWND>,
    pub message_window: HWND,
    // Kept alive for the process lifetime; dropped on exit.
    _com: ComApartment,
    _primary: PrimaryGuard,
}

impl App {
    fn new(store: Store, com: ComApartment, primary: PrimaryGuard) -> Result<App> {
        let (index, drawers) = store.load_all()?;
        Ok(App {
            store,
            index,
            drawers,
            manager: None,
            open_drawers: HashMap::new(),
            message_window: HWND::default(),
            _com: com,
            _primary: primary,
        })
    }

    // ---- Drawer lookup --------------------------------------------------

    pub fn drawer(&self, id: Uuid) -> Option<&Drawer> {
        self.drawers.iter().find(|d| d.id == id)
    }
    pub fn drawer_mut(&mut self, id: Uuid) -> Option<&mut Drawer> {
        self.drawers.iter_mut().find(|d| d.id == id)
    }

    /// Persist a drawer and its index position.
    pub fn save_drawer(&mut self, id: Uuid) -> Result<()> {
        if let Some(d) = self.drawer(id) {
            self.store.save_drawer(d)?;
        }
        Ok(())
    }

    pub fn save_index(&self) -> Result<()> {
        self.store.save_index(&self.index)
    }

    /// Insert a freshly created drawer, persist it, and update the index.
    pub fn add_drawer(&mut self, drawer: Drawer) -> Result<Uuid> {
        let id = drawer.id;
        self.store.save_drawer(&drawer)?;
        self.drawers.push(drawer);
        self.index.drawer_order.push(id);
        self.save_index()?;
        Ok(id)
    }

    /// Delete a drawer entirely (config + managed `.lnk` files only).
    pub fn delete_drawer(&mut self, id: Uuid) -> Result<()> {
        if let Some(&hwnd) = self.open_drawers.get(&id) {
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
        }
        self.open_drawers.remove(&id);
        self.drawers.retain(|d| d.id != id);
        self.index.drawer_order.retain(|x| *x != id);
        self.save_index()?;
        self.store.delete_drawer_dir(id)?;
        Ok(())
    }

    // ---- Window management ----------------------------------------------

    /// Open the manager, or focus it if already open.
    pub fn open_manager(&mut self) -> Result<()> {
        if let Some(hwnd) = self.manager {
            focus_window(hwnd);
            return Ok(());
        }
        let hwnd = ui::manager::create(self as *mut App)?;
        self.manager = Some(hwnd);
        Ok(())
    }

    /// Open a drawer by id, or focus it if already open.
    pub fn open_drawer(&mut self, id: Uuid) -> Result<()> {
        if let Some(&hwnd) = self.open_drawers.get(&id) {
            focus_window(hwnd);
            return Ok(());
        }
        if self.drawer(id).is_none() {
            return Err(Error::DrawerNotFound(id.to_string()));
        }
        let hwnd = ui::drawer::create(self as *mut App, id)?;
        self.open_drawers.insert(id, hwnd);
        Ok(())
    }

    /// Execute a command (initial or forwarded via IPC).
    pub fn execute(&mut self, cmd: Command) {
        let result = match cmd {
            Command::Manager => self.open_manager(),
            Command::OpenDrawer(id) => match Uuid::parse_str(&id) {
                Ok(uuid) => match self.open_drawer(uuid) {
                    Ok(()) => Ok(()),
                    Err(Error::DrawerNotFound(_)) => {
                        // Missing drawer: fall back to the manager.
                        ui::message_box(
                            self.any_window(),
                            "That drawer no longer exists. Opening the manager instead.",
                            "DesktopDrawers",
                        );
                        self.open_manager()
                    }
                    Err(e) => Err(e),
                },
                Err(_) => {
                    ui::message_box(self.any_window(), "Invalid drawer id.", "DesktopDrawers");
                    self.open_manager()
                }
            },
        };
        if let Err(e) = result {
            ui::message_box(self.any_window(), &format!("{e}"), "DesktopDrawers");
        }
    }

    fn any_window(&self) -> Option<HWND> {
        self.manager
            .or_else(|| self.open_drawers.values().next().copied())
    }

    /// Called when the manager window is destroyed.
    pub fn on_manager_closed(&mut self) {
        self.manager = None;
        self.maybe_quit();
    }

    /// Called when a drawer window is destroyed.
    pub fn on_drawer_closed(&mut self, id: Uuid) {
        self.open_drawers.remove(&id);
        self.maybe_quit();
    }

    /// Quit once no visible windows remain.
    fn maybe_quit(&self) {
        if self.manager.is_none() && self.open_drawers.is_empty() {
            unsafe { PostQuitMessage(0) };
        }
    }

    pub fn data_location(&self, id: Uuid) -> PathBuf {
        self.store.drawer_dir(id)
    }
}

/// Bring a window to the foreground, restoring it if minimized.
pub fn focus_window(hwnd: HWND) {
    unsafe {
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindowAsync(hwnd, SW_RESTORE);
        } else {
            let _ = ShowWindow(hwnd, SW_SHOW);
        }
        let _ = SetForegroundWindow(hwnd);
    }
}

// ---- Entry point --------------------------------------------------------

/// Run the application. Returns once the message loop exits (or immediately for
/// a secondary instance that forwarded its command).
pub fn run() -> Result<()> {
    let cmd = Command::from_env()?;

    // Single-instance decision.
    match ipc::acquire()? {
        ipc::Instance::Secondary => {
            // Forward and exit. If the primary is mid-startup the pipe may not
            // exist yet; a brief retry covers the race.
            for attempt in 0..20 {
                match ipc::send_command(&cmd) {
                    Ok(()) => return Ok(()),
                    Err(_) if attempt < 19 => std::thread::sleep(std::time::Duration::from_millis(50)),
                    Err(e) => return Err(e),
                }
            }
            Ok(())
        }
        ipc::Instance::Primary(primary) => {
            let com = ComApartment::init_sta()?;
            init_common_controls();

            let store = Store::open_default()?;
            let mut app = Box::new(App::new(store, com, primary)?);

            // Register window classes.
            ui::register_all_classes()?;

            // Hidden message-only window receives forwarded IPC commands.
            let msg_hwnd = create_message_window(app.as_mut() as *mut App)?;
            app.message_window = msg_hwnd;

            // Begin listening for secondary instances.
            ipc::start_server(msg_hwnd)?;

            // Execute the initial command.
            app.execute(cmd);

            // If nothing opened (shouldn't happen), open the manager.
            if app.manager.is_none() && app.open_drawers.is_empty() {
                app.execute(Command::Manager);
            }

            pump_messages();

            // `app` (and thus COM + primary guard) drops here.
            unsafe {
                let _ = DestroyWindow(msg_hwnd);
            }
            Ok(())
        }
    }
}

fn init_common_controls() {
    let icc = INITCOMMONCONTROLSEX {
        dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
        dwICC: ICC_LISTVIEW_CLASSES | ICC_STANDARD_CLASSES,
    };
    unsafe {
        let _ = InitCommonControlsEx(&icc);
    }
}

fn pump_messages() {
    let mut msg = MSG::default();
    unsafe {
        loop {
            let ret = GetMessageW(&mut msg, HWND::default(), 0, 0);
            if ret.0 <= 0 {
                break; // 0 = WM_QUIT, -1 = error
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

// ---- Hidden message window ----------------------------------------------

fn create_message_window(app: *mut App) -> Result<HWND> {
    let class = wide(ui::MESSAGE_CLASS);
    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            PCWSTR(class.as_ptr()),
            PCWSTR::null(),
            WINDOW_STYLE(WS_OVERLAPPED.0),
            0,
            0,
            0,
            0,
            HWND_MESSAGE, // message-only window
            HMENU::default(),
            crate::win32::instance(),
            None,
        )?
    };
    unsafe {
        set_userdata(hwnd, app);
    }
    Ok(hwnd)
}

/// Window procedure for the hidden message window.
pub extern "system" fn message_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_APP_IPC_COMMAND {
        let ptr = wparam.0 as *mut Command;
        if !ptr.is_null() {
            let cmd = *unsafe { Box::from_raw(ptr) };
            let app = unsafe { get_userdata::<App>(hwnd) };
            if !app.is_null() {
                unsafe { (*app).execute(cmd) };
            }
        }
        return LRESULT(0);
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}
