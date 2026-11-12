//! `--snapshot-dir DIR`: walks the demo through the main screens and writes
//! a PNG of each, then quits. The preview workflow uploads them so the app
//! can be checked on a Windows runner without anyone at the machine.

use std::path::PathBuf;

use dbm_core::models::{DatabaseEngine, SchemaNode};
use dbm_desktop::worker::Command;
use dbm_ui::Ui;

use crate::state::{ProfileForm, Workbench};
use crate::table::{stage_cell, stage_delete};

/// Each step's name and what it does to the app before its snapshot.
const STEPS: &[(&str, fn(&mut Workbench))] = &[
    ("01-welcome", |_| {}),
    ("02-connected", |app| {
        if let Some(id) = app.profiles.first().map(|p| p.id) {
            app.active_profile = Some(id);
            app.send(Command::Connect(id), "Connecting", Some(id));
        }
    }),
    ("03-query-result", |app| {
        if let Some(tab) = app.active_tab {
            let sql = app
                .tabs
                .iter()
                .find(|t| t.id == tab)
                .map(|t| t.sql.clone())
                .unwrap_or_default();
            app.run_query(tab, sql, false, false);
        }
    }),
    ("04-table", |app| {
        if let Some(id) = app.active_profile {
            app.open_table(id, "public".into(), "users".into());
        }
    }),
    ("05-new-connection", |app| {
        app.profile_form = Some(ProfileForm::fresh())
    }),
    ("06-sidebar-collapsed", |app| {
        app.profile_form = None;
        app.sidebar_collapsed = true;
    }),
    ("07-row-inspector", |app| {
        app.sidebar_collapsed = false;
        if let Some(state) = app.active_tab.and_then(|t| app.tables.get_mut(&t)) {
            state.click_row(2, false, false);
            state.inspector_open = true;
        }
    }),
    ("08-staged-edits", |app| {
        if let Some(state) = app.active_tab.and_then(|t| app.tables.get_mut(&t)) {
            stage_cell(state, 1, 2, "Liam B.");
            stage_delete(state, &[4], true);
        }
    }),
    ("09-redis-connected", |app| {
        let redis = app
            .profiles
            .iter()
            .find(|p| p.engine == DatabaseEngine::Redis);
        if let Some(id) = redis.map(|p| p.id) {
            app.active_profile = Some(id);
            app.send(Command::Connect(id), "Connecting", Some(id));
        }
    }),
    ("10-redis-key", |app| {
        fn first_leaf(nodes: &[SchemaNode]) -> Option<(String, String)> {
            nodes.iter().find_map(|n| match (&n.schema, &n.table) {
                (Some(s), Some(t)) => Some((s.clone(), t.clone())),
                _ => first_leaf(&n.children),
            })
        }
        if let Some(id) = app.active_profile
            && let Some((schema, key)) = app.schemas.get(&id).and_then(|n| first_leaf(n))
        {
            app.open_table(id, schema, key);
        }
    }),
    ("11-update-confirm", |app| {
        app.confirm_update = Some(dbm_update::Available {
            build: 2026111110,
            version: "2026.11.11.10".into(),
            notes: String::new(),
            url: String::new(),
            signature: String::new(),
            highlights: Some(vec![dbm_update::Highlight {
                title: "Faster Windows app".into(),
                body: "Anybase now draws with Direct2D instead of egui.".into(),
            }]),
            released: Some("2026-11-12T03:15:58Z".into()),
            commit: Some("2937930046c2f0e986a1b71dc2faf2fbda55e4ba".into()),
            patch: None,
        });
    }),
    ("12-update-failed", |app| {
        app.confirm_update = None;
        app.updates.state = dbm_desktop::updates::State::Failed(
            "Couldn't install the update: Access is denied. (os error 5)".into(),
        );
        app.updates.failure_open = true;
    }),
];

/// Seconds a step waits after its work finishes, for fades and layout.
const SETTLE: f64 = 0.8;

pub struct Walk {
    dir: PathBuf,
    step: usize,
    entered: Option<f64>,
    idle_since: Option<f64>,
    ready: bool,
}

impl Walk {
    pub fn new(dir: PathBuf) -> Self {
        let _ = std::fs::create_dir_all(&dir);
        Walk {
            dir,
            step: 0,
            entered: None,
            idle_since: None,
            ready: false,
        }
    }

    pub fn step(&mut self, app: &mut Workbench, ui: &mut Ui) {
        let now = ui.ctx.time();
        ui.ctx.request_repaint_after(0.05);
        let Some(&(_, enter)) = STEPS.get(self.step) else {
            return;
        };
        if self.entered.is_none() {
            enter(app);
            self.entered = Some(now);
            self.idle_since = None;
            return;
        }
        let idle = app.pending_requests.is_empty() && app.queued.is_empty();
        match (idle, self.idle_since) {
            (true, None) => self.idle_since = Some(now),
            (false, _) => self.idle_since = None,
            _ => {}
        }
        self.ready = self.idle_since.is_some_and(|t| now - t >= SETTLE);
    }

    /// The PNG to write of the frame just drawn, once the step has settled.
    pub fn take_path(&mut self, _app: &Workbench) -> Option<PathBuf> {
        if !std::mem::take(&mut self.ready) {
            return None;
        }
        let (name, _) = STEPS.get(self.step)?;
        self.step += 1;
        self.entered = None;
        Some(self.dir.join(format!("{name}.png")))
    }

    pub fn done(&self) -> bool {
        self.step >= STEPS.len()
    }
}
