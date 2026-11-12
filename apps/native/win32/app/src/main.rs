// Release builds are GUI apps: no console window behind Anybase.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
// The app only runs on Windows; elsewhere the crate builds for its tests.
#![cfg_attr(not(windows), allow(dead_code))]

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

    // Release builds have no console: startup failures and panics go to a
    // file the preview workflow prints.
    let error_log = std::env::temp_dir().join("anybase-win32-error.txt");
    let _ = std::fs::remove_file(&error_log);
    let panic_log = error_log.clone();
    std::panic::set_hook(Box::new(move |info| {
        let _ = std::fs::write(
            &panic_log,
            format!(
                "panic: {info}\n{}",
                std::backtrace::Backtrace::force_capture()
            ),
        );
    }));
    let args: Vec<std::ffi::OsString> = std::env::args_os().collect();
    let demo = args.iter().any(|a| a == "--demo");
    let snapshot_dir = args
        .iter()
        .position(|a| a == "--snapshot-dir")
        .and_then(|i| args.get(i + 1))
        .map(std::path::PathBuf::from);
    let options = Options {
        title: if demo { "Anybase — DEMO" } else { "Anybase" }.to_owned(),
        size: vec2(1280.0, 800.0),
        min_size: vec2(900.0, 600.0),
        // The demo keeps its window placement out of the real app directory.
        placement_file: if demo || snapshot_dir.is_some() {
            None
        } else {
            store::data_dir().map(|d| d.join("window.ron"))
        },
        focus: args.iter().any(|a| a == "--after-update"),
        hidden: false,
    };
    let result = run(options, move |waker| {
        let app = state::Workbench::new(waker.callback(), demo);
        let view = view::View::new(app, store::Store::open(demo));
        Box::new(Anybase {
            view,
            snapshots: snapshot_dir.map(snapshots::Walk::new),
        })
    });
    if let Err(e) = result {
        eprintln!("Anybase could not start: {e}");
        let _ = std::fs::write(&error_log, format!("Anybase could not start: {e:?}"));
        std::process::exit(1);
    }
}

/// Off Windows with the `raster` feature: `--snapshot-dir DIR` walks the
/// demo with the software renderer, to check layout without a Windows box.
#[cfg(all(not(windows), feature = "raster"))]
fn main() {
    use std::rc::Rc;

    use dbm_ui::ctx::NullPlatform;
    use dbm_ui::raster::{RasterMetrics, render};
    use dbm_ui::{Color32, Context, Fonts, Layer, RawInput, Rect, Ui, pos2, vec2};

    let args: Vec<String> = std::env::args().collect();
    let Some(dir) = args
        .iter()
        .position(|a| a == "--snapshot-dir")
        .and_then(|i| args.get(i + 1))
    else {
        eprintln!("usage: dbm-win32 --snapshot-dir DIR");
        std::process::exit(2);
    };
    let (screen, scale) = (vec2(1280.0, 800.0), 1.0);
    let app = state::Workbench::new(std::sync::Arc::new(|| {}), true);
    let mut view = view::View::new(app, store::Store::open(true));
    let mut walk = snapshots::Walk::new(dir.into());
    let mut ctx = Context::new(
        Fonts::new(Box::new(RasterMetrics)),
        Rc::new(NullPlatform::default()),
    );
    let start = std::time::Instant::now();
    while !walk.done() && start.elapsed().as_secs() < 60 {
        let mut out = None;
        for _ in 0..4 {
            ctx.begin_frame(RawInput {
                events: Vec::new(),
                screen,
                time: start.elapsed().as_secs_f64(),
                modifiers: Default::default(),
            });
            {
                let mut ui = Ui::root(
                    &mut ctx,
                    Rect::from_min_size(pos2(0.0, 0.0), screen),
                    Layer::Background,
                );
                walk.step(&mut view.app, &mut ui);
                view.update(&mut ui);
            }
            let o = ctx.end_frame();
            let rerun = o.rerun;
            out = Some(o);
            if !rerun {
                break;
            }
        }
        if let (Some(out), Some(path)) = (out, walk.take_path(&view.app)) {
            let pm = render(
                &out.list,
                screen.x as u32,
                screen.y as u32,
                scale,
                Color32::from_rgb(0x1c, 0x1c, 0x1e),
            );
            pm.save_png(&path).expect("write snapshot");
            println!("{}", path.display());
        }
        std::thread::sleep(std::time::Duration::from_millis(30));
    }
}

/// The app is Windows only; elsewhere the crate builds for its unit tests.
#[cfg(all(not(windows), not(feature = "raster")))]
fn main() {
    eprintln!("Anybase's Win32 app runs on Windows.");
}
