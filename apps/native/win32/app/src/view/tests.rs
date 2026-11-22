//! Drives the whole view on the demo fixture with key and text events, the
//! way the window feeds them, and checks what the keyboard does. No window
//! or renderer is involved, so these run on every platform.

use std::rc::Rc;
use std::sync::Arc;

use dbm_desktop::worker::Command;
use dbm_ui::ctx::NullPlatform;
use dbm_ui::input::{Event, Key, Modifiers};
use dbm_ui::text::MonoMetrics;
use dbm_ui::{Context, Fonts, Id, Layer, RawInput, Rect, Ui, pos2, vec2};

use super::View;
use crate::state::{TabKind, Workbench};
use crate::store::Store;

struct Harness {
    ctx: Context,
    view: View,
    time: f64,
}

impl Harness {
    /// The demo with its first (PostgreSQL) connection open.
    fn connected() -> Self {
        let app = Workbench::new(Arc::new(|| {}), true);
        let mut view = View::new(app, Store::open(true));
        view.app.sidebar_collapsed = false;
        let ctx = Context::new(
            Fonts::new(Box::new(MonoMetrics)),
            Rc::new(NullPlatform::default()),
        );
        let mut harness = Harness {
            ctx,
            view,
            time: 0.0,
        };
        harness.settle();
        let app = &mut harness.view.app;
        let id = app.profiles[0].id;
        app.active_profile = Some(id);
        app.send(Command::Connect(id), "Connecting", Some(id));
        harness.settle();
        harness
    }

    /// One presented frame: the events, then reruns until layout is stable.
    fn frame(&mut self, events: Vec<Event>) {
        self.time += 0.05;
        let modifiers = events
            .iter()
            .find_map(|event| match event {
                Event::Key { modifiers, .. } => Some(*modifiers),
                _ => None,
            })
            .unwrap_or_default();
        let screen = vec2(1280.0, 800.0);
        let mut events = Some(events);
        for _ in 0..4 {
            self.ctx.begin_frame(RawInput {
                events: events.take().unwrap_or_default(),
                screen,
                time: self.time,
                modifiers,
            });
            {
                let mut ui = Ui::root(
                    &mut self.ctx,
                    Rect::from_min_size(pos2(0.0, 0.0), screen),
                    Layer::Background,
                );
                self.view.update(&mut ui);
            }
            if !self.ctx.end_frame().rerun {
                break;
            }
        }
    }

