//! Anybase for Linux: a GTK 4 client over the shared core. The database
//! worker and self-updater come from `dbm-desktop`, as in the egui app.

mod app;
mod dialogs;
mod fonts;
mod grid;
mod icons;
mod messages;
mod query;
mod settings;
mod sidebar;
mod state;
mod table;
mod tabs;
mod ui;

use gtk::prelude::*;

fn main() -> gtk::glib::ExitCode {
    let demo = std::env::args().any(|arg| arg == "--demo");
    fonts::prepare();
    // WM_CLASS on X11 follows the program name; match the app id (and the
    // desktop entry's StartupWMClass) rather than the binary's file name,
    // which is dbm-workbench inside the AppImage.
    gtk::glib::set_prgname(Some("io.github.joswayski.dbm"));
    // NON_UNIQUE: a second launch opens its own window, as the egui app did,
    // and the self-updater can start the new build while this one exits.
    let application = gtk::Application::builder()
        .application_id("io.github.joswayski.dbm")
        .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
        .build();
    application.connect_startup(|_| {
        let css = gtk::CssProvider::new();
        css.load_from_string(include_str!("style.css"));
        if let Some(display) = gtk::gdk::Display::default() {
            gtk::style_context_add_provider_for_display(
                &display,
                &css,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
            // The window icon, for X11 docks and taskbars that read it
            // rather than the desktop entry's icon.
            if let Some(icons) = icon_directory() {
                gtk::IconTheme::for_display(&display).add_search_path(icons);
            }
            if let Some(settings) = gtk::Settings::for_display(&display).into() {
                let settings: gtk::Settings = settings;
                settings.set_gtk_application_prefer_dark_theme(true);
            }
        }
    });
    let after_update = std::env::args_os().any(|arg| arg == "--after-update");
    application.connect_activate(move |application| {
        let app = app::App::new(application, demo);
        app.present();
        if after_update {
            app.window.present();
        }
    });
    // GTK would reject our own flags (--demo, --after-update).
    application.run_with_args::<&str>(&[])
}

/// Writes the bundled app icon into a hicolor tree in the cache directory,
/// beside the bundled fonts, and returns its root. GTK builds the X11 window
/// icon only from themed sizes, so an unthemed `dbm.png` would be ignored.
fn icon_directory() -> Option<std::path::PathBuf> {
    const ICON: &[u8] = include_bytes!("../../icons/icon.png");
    let root = fonts::cache_directory()?.join("icons");
    let directory = root.join("hicolor/512x512/apps");
    let path = directory.join("dbm.png");
    if std::fs::read(&path).ok().as_deref() != Some(ICON) {
        std::fs::create_dir_all(&directory).ok()?;
        std::fs::write(&path, ICON).ok()?;
    }
    Some(root)
}
