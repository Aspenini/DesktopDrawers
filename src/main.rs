// Release builds are GUI apps: no console window should appear.
#![windows_subsystem = "windows"]

fn main() {
    if let Err(error) = desktopdrawers::run() {
        desktopdrawers::report_fatal_error(&error);
    }
}
