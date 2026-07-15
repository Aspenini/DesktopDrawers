# DesktopDrawers

A tiny native **Win32** utility (Rust 2024) for compact, persistent "drawers"
of shortcuts on the Windows desktop. Each drawer is a small icon grid — not a
full Explorer window — and the process only exists while the manager or at least
one drawer is open. No tray icon, no startup service, no background polling.

## Build & run

Requires the MSVC toolchain (`x86_64-pc-windows-msvc`).

```sh
cargo build --release          # -> target/release/DesktopDrawers.exe (~300 KB)
cargo test                     # 23 unit + integration tests
```

```text
DesktopDrawers.exe                 open the manager
DesktopDrawers.exe --manage        open/focus the manager
DesktopDrawers.exe --open <id>     open/focus a specific drawer (desktop shortcuts use this)
```

Configuration lives under `%LOCALAPPDATA%\DesktopDrawers\`.

## Architecture

Library-backed binary (`src/lib.rs` + a tiny `src/main.rs`):

| Module | Responsibility |
|--------|----------------|
| `command` | Command-line + IPC-envelope parsing |
| `model` | Pure drawer/grid data model (no HWND/COM), grid placement, arrange/sort |
| `storage` | Directory layout, **atomic writes** with one backup, schema-version guard, backup recovery |
| `ipc` | Per-user named **mutex** (single instance) + named **pipe** command forwarding |
| `shortcut` | `.lnk` create/copy, `ShellExecuteExW` launch, desktop-shortcut creation via `IShellLinkW`/`IPersistFile`, COM apartment guard |
| `icon_cache` | Shell icon extraction (`SHGetFileInfoW`) into a per-drawer `HIMAGELIST` |
| `win32/*` | Small `unsafe` wrappers: RAII handles, DPI helpers, message/control IDs |
| `ui/manager` | Manager window (themed `ListView` report view + "New Drawer" + context actions) |
| `ui/drawer` | Drawer window (`ListView` icon grid with manual cell positioning, keyboard + context menus) |
| `ui/create_drawer`, `ui/drawer_settings` | Modal dialogs |
| `app` | Primary-instance state, window map, message loop, IPC dispatch |

Idiomatic-Rust rules are followed: `unsafe` is confined to small
modules, handles use RAII, per-window state lives in `GWLP_USERDATA`, errors are
explicit (`thiserror`) and returned as `Result`, and the message loop stays on
the main thread. The app manifest (`assets/`) opts into Common Controls v6 and
per-monitor-v2 DPI awareness.

## Status by milestone

**Implemented and verified**

- **M1 Native foundation** — Rust 2024, `#![windows_subsystem = "windows"]`, main
  message loop, manager + drawer windows, command-line parsing, process exits
  after the last window closes.
- **M2 Models & persistence** — UUID drawers, JSON config, atomic saves with
  backup + corrupt-file recovery, create/rename/duplicate/delete, grid settings,
  schema versioning.
- **M3 Shortcut grid** — native `ListView` icon grid, selection + keyboard
  (arrows/Enter/Delete/F2/Home/End), double-click launch, in-place label edit,
  manual cell positioning, layout locking, arrange/sort.
- **M4 Managed shortcuts** — dropped/added `.exe`/file/folder become managed
  `.lnk`s; dragged `.lnk`s are **copied** (targets never touched); Shell icon
  extraction + per-drawer image list; broken shortcuts fall back to a generic icon.
- **M5 One-process IPC** — per-user mutex, named-pipe forwarding, open-or-focus,
  startup-race retry, message-only IPC window.
- **M6 (partial)** — desktop-shortcut creation, `ShellExecuteExW` launch,
  Run-as-admin / Properties verbs, DPI + multi-monitor placement (`ensure_on_screen`).

**Deferred (next iteration)**

- Explorer **drag-and-drop** via `IDropTarget`. The plumbing point exists —
  `ui::drawer::add_path_to_drawer` already turns a path into a managed item — so
  wiring a registered drop target is the remaining step. For now, **right-click →
  "Add Shortcut…"** (file picker) covers adding items.
- Native Shell **`IContextMenu`** integration. A safe built-in context menu is
  used instead (Open / Open File Location / Rename / Properties / Remove, and the
  empty-space drawer menu), which is also the intended fallback.
- Rotating file logging and light/dark ListView theming polish.

## Installer

The installer is a single [Inno Setup](https://jrsoftware.org/isinfo.php) script,
`installer\DesktopDrawers.iss`. To build `setup.exe`:

1. `cargo build --release`
2. Open `installer\DesktopDrawers.iss` in the Inno Setup Compiler and press
   **Build** (or run `ISCC.exe installer\DesktopDrawers.iss`).

Output is `dist\DesktopDrawers-0.1.0-setup.exe`. It installs per-user to
`%LOCALAPPDATA%\Programs\DesktopDrawers` (no administrator prompt), adds a
Start-menu entry, an uninstaller, and an optional desktop shortcut — and never
creates a startup entry, service, scheduled task, or tray icon.

## Tests

`cargo test` runs 23 tests: command parsing, atomic writes + backup recovery,
schema-version rejection, grid placement/collision, duplication, and a full
create→save→load→duplicate→delete drawer lifecycle. The GUI itself was verified
by launching the binary (manager window shows; second launch forwards over IPC
and exits, keeping a single process).
