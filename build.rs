//! Windows only: embeds the icon and the version information into the
//! .exe, so Explorer, the taskbar and the file's Properties show MimirDLP's
//! own instead of a blank program. Every other target skips it.
//!
//! `rc.exe` comes with the Windows SDK, which the MSVC toolchain needs for
//! linking anyway, so a Windows build that can link can also do this. A
//! failure is an error rather than a warning: a release must not quietly
//! ship without its icon.

fn main() {
    println!("cargo:rerun-if-changed=branding/icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut resource = winresource::WindowsResource::new();
    // Rendered from branding/logo.svg, 16 to 256 px, PNG entries.
    resource.set_icon("branding/icon.ico");
    resource.set("ProductName", "MimirDLP");
    resource.set("FileDescription", "MimirDLP");
    if let Err(e) = resource.compile() {
        panic!("could not embed the Windows icon and version: {e}");
    }
}