    /// Frames until the worker has answered everything.
    fn settle(&mut self) {
        let start = std::time::Instant::now();
        let mut idle = 0;
        while idle < 3 {
            assert!(start.elapsed().as_secs() < 20, "the demo worker is stuck");
            self.frame(Vec::new());
            let app = &self.view.app;
            if app.pending_requests.is_empty() && app.queued.is_empty() {
                idle += 1;
            } else {
                idle = 0;
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }
    }

    fn key(&mut self, modifiers: Modifiers, key: Key) {
        self.frame(vec![Event::Key {
            key,
            pressed: true,
            modifiers,
        }]);
        self.settle();
    }

    fn text(&mut self, text: &str) {
        self.frame(vec![Event::Text(text.to_owned())]);
    }

    fn app(&mut self) -> &mut Workbench {
        &mut self.view.app
    }

    fn active_tab(&self) -> u64 {
        self.view.app.active_tab.expect("an active tab")
    }

    fn sql(&self) -> String {
        let tab = self.active_tab();
        let tab = self.view.app.tabs.iter().find(|t| t.id == tab).unwrap();
        tab.sql.clone()
    }

    fn editor(&self) -> Id {
        Id::new(("sql-editor", self.active_tab()))
    }

    /// Opens the demo's `public.users` table and waits for its page.
    fn open_users(&mut self) -> u64 {
        let profile = self.view.app.active_profile.unwrap();
        self.app()
            .open_table(profile, "public".into(), "users".into());
        self.settle();
        let tab = self.active_tab();
        assert!(self.view.app.tables[&tab].page.is_some());
        tab
    }
}

#[test]
fn connecting_lands_in_the_editor_ready_to_type_and_run() {
    let mut h = Harness::connected();
    assert_eq!(
        h.ctx.focused(),
        Some(h.editor()),
        "the editor has the caret"
    );
    h.key(Modifiers::COMMAND, Key::Enter);
    let tab = h.active_tab();
    assert!(h.view.app.query_results.contains_key(&tab));

    // A new tab takes the caret too, and typing goes into it.
    h.key(Modifiers::COMMAND, Key::T);
    assert_ne!(h.active_tab(), tab);
    assert_eq!(h.ctx.focused(), Some(h.editor()));
    let before = h.sql();
    h.text("x");
    assert_eq!(h.sql().len(), before.len() + 1);
}

#[test]
fn closing_a_tab_frees_its_editor_history() {
    use dbm_ui::text_edit::EditState;
    let mut h = Harness::connected();
    h.key(Modifiers::COMMAND, Key::T);
    let editor = h.editor();
    h.text("x");
    assert!(h.ctx.data::<EditState>(editor).is_some());
    h.key(Modifiers::COMMAND, Key::W);
    assert_eq!(h.view.app.tabs.len(), 1);
    assert!(h.ctx.data::<EditState>(editor).is_none());
}

#[test]
fn enter_and_space_type_into_the_editor() {
    let mut h = Harness::connected();
    let before = h.sql();
    // The window sends Enter as a key only, and Space as a key and as text.
    h.key(Modifiers::NONE, Key::Enter);
    h.frame(vec![
        Event::Key {
            key: Key::Space,
            pressed: true,
            modifiers: Modifiers::NONE,
        },
        Event::Text(" ".into()),
    ]);
    assert_eq!(h.sql().len(), before.len() + 2);
    assert_eq!(h.sql().matches('\n').count(), 1);
    assert_eq!(h.ctx.focused(), Some(h.editor()));
}

#[test]
fn ctrl_enter_runs_the_query_when_nothing_has_focus() {
    let mut h = Harness::connected();
    let editor = h.editor();
    h.ctx.surrender_focus(editor);
    h.frame(Vec::new());
    assert_eq!(h.ctx.focused(), None);
    h.key(Modifiers::COMMAND, Key::Enter);
    let tab = h.active_tab();
    assert!(h.view.app.query_results.contains_key(&tab));
}

#[test]
fn escape_reverts_a_cell_edit_and_enter_stages_it() {
    let mut h = Harness::connected();
    let tab = h.open_users();
    for (key, staged) in [(Key::Escape, false), (Key::Enter, true)] {
        let state = h.app().tables.get_mut(&tab).unwrap();
        state.editing = Some((1, 2));
        state.focus_editor = true;
        h.frame(Vec::new());
        h.text("edited");
        assert!(h.view.app.tables[&tab].drafts.contains_key(&(1, 2)));
        h.key(Modifiers::NONE, key);
        let state = &h.view.app.tables[&tab];
        assert_eq!((state.drafts.len(), state.editing), (0, None), "{key:?}");
        assert_eq!(!state.pending.is_empty(), staged, "{key:?}");
    }
}

#[test]
fn switching_tabs_from_the_keyboard_returns_the_caret_to_the_editor() {
    let mut h = Harness::connected();
    let query = h.active_tab();
    let table = h.open_users();
    assert_eq!(h.view.app.tabs.len(), 2);

    let control_tab = Modifiers {
        ctrl: true,
        ..Modifiers::NONE
    };
    h.key(control_tab, Key::Tab);
    assert_eq!(h.active_tab(), query);
    assert_eq!(h.ctx.focused(), Some(h.editor()));
    h.key(control_tab, Key::Tab);
    assert_eq!(h.active_tab(), table, "wraps around");
    let back = Modifiers {
        shift: true,
        ..control_tab
    };
    h.key(back, Key::Tab);
    assert_eq!(h.active_tab(), query);
    let tab = h.view.app.tabs.iter().find(|t| t.id == query).unwrap();
    assert_eq!(tab.kind, TabKind::Query);
}

#[test]
fn f5_reruns_the_query_and_reloads_the_table() {
    let mut h = Harness::connected();
    let query = h.active_tab();
    // Nothing has run yet, so there is nothing to refresh.
    h.key(Modifiers::NONE, Key::F5);
    assert!(!h.view.app.query_results.contains_key(&query));
    h.key(Modifiers::COMMAND, Key::Enter);
    h.app().query_results.remove(&query);
    h.key(Modifiers::NONE, Key::F5);
    assert!(h.view.app.query_results.contains_key(&query));
    h.app().query_results.remove(&query);
    h.key(Modifiers::COMMAND, Key::R);
    assert!(h.view.app.query_results.contains_key(&query));

    let table = h.open_users();
    h.app().tables.get_mut(&table).unwrap().page = None;
    h.key(Modifiers::NONE, Key::F5);
    assert!(h.view.app.tables[&table].page.is_some());
}

#[test]
fn ctrl_s_saves_staged_changes() {
    let mut h = Harness::connected();
    let tab = h.open_users();
    // Nothing staged: nothing is sent.
    h.key(Modifiers::COMMAND, Key::S);
    assert!(h.view.app.tables[&tab].message.is_none());

    // An edit still being typed is staged and saved with the rest.
    let state = h.app().tables.get_mut(&tab).unwrap();
    state.editing = Some((1, 2));
    state.focus_editor = true;
    h.frame(Vec::new());
    h.text("edited");
    h.key(Modifiers::COMMAND, Key::S);
    let state = &h.view.app.tables[&tab];
    assert!(state.drafts.is_empty() && state.editing.is_none());
    assert_eq!(state.pending.len(), 1);
    // The demo's sample data refuses saves, which shows the save was sent.
    let message = state.message.as_ref().expect("the save's answer");
    assert_eq!(message.text, dbm_core::demo::SAVE_REFUSED);
}

#[test]
fn a_dialog_takes_the_keyboard_from_the_editor() {
    let mut h = Harness::connected();
    let before = h.sql();
    h.key(Modifiers::COMMAND, Key::N);
    assert!(h.view.app.profile_form.is_some());
    h.text("Staging");
    assert_eq!(h.sql(), before, "typing does not reach the editor behind");
    assert_eq!(h.view.app.profile_form.as_ref().unwrap().name, "Staging");
    // Tab stays inside the dialog instead of walking the window behind it.
    let name = h.ctx.focused();
    let mut seen = vec![name];
    for _ in 0..60 {
        h.key(Modifiers::NONE, Key::Tab);
        assert_ne!(h.ctx.focused(), Some(h.editor()));
        if h.ctx.focused() == name {
            break;
        }
        seen.push(h.ctx.focused());
    }
    assert_eq!(h.ctx.focused(), name, "Tab wraps within the dialog");
    assert!(seen.len() > 3, "{seen:?}");
    // One Escape closes the dialog even with a field focused, and the
    // keyboard goes back to the editor.
    h.key(Modifiers::NONE, Key::Escape);
    assert!(h.view.app.profile_form.is_none());
    assert_eq!(h.ctx.focused(), Some(h.editor()));
    h.text("x");
    assert_eq!(h.sql().len(), before.len() + 1);
}

#[test]
fn keys_do_not_reach_the_table_behind_a_dialog() {
    let mut h = Harness::connected();
    let tab = h.open_users();
    h.app()
        .tables
        .get_mut(&tab)
        .unwrap()
        .click_row(2, false, false);
    h.frame(Vec::new());
    h.app().confirm_export = Some(tab);
    h.key(Modifiers::NONE, Key::Delete);
    assert!(h.view.app.tables[&tab].pending.is_empty());
    h.key(Modifiers::NONE, Key::Escape);
    assert_eq!(h.view.app.confirm_export, None);
    assert_eq!(h.view.app.tables[&tab].selected.len(), 1, "still selected");
    // With the dialog gone, Delete stages the selected row again.
    h.key(Modifiers::NONE, Key::Delete);
    assert_eq!(h.view.app.tables[&tab].pending.len(), 1);
}
