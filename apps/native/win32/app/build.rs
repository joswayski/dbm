//! Embeds the app icon (resource 1) and the Anybase name in the executable,
//! so Explorer, Task Manager, shortcuts, the taskbar and the window show them.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../icons/icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon_with_id("../../icons/icon.ico", "1");
        // Task Manager and Explorer show these, not the window title; Cargo's
        // defaults would show the crate name.
        resource.set("FileDescription", "Anybase");
        resource.set("ProductName", "Anybase");
        resource.set("InternalName", "Anybase");
        resource.set("OriginalFilename", "dbm.exe");
        if let Err(error) = resource.compile() {
            panic!("embed the Windows icon resource: {error}");
        }
    }
}
