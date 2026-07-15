//! Build script: compiles the Win32 resource file (application manifest so we
//! get Common Controls v6 + per-monitor DPI awareness, plus the app icon when
//! one is present). Failing to embed resources must not break local builds, so
//! this is best-effort.

fn main() {
    let rc = std::path::Path::new("assets/resources.rc");
    if rc.exists() {
        embed_resource::compile(rc, embed_resource::NONE);
    }
    println!("cargo:rerun-if-changed=assets/resources.rc");
    println!("cargo:rerun-if-changed=assets/DesktopDrawers.manifest");
    println!("cargo:rerun-if-changed=assets/DesktopDrawers.ico");
}
