//! Single-instance coordination: a per-user named mutex decides who the primary
//! process is, and a named pipe forwards commands from secondary launches to it.

use std::thread;

use windows::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, ERROR_PIPE_CONNECTED, GENERIC_WRITE, HANDLE,
    HWND,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, ReadFile, WriteFile, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_NONE, OPEN_EXISTING,
    PIPE_ACCESS_INBOUND,
};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_MESSAGE,
    PIPE_TYPE_MESSAGE, PIPE_WAIT,
};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;
use windows::core::PCWSTR;

use crate::command::Command;
use crate::error::{Error, Result};
use crate::win32::messages::WM_APP_IPC_COMMAND;
use crate::win32::wide;

/// A per-user suffix so instances of different users don't collide. Uses the
/// username; the `Local\` namespace already scopes to the session.
fn user_suffix() -> String {
    std::env::var("USERNAME").unwrap_or_else(|_| "default".into())
}

fn mutex_name() -> String {
    format!("Local\\DesktopDrawers.Instance.{}", user_suffix())
}

fn pipe_name() -> String {
    format!(r"\\.\pipe\DesktopDrawers.{}", user_suffix())
}

/// Holds the primary-instance mutex for the process lifetime. Dropping it
/// releases the single-instance claim.
pub struct PrimaryGuard {
    handle: HANDLE,
}

impl Drop for PrimaryGuard {
    fn drop(&mut self) {
        if !self.handle.is_invalid() {
            unsafe {
                let _ = CloseHandle(self.handle);
            }
        }
    }
}

/// Result of attempting to claim primary-instance status.
pub enum Instance {
    /// We are the first/primary process; hold onto the guard.
    Primary(PrimaryGuard),
    /// Another primary already exists.
    Secondary,
}

/// Try to become the primary instance by creating the named mutex.
pub fn acquire() -> Result<Instance> {
    let name = wide(&mutex_name());
    let handle = unsafe { CreateMutexW(None, true, PCWSTR(name.as_ptr()))? };
    let already = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    if already {
        unsafe {
            let _ = CloseHandle(handle);
        }
        Ok(Instance::Secondary)
    } else {
        Ok(Instance::Primary(PrimaryGuard { handle }))
    }
}

/// Secondary path: connect to the primary's pipe and forward a command.
pub fn send_command(cmd: &Command) -> Result<()> {
    let name = wide(&pipe_name());
    let handle = unsafe {
        CreateFileW(
            PCWSTR(name.as_ptr()),
            GENERIC_WRITE.0,
            FILE_SHARE_NONE,
            None,
            OPEN_EXISTING,
            FILE_FLAGS_AND_ATTRIBUTES(0),
            None,
        )?
    };
    let payload = cmd.to_ipc_json();
    let bytes = payload.as_bytes();
    let mut written = 0u32;
    let write_res = unsafe { WriteFile(handle, Some(bytes), Some(&mut written), None) };
    unsafe {
        let _ = CloseHandle(handle);
    }
    write_res?;
    Ok(())
}

/// Start the pipe server on a background thread. Received commands are posted to
/// `target` via [`WM_APP_IPC_COMMAND`] with a `Box<Command>` raw pointer in
/// `wparam`.
pub fn start_server(target: HWND) -> Result<()> {
    // HWND isn't Send; move the raw pointer value instead.
    let target_raw = target.0 as isize;
    thread::Builder::new()
        .name("dd-ipc".into())
        .spawn(move || {
            let target = HWND(target_raw as *mut _);
            server_loop(target);
        })
        .map_err(|e| Error::Other(format!("failed to spawn IPC thread: {e}")))?;
    Ok(())
}

fn server_loop(target: HWND) {
    let name = wide(&pipe_name());
    loop {
        let pipe = unsafe {
            CreateNamedPipeW(
                PCWSTR(name.as_ptr()),
                PIPE_ACCESS_INBOUND,
                PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_WAIT,
                4,       // max instances
                0,       // out buffer
                4096,    // in buffer
                0,       // default timeout
                None,    // default security: current user + admins
            )
        };
        if pipe.is_invalid() {
            // Can't create the pipe; give up the server (secondaries just won't
            // be forwarded). Avoid a busy loop.
            break;
        }

        let connected = unsafe { ConnectNamedPipe(pipe, None) };
        // ConnectNamedPipe returns Err with ERROR_PIPE_CONNECTED if a client
        // connected between create and connect — that's still success.
        let ok = connected.is_ok() || unsafe { GetLastError() } == ERROR_PIPE_CONNECTED;
        if ok {
            read_and_dispatch(pipe, target);
        }

        unsafe {
            let _ = DisconnectNamedPipe(pipe);
            let _ = CloseHandle(pipe);
        }
    }
}

fn read_and_dispatch(pipe: HANDLE, target: HWND) {
    let mut buf = [0u8; 4096];
    let mut read = 0u32;
    let res = unsafe { ReadFile(pipe, Some(&mut buf), Some(&mut read), None) };
    if res.is_err() || read == 0 {
        return;
    }
    let text = String::from_utf8_lossy(&buf[..read as usize]);
    if let Ok(cmd) = Command::from_ipc_json(&text) {
        // Hand ownership of the command to the UI thread.
        let boxed = Box::new(cmd);
        let ptr = Box::into_raw(boxed) as isize;
        let posted = unsafe {
            PostMessageW(
                target,
                WM_APP_IPC_COMMAND,
                windows::Win32::Foundation::WPARAM(ptr as usize),
                windows::Win32::Foundation::LPARAM(0),
            )
        };
        if posted.is_err() {
            // Reclaim the box so we don't leak if posting failed.
            drop(unsafe { Box::from_raw(ptr as *mut Command) });
        }
    }
}
