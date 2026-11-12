//! Embeds the app icon (resource 1) in Anybase.exe, so Explorer, shortcuts,
//! the taskbar and the window show it.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../icons/icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon_with_id("../../icons/icon.ico", "1");
        if let Err(error) = resource.compile() {
            panic!("embed the Windows icon resource: {error}");
        }
    }
}
