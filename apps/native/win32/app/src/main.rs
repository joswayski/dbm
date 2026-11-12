// Release builds are GUI apps: no console window behind Anybase.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod messages;
mod platform;
mod snapshots;
mod state;
mod store;
mod table;
mod view;

#[cfg(windows)]
fn main() {
    use dbm_ui::vec2;
    use dbm_ui::win32::{App, Options, run};

    struct Anybase {
        view: view::View,
        snapshots: Option<snapshots::Walk>,
    }

    impl App for Anybase {
        fn update(&mut self, ui: &mut dbm_ui::Ui) {
            if let Some(walk) = &mut self.snapshots {
                walk.step(&mut self.view.app, ui);
            }
            self.view.update(ui);
        }

        fn snapshot_path(&mut self) -> Option<std::path::PathBuf> {
            self.snapshots.as_mut()?.take_path(&self.view.app)
        }

        fn should_exit(&self) -> bool {
            self.snapshots.as_ref().is_some_and(snapshots::Walk::done)
        }

        fn on_exit(&mut self) {
            self.view.store.flush();
        }
    }

    let args: Vec<std::ffi::OsString> = std::env::args_os().collect();
    let demo = args.iter().any(|a| a == "--demo");
    let snapshot_dir = args.iter().position(|a| a == "--snapshot-dir").and_then(|i| args.get(i + 1)).map(std::path::PathBuf::from);
    let options = Options {
        title: if demo { "Anybase — DEMO" } else { "Anybase" }.to_owned(),
        size: vec2(1280.0, 800.0),
        min_size: vec2(900.0, 600.0),
        // The demo keeps its window placement out of the real app directory.
        placement_file: if demo || snapshot_dir.is_some() { None } else { store::data_dir().map(|d| d.join("window.ron")) },
        focus: args.iter().any(|a| a == "--after-update"),
        hidden: false,
    };
    let result = run(options, move |waker| {
        let app = state::Workbench::new(waker.callback(), demo);
        let view = view::View::new(app, store::Store::open(demo));
        Box::new(Anybase { view, snapshots: snapshot_dir.map(snapshots::Walk::new) })
    });
    if let Err(e) = result {
        eprintln!("Anybase could not start: {e}");
        std::process::exit(1);
    }
}

/// The app is Windows only; elsewhere the crate builds for its unit tests.
#[cfg(not(windows))]
fn main() {
    eprintln!("Anybase's Win32 app runs on Windows.");
}
