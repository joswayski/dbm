//! The small settings file the egui app kept through eframe: `app.ron` in
//! `%APPDATA%\DBM\data`, a RON map of string keys to string values. Anybase
//! reads and writes the same file and keys, so the sidebar state and the
//! What's new acknowledgement carry over, and leaves other keys alone.

use std::collections::BTreeMap;
use std::path::PathBuf;

pub const SIDEBAR_COLLAPSED: &str = "dbm.sidebarCollapsed";
pub const SIDEBAR_WIDTH: &str = "dbm.sidebarWidth";
/// The newest build whose release highlights this install has shown.
pub const WHATS_NEW: &str = "dbm.whatsNewAcknowledgedBuild";

pub struct Store {
    path: Option<PathBuf>,
    values: BTreeMap<String, String>,
}

impl Store {
    /// The real file, or a throwaway one for `--demo`.
    pub fn open(demo: bool) -> Self {
        let path = if demo {
            Some(std::env::temp_dir().join("dbm-demo-layout.ron"))
        } else {
            data_dir().map(|d| d.join("app.ron"))
        };
        let values = path
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .map(|text| parse(&text))
            .unwrap_or_default();
        Store { path, values }
    }

    #[cfg(test)]
    pub fn memory() -> Self {
        Store {
            path: None,
            values: BTreeMap::new(),
        }
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }

    pub fn set(&mut self, key: &str, value: impl Into<String>) {
        self.values.insert(key.to_owned(), value.into());
    }

    pub fn flush(&self) {
        let Some(path) = &self.path else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(text) = ron::to_string(&self.values) {
            let _ = std::fs::write(path, text);
        }
    }
}

fn parse(text: &str) -> BTreeMap<String, String> {
    ron::from_str(text).unwrap_or_default()
}

/// `%APPDATA%\DBM\data`, where eframe kept `app.ron` (its app id is "DBM").
pub fn data_dir() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|d| PathBuf::from(d).join("DBM").join("data"))
}

#[cfg(test)]
mod tests {
    #[test]
    fn reads_eframes_file_and_keeps_unknown_keys() {
        // As eframe writes it: a RON map with egui's own state beside ours.
        let text = r#"{"dbm.sidebarCollapsed":"true","dbm.whatsNewAcknowledgedBuild":"2026110301","egui":"(focus:None)"}"#;
        let values = super::parse(text);
        assert_eq!(values["dbm.sidebarCollapsed"], "true");
        assert_eq!(values["dbm.whatsNewAcknowledgedBuild"], "2026110301");
        let again = super::parse(&ron::to_string(&values).unwrap());
        assert_eq!(again["egui"], "(focus:None)");
    }
}
