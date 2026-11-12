//! Layout and What's new state, in the file the egui app kept them in
//! (eframe's `app.ron` under the app name "DBM"), so an install that updates
//! from egui keeps its window size, collapsed sidebar, and What's new state.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub const SIDEBAR_COLLAPSED: &str = "dbm.sidebarCollapsed";
/// The newest build whose release highlights this install has shown.
pub const WHATS_NEW: &str = "dbm.whatsNewAcknowledgedBuild";
pub const SIDEBAR_WIDTH: &str = "dbm.sidebarWidth";
/// eframe's window geometry, kept in its own format.
const WINDOW: &str = "window";

#[derive(Default, Serialize, Deserialize)]
struct Point {
    x: f32,
    y: f32,
}

/// egui-winit's `WindowSettings`, field for field.
#[derive(Default, Serialize, Deserialize)]
struct WindowSettings {
    #[serde(default)]
    inner_position_pixels: Option<Point>,
    #[serde(default)]
    outer_position_pixels: Option<Point>,
    #[serde(default)]
    fullscreen: bool,
    #[serde(default)]
    maximized: bool,
    #[serde(default)]
    inner_size_points: Option<Point>,
}

pub struct Settings {
    path: Option<PathBuf>,
    values: HashMap<String, String>,
}

impl Settings {
    /// The real layout file, or for `--demo` the throwaway one the egui demo
    /// used, so the fixture never touches the real app directory.
    pub fn load(demo: bool) -> Self {
        let path = if demo {
            Some(std::env::temp_dir().join("dbm-demo-layout.ron"))
        } else {
            storage_dir().map(|dir| dir.join("app.ron"))
        };
        let values = path
            .as_ref()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|text| ron::from_str(&text).ok())
            .unwrap_or_default();
        Self { path, values }
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }

    pub fn set(&mut self, key: &str, value: impl Into<String>) {
        self.values.insert(key.to_owned(), value.into());
    }

    /// The saved window size in logical pixels, and whether it was maximized.
    pub fn window(&self) -> (Option<(i32, i32)>, bool) {
        let Some(settings) = self
            .get(WINDOW)
            .and_then(|text| ron::from_str::<WindowSettings>(text).ok())
        else {
            return (None, false);
        };
        let size = settings
            .inner_size_points
            .map(|size| (size.x.round() as i32, size.y.round() as i32));
        (size, settings.maximized)
    }

    pub fn set_window(&mut self, size: (i32, i32), maximized: bool) {
        let mut settings = self
            .get(WINDOW)
            .and_then(|text| ron::from_str::<WindowSettings>(text).ok())
            .unwrap_or_default();
        // A maximized window keeps the size it restores to.
        if !maximized {
            settings.inner_size_points = Some(Point {
                x: size.0 as f32,
                y: size.1 as f32,
            });
        }
        settings.maximized = maximized;
        if let Ok(text) = ron::to_string(&settings) {
            self.set(WINDOW, text);
        }
    }

    pub fn save(&self) {
        let Some(path) = &self.path else { return };
        let Ok(text) = ron::ser::to_string_pretty(&self.values, ron::ser::PrettyConfig::default())
        else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let staged = path.with_extension("ron.tmp");
        if std::fs::write(&staged, text).is_ok() {
            let _ = std::fs::rename(&staged, path);
        }
    }
}

/// eframe's `storage_dir("DBM")` on Linux.
fn storage_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
        .map(|dir| dir.join("dbm"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_and_keeps_the_egui_layout_file() {
        // What eframe 0.33 writes: a map of strings, the window as nested RON.
        let egui = r#"{
            "dbm.sidebarCollapsed": "true",
            "dbm.whatsNewAcknowledgedBuild": "2026110101",
            "egui": "(opaque)",
            "window": "(inner_position_pixels:Some((x:10.0,y:20.0)),outer_position_pixels:None,fullscreen:false,maximized:false,inner_size_points:Some((x:1100.0,y:700.0)))",
        }"#;
        let mut settings = Settings {
            path: None,
            values: ron::from_str(egui).unwrap(),
        };
        assert_eq!(settings.get(SIDEBAR_COLLAPSED), Some("true"));
        assert_eq!(settings.get(WHATS_NEW), Some("2026110101"));
        assert_eq!(settings.window(), (Some((1100, 700)), false));
        settings.set_window((1300, 820), false);
        assert_eq!(settings.window(), (Some((1300, 820)), false));
        settings.set_window((1920, 1080), true);
        assert_eq!(settings.window(), (Some((1300, 820)), true));
        assert_eq!(settings.get("egui"), Some("(opaque)"));
        let window: WindowSettings = ron::from_str(settings.get(WINDOW).unwrap()).unwrap();
        assert_eq!(
            window.inner_position_pixels.map(|p| (p.x, p.y)),
            Some((10.0, 20.0))
        );
    }
}
