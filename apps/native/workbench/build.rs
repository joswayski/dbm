//! Embeds the app icon in the Windows executable, so Explorer,
//! shortcuts, and the taskbar show Anybase rather than a generic program icon.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../icons/icon.ico");
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("../icons/icon.ico");
        resource.compile().expect("embed the Windows icon resource");
    }
}
