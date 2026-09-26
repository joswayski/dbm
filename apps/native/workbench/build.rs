//! Embeds the desktop app's icon in the Windows executable, so Explorer,
//! shortcuts, and the taskbar show DBM rather than a generic program icon.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../desktop/src-tauri/icons/icon.ico");
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("../../desktop/src-tauri/icons/icon.ico");
        resource.compile().expect("embed the Windows icon resource");
    }
}
