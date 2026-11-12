//! The query tab: toolbar (title, status, Refresh, Run), the SQL or Redis
//! editor with line numbers, highlighting, the active-statement band and
//! keyword completion, the history panel, and the results grid.

use std::cell::RefCell;

use dbm_core::models::DatabaseEngine;
use dbm_core::sql_text::{
    ExecutionKind, TokenKind, completions, csv_document, execution_target, highlight,
};
use dbm_ui::a11y::{Node, Role};
use dbm_ui::icons::{self, Icon};
use dbm_ui::input::{Key, Modifiers};
use dbm_ui::text_edit::{EditState, TextEdit, layout_lines, locate};
use dbm_ui::widgets::{icon_button, primary_button, secondary_button};
use dbm_ui::{
    Color32, CursorIcon, Dir, Id, Layer, Pos2, Rect, ScrollArea, Sense, Stroke, Ui, Vec2, pos2,
    theme, vec2,
};
use uuid::Uuid;

use super::grid;
use crate::messages::{self, Message};
use crate::state::{Completion, RequestKind, Workbench};

const GUTTER_TEXT: Color32 = Color32::from_rgb(0x5c, 0x5c, 0x62);
const EDITOR_MARGIN: Vec2 = Vec2 { x: 4.0, y: 2.0 };
const DEFAULT_COLUMN_WIDTH: f32 = 170.0;

fn editor_font() -> dbm_ui::Font {
    theme::ui(12.5)
}

thread_local! {
    /// The last highlighted script and its colour spans; tokenizing runs
    /// only when the text changes.
    static SPANS: RefCell<(u64, Vec<(usize, Color32)>)> = const { RefCell::new((0, Vec::new())) };
}

fn spans(engine: DatabaseEngine, text: &str) -> Vec<(usize, Color32)> {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (engine == DatabaseEngine::Redis, text).hash(&mut h);
    let key = h.finish();
    SPANS.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache.0 != key {
            let mut out = Vec::new();
            for token in highlight(engine, text) {
                let color = match token.kind {
                    TokenKind::Keyword => Color32::from_rgb(0xc6, 0x9c, 0xff),
                    TokenKind::String => Color32::from_rgb(0x9f, 0xd8, 0x8a),
                    TokenKind::Number => Color32::from_rgb(0xf0, 0xb1, 0x4c),
                    TokenKind::Comment => theme::FAINT,
                    TokenKind::QuotedIdentifier => theme::ACCENT_TEXT,
                };
                out.push((token.from, color));
                out.push((token.to, theme::TEXT));
            }
            *cache = (key, out);
        }
        cache.1.clone()
    })
}

fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

impl Workbench {
    pub(super) fn query_ui(&mut self, ui: &mut Ui, rect: Rect, tab_id: u64) {
        let Some(tab) = self
            .tabs
            .iter()
            .find(|t| t.id == tab_id)
            .map(crate::state::Tab::without_sql)
        else {
            return;
        };
        let engine = self.engine(tab.profile_id);
        let redis = engine == DatabaseEngine::Redis;
        let running = self.busy(RequestKind::Query, Some(tab_id));
        let exporting = self.exporting(tab_id);
        let target = self
            .tabs
            .iter()
            .find(|t| t.id == tab_id)
            .and_then(|t| execution_target(engine, &t.sql, t.selection.0, t.selection.1));
        let run_label = match target.as_ref().map(|t| t.kind) {
            Some(ExecutionKind::Selection) => "Run selection",
            _ if redis => "Run command",
            _ => "Run statement",
        };
        let selection_sql = target
            .as_ref()
            .filter(|t| t.kind == ExecutionKind::Selection)
            .map(|t| t.sql.clone());
        let embedded_dirty = self.tab_dirty(tab_id);
        let mut run: Option<(String, bool)> = None;
        let run_tip = if redis {
            "Run the selected command (Command/Ctrl+Enter)"
        } else {
            "Run the selected SQL (Command/Ctrl+Enter)"
        };

        // Toolbar.
        let (toolbar, rest) = rect.split_top(theme::TOOLBAR_HEIGHT);
        {
            let mut row = ui.child(toolbar.inset(14.0, 0.0, 14.0, 0.0), Dir::Left);
            let label = if running && self.pending_label(tab_id) == Some("Running query") {
                "Running…"
            } else {
                run_label
            };
            let clicked = primary_button(label)
                .icon(Icon::Play)
                .shortcut("Ctrl+Enter")
                .enabled(!running && !exporting && target.is_some())
                .id(Id::new(("run", tab_id)))
                .show(&mut row)
                .on_any_hover_text(run_tip)
                .clicked;
            if clicked {
                run = target.as_ref().map(|t| (t.sql.clone(), false));
            }
            let refresh_label = if running && self.pending_label(tab_id) == Some("Refreshing query")
            {
                "Refreshing…"
            } else {
                "Refresh"
            };
            let tip = if tab.last_executed.is_some() {
                "Re-run the last executed statement for fresh results"
            } else {
                "Run a statement first to enable refresh"
            };
            let refresh = secondary_button(refresh_label)
                .icon(Icon::Refresh)
                .enabled(!running && !exporting && tab.last_executed.is_some() && !embedded_dirty)
                .id(Id::new(("refresh", tab_id)))
                .show(&mut row)
                .on_any_hover_text(tip);
            if refresh.clicked {
                run = tab.last_executed.clone().map(|sql| (sql, true));
            }
            let left = row.available_rect();
            let mut l = row.child(left, Dir::Right);
            l.spacing.x = 10.0;
            let font = theme::bold(15.0);
            let (title, _) = l.ctx.fonts.truncate(&tab.title, font, l.available_width());
            l.label(&title, font, theme::TEXT_STRONG);
            let failed = self
                .query_errors
                .get(&tab_id)
                .is_some_and(|m| m.kind == messages::Kind::Error);
            if let Some(result) = self
                .query_results
                .get(&tab_id)
                .filter(|_| !running && !failed)
            {
                let mut status = format!("Query completed in {} ms", result.duration_ms);
                if tab.embedded.is_none() {
                    status.push_str(&format!(
                        " · {} rows",
                        theme::count(result.row_count as u64)
                    ));
                    if let Some(affected) = result.affected_rows {
                        status.push_str(&format!(" · {} affected", theme::count(affected)));
                    }
                }
                let (r, elided) = l.label_truncated(&status, theme::ui(11.5), theme::MUTED);
                if elided {
                    let id = l.make_id("status");
                    let _ = l.interact(r, id, Sense::HOVER).on_hover_text(status);
                }
            }
        }

        // Editor panel, resizable at its bottom edge (140 px up to 120 px
        // above the bottom).
        let max_h = (rest.height() - 120.0).max(160.0);
        let height = self
            .editor_heights
            .get(&tab_id)
            .copied()
            .unwrap_or(260.0)
            .clamp(140.0, max_h);
        let (panel, below) = rest.split_top(height);
        let inner = panel.inset(14.0, 0.0, 14.0, 12.0);
        let history_w = self
            .history_widths
            .get(&tab_id)
            .copied()
            .unwrap_or(250.0)
            .clamp(200.0, 460.0);
        let (history, card) = inner.split_right(history_w + 10.0);
        let history = history.with_min_x(history.left() + 10.0);
        {
            let mut p = ui.painter();
            p.rect(card, 10.0, theme::BG, Stroke::new(1.0, theme::BORDER));
            p.rect(
                history,
                10.0,
                theme::CHROME,
                Stroke::new(1.0, theme::BORDER),
            );
        }
        // History width: drag its left edge.
        let hedge = Rect::from_ltrb(
            history.left() - 7.0,
            history.top(),
            history.left() - 2.0,
            history.bottom(),
        );
        let hr = ui.interact(
            hedge,
            Id::new(("history-resize", tab_id)),
            Sense::DRAG.unfocusable(),
        );
        if hr.hovered || hr.dragged {
            ui.ctx.set_cursor(CursorIcon::ResizeHorizontal);
        }
        if hr.dragged {
            self.history_widths
                .insert(tab_id, (history_w - hr.drag_delta.x).clamp(200.0, 460.0));
        }
        if let Some(sql) = self.history_ui(ui, history, tab.profile_id)
            && let Some(t) = self.tabs.iter_mut().find(|t| t.id == tab_id)
        {
            t.selection = (sql.len(), sql.len());
            t.sql = sql;
            let mut st = EditState::load(ui, Id::new(("sql-editor", tab_id))).unwrap_or_default();
            st.set_caret(t.sql.len());
            st.store(ui, Id::new(("sql-editor", tab_id)));
        }
        let card_inner = card.shrink(1.0);
        let (hint, editor_rect) = card_inner.split_bottom(28.0);
        {
            let mut p = ui.painter();
            p.rect_filled(hint, dbm_ui::Radius::bottom(9.0), theme::CHROME);
            p.hline(
                hint.left()..=hint.right(),
                hint.top() + 0.5,
                Stroke::new(1.0, theme::HAIRLINE),
            );
            let text = if redis {
                "The outlined command or selection will run · Command/Ctrl+Enter · Preview limit: 10,000 rows"
            } else {
                "The outlined statement or selected SQL will run · Command/Ctrl+Enter · Preview limit: 10,000 rows"
            };
            p.text_truncated(
                hint.left() + 10.0,
                hint.center().y,
                text,
                theme::ui(11.5),
                theme::FAINT,
                hint.width() - 20.0,
            );
        }
        if let Some(sql) =
            self.sql_editor(ui, editor_rect.inset(0.0, 6.0, 6.0, 0.0), tab_id, engine)
        {
            run = Some((sql, false));
        }
        // `.editor-selection-run`: floats top-right while text is selected.
        if let Some(sql) = &selection_sql {
            let b = primary_button("Run selection")
                .icon(Icon::Play)
                .enabled(!running && !exporting)
                .id(Id::new(("selection-run", tab_id)));
            let w = b.width(ui);
            let r = Rect::from_min_size(
                pos2(card.right() - 10.0 - w, card.top() + 8.0),
                vec2(w, 30.0),
            );
            ui.painter()
                .shadow(r, 6.0, vec2(0.0, 6.0), 18.0, Color32::from_black_alpha(90));
            if b.show_in(ui, r).on_any_hover_text(run_tip).clicked {
                run = Some((sql.clone(), false));
            }
        }
        // The resize edge between editor and results: a 12 px grab area
        // and a small grip.
        let grab = Rect::from_ltrb(
            panel.left(),
            panel.bottom() - 6.0,
            panel.right(),
            panel.bottom() + 6.0,
        );
        let gr = ui.interact(
            grab,
            Id::new(("editor-resize", tab_id)),
            Sense::DRAG.unfocusable(),
        );
        if gr.hovered || gr.dragged {
            ui.ctx.set_cursor(CursorIcon::ResizeVertical);
        }
        if gr.dragged {
            self.editor_heights
                .insert(tab_id, (height + gr.drag_delta.y).clamp(140.0, max_h));
        }
        {
            let mut p = ui.painter();
            let line = if gr.hovered || gr.dragged {
                theme::ACCENT
            } else {
                theme::BORDER
            };
            p.hline(
                panel.left()..=panel.right(),
                panel.bottom() - 0.5,
                Stroke::new(1.0, line),
            );
            p.rect_filled(
                Rect::from_center_size(pos2(panel.center().x, panel.bottom()), vec2(36.0, 3.0)),
                1.5,
                theme::FAINT,
            );
        }

        if let Some((sql, refresh)) = run {
            self.run_query(tab_id, sql, refresh, false);
        }

        let mut c = ui.child(below, Dir::Down);
        c.spacing.y = 0.0;
        if let Some(error) = self.query_errors.get(&tab_id).cloned() {
            match error.opacity(c.ctx, self.now) {
                None => {
                    self.query_errors.remove(&tab_id);
                }
                Some(opacity) => match messages::inline(&mut c, &error, opacity) {
                    messages::Response::Dismiss => {
                        self.query_errors.remove(&tab_id);
                    }
                    messages::Response::Open => {
                        if let Some((path, _)) = &error.export {
                            crate::platform::open_path(path, false);
                        }
                    }
                    messages::Response::Reveal => {
                        if let Some((path, _)) = &error.export {
                            crate::platform::open_path(path, true);
                        }
                    }
                    messages::Response::None => {}
                },
            }
        }
        let results = c.available_rect();
        self.query_result_ui(&mut c, results, tab_id, tab.profile_id);
    }

    /// The code editor. Returns SQL to run when Command/Ctrl+Enter is pressed.
    fn sql_editor(
        &mut self,
        ui: &mut Ui,
        rect: Rect,
        tab_id: u64,
        engine: DatabaseEngine,
    ) -> Option<String> {
        let editor_id = Id::new(("sql-editor", tab_id));
        let focused = ui.ctx.has_focus(editor_id);
        let run_pressed = focused && ui.ctx.consume_key(Modifiers::COMMAND, Key::Enter);
        let mut completion = self
            .completion
            .take()
            .filter(|c| c.tab == tab_id && focused);
        // Completion keys are handled before the editor sees them.
        let mut accept = None;
        if let Some(open) = &mut completion {
            let none = Modifiers::NONE;
            if ui.ctx.consume_key(none, Key::Escape) {
                open.items.clear();
            } else {
                let count = open.items.len().max(1);
                if ui.ctx.consume_key(none, Key::ArrowDown) {
                    open.selected = (open.selected + 1) % count;
                }
                if ui.ctx.consume_key(none, Key::ArrowUp) {
                    open.selected = (open.selected + count - 1) % count;
                }
                if ui.ctx.consume_key(none, Key::Enter) || ui.ctx.consume_key(none, Key::Tab) {
                    accept = Some(open.selected);
                }
            }
        }
        let tab = self.tabs.iter_mut().find(|t| t.id == tab_id)?;
        let end = tab.selection.1.min(tab.sql.len());
        let accepted = accept.zip(completion.as_ref()).and_then(|(index, open)| {
            let replaceable = open.start <= end && tab.sql.is_char_boundary(open.start);
            Some((open.start, open.items.get(index)?.clone())).filter(|_| replaceable)
        });
        if let Some((start, word)) = accepted {
            tab.sql.replace_range(start..end, &word);
            let cursor = start + word.len();
            tab.selection = (cursor, cursor);
            let mut st = EditState::load(ui, editor_id).unwrap_or_default();
            st.set_caret(cursor);
            st.store(ui, editor_id);
            completion = None;
        }
        completion = completion.filter(|c| !c.items.is_empty());

        let font = editor_font();
        let lh = font.line_height();
        let lines = layout_lines(&tab.sql, true);
        let line_count = lines.len();
        // Gutter: right-aligned numbers left of a hairline.
        let digits = line_count.to_string().len().max(1);
        let gutter_w = 4.0 + ui.ctx.fonts.width(&"9".repeat(digits), font) + 10.0;
        let (gutter, text_rect) = rect.split_left(gutter_w);
        let st = EditState::load(ui, editor_id).unwrap_or_default();
        let origin = pos2(
            text_rect.left() + EDITOR_MARGIN.x - st.scroll.x,
            text_rect.top() + EDITOR_MARGIN.y - st.scroll.y,
        );
        let full_width = Rect::from_ltrb(text_rect.left(), 0.0, rect.right(), 0.0);
        {
            let clip = rect;
            let mut p = ui.painter_at(clip);
            p.vline(
                gutter.right() - 4.5,
                rect.top()..=rect.bottom(),
                Stroke::new(1.0, theme::HAIRLINE),
            );
            let first = ((st.scroll.y / lh).floor().max(0.0)) as usize;
            let visible = (rect.height() / lh).ceil() as usize + 2;
            for n in first..(first + visible).min(line_count) {
                let label = (n + 1).to_string();
                let w = p.fonts().width(&label, font);
                p.text(
                    pos2(gutter.right() - 10.0 - w, origin.y + n as f32 * lh),
                    label,
                    font,
                    GUTTER_TEXT,
                );
            }
            let row_y = |line: usize| origin.y + line as f32 * lh;
            // `.cm-activeLine`: the caret's row, while focused.
            if focused {
                let (line, _) = locate(
                    p.fonts(),
                    &tab.sql,
                    &lines,
                    font,
                    tab.selection.1.min(tab.sql.len()),
                );
                p.rect_filled(
                    Rect::from_ltrb(
                        full_width.left(),
                        row_y(line),
                        full_width.right(),
                        row_y(line) + lh,
                    ),
                    0.0,
                    Color32::from_white_alpha(6),
                );
            }
            // `.cm-active-sql-line`: a band, a left bar and hairlines around
            // the statement Ctrl+Enter would run.
            let (from, to) = tab.selection;
            if from == to
                && let Some(t) = execution_target(engine, &tab.sql, from, to)
            {
                let (a, _) = locate(p.fonts(), &tab.sql, &lines, font, t.from);
                let (b, _) = locate(p.fonts(), &tab.sql, &lines, font, t.to);
                let band = Rect::from_ltrb(
                    full_width.left(),
                    row_y(a),
                    full_width.right(),
                    row_y(b) + lh,
                );
                let accent = |alpha: f32| theme::ACCENT.gamma_multiply(alpha);
                p.rect_filled(band, 0.0, accent(0.05));
                p.hline(
                    band.left()..=band.right(),
                    band.top() + 0.5,
                    Stroke::new(1.0, accent(0.2)),
                );
                p.hline(
                    band.left()..=band.right(),
                    band.bottom() - 0.5,
                    Stroke::new(1.0, accent(0.2)),
                );
                p.rect_filled(
                    Rect::from_min_size(
                        pos2(text_rect.left() - 1.0, band.top()),
                        vec2(2.0, band.height()),
                    ),
                    0.0,
                    accent(0.5),
                );
            }
        }
        let colors = move |text: &str| spans(engine, text);
        let previous = tab.selection;
        let out = TextEdit::multiline(editor_id, &mut tab.sql)
            .font(font)
            .frameless()
            .margin(EDITOR_MARGIN.x, EDITOR_MARGIN.y)
            .colors(&colors)
            .name("SQL editor")
            .show_in(ui, text_rect);
        tab.selection = out.state.selection;
        // Open or refine keyword completion after typing an identifier.
        let (from, to) = tab.selection;
        if out.response.changed && from == to {
            let start = tab.sql[..to]
                .rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .map_or(0, |i| i + 1);
            let prefix = &tab.sql[start..to];
            let typed_letter = prefix
                .chars()
                .last()
                .is_some_and(|c| c.is_ascii_alphabetic());
            let mut items = if prefix.len() >= 2 && typed_letter {
                completions(engine, prefix)
            } else {
                Vec::new()
            };
            items.truncate(12);
            completion = (!items.is_empty()).then_some(Completion {
                tab: tab_id,
                start,
                items,
                selected: 0,
            });
        } else if tab.selection != previous && !out.response.changed {
            completion = None;
        }
        if let Some(open) = &completion {
            let lines = layout_lines(&tab.sql, true);
            let (line, x) = locate(&ui.ctx.fonts, &tab.sql, &lines, font, to);
            let o = out.text_origin;
            let anchor = Pos2 {
                x: o.x + x,
                y: o.y + (line + 1) as f32 * lh + 2.0,
            };
            let item_h = 22.0;
            let size = vec2(180.0, open.items.len() as f32 * item_h + 8.0);
            let area = Rect::from_min_size(anchor, size);
            let mut picked = None;
            ui.area(Layer::Popup, area, |ui| {
                let _ = ui.interact(area, editor_id.with("completion-bg"), Sense::HOVER);
                ui.painter().rect(
                    area,
                    6.0,
                    theme::POPOVER,
                    Stroke::new(1.0, theme::BORDER_STRONG),
                );
                for (index, item) in open.items.iter().enumerate() {
                    let r = Rect::from_min_size(
                        pos2(area.left() + 4.0, area.top() + 4.0 + index as f32 * item_h),
                        vec2(size.x - 8.0, item_h),
                    );
                    let id = editor_id.with(("completion", index));
                    let resp = ui.interact(r, id, Sense::CLICK.unfocusable());
                    let mut node = Node::new(id.0, Role::ListItem, item, r);
                    node.selected = Some(index == open.selected);
                    ui.a11y(node);
                    let selected = index == open.selected;
                    let mut p = ui.painter();
                    if selected || resp.hovered {
                        p.rect_filled(r, 5.0, theme::ACCENT_STRONG);
                    }
                    p.text_mid(
                        r.left() + 8.0,
                        r.center().y,
                        item,
                        font,
                        if selected || resp.hovered {
                            Color32::WHITE
                        } else {
                            theme::TEXT
                        },
                    );
                    if resp.clicked {
                        picked = Some(index);
                    }
                }
            });
            if let Some(index) = picked {
                let word = open.items[index].clone();
                tab.sql.replace_range(open.start..to, &word);
                let cursor = open.start + word.len();
                tab.selection = (cursor, cursor);
                let mut st = EditState::load(ui, editor_id).unwrap_or_default();
                st.set_caret(cursor);
                st.store(ui, editor_id);
                ui.ctx.request_focus(editor_id);
                completion = None;
            }
        }
        self.completion = completion;
        let tab = self.tabs.iter().find(|t| t.id == tab_id)?;
        if run_pressed {
            let (from, to) = tab.selection;
            return execution_target(engine, &tab.sql, from, to).map(|t| t.sql);
        }
        None
    }

    /// History for the tab's profile and database. Returns SQL the user picked.
    fn history_ui(&mut self, ui: &mut Ui, rect: Rect, profile: Uuid) -> Option<String> {
        let database = self
            .workspaces
            .get(&profile)
            .map(|w| w.profile.default_database.clone())
            .unwrap_or_default();
        let key = (profile, database);
        self.ensure_history(profile);
        let entries = self.histories.get(&key).map_or(&[][..], Vec::as_slice);
        let entries = &entries[..entries.len().min(100)];
        let (head, body) = rect.split_top(36.0);
        {
            let mut p = ui.painter();
            p.text_mid(
                head.left() + 12.0,
                head.center().y,
                "History",
                theme::bold(12.0),
                theme::SECONDARY,
            );
            let count = theme::count(entries.len() as u64);
            let w = p.fonts().width(&count, theme::ui(11.5));
            p.text_mid(
                head.right() - 12.0 - w,
                head.center().y,
                &count,
                theme::ui(11.5),
                theme::FAINT,
            );
            p.hline(
                head.left()..=head.right(),
                head.bottom() - 0.5,
                Stroke::new(1.0, theme::BORDER),
            );
        }
        if entries.is_empty() {
            let mut c = ui.child(body.shrink(12.0), Dir::Down);
            c.label_wrapped(
                "Run a query to start history.",
                theme::ui(12.5),
                theme::MUTED,
                false,
            );
            return None;
        }
        let mut picked = None;
        let list = body.inset(0.0, 4.0, 0.0, 1.0);
        let mut c = ui.child(list, Dir::Down);
        c.set_clip(list);
        c.spacing.y = 0.0;
        ScrollArea::vertical(Id::new(("history-scroll", profile))).show_rows(
            &mut c,
            48.0,
            entries.len(),
            |ui, rows| {
                for entry in &entries[rows] {
                    let time = entry
                        .executed_at
                        .with_timezone(&chrono::Local)
                        .format("%-I:%M:%S %p")
                        .to_string();
                    let outer = ui.allocate_row(48.0);
                    let rect = outer.shrink2(vec2(4.0, 0.0));
                    let id =
                        ui.make_id(("history", entry.executed_at.timestamp_micros(), &entry.sql));
                    let response = ui
                        .interact(rect, id, Sense::CLICK)
                        .on_hover_text(format!("{} ms\n\n{}", entry.duration_ms, entry.sql));
                    let mut node = Node::new(id.0, Role::ListItem, &entry.sql, rect);
                    node.focusable = true;
                    ui.a11y(node);
                    let hovered = response.hovered;
                    let mut p = ui.painter();
                    if hovered {
                        p.rect_filled(rect, 6.0, Color32::from_white_alpha(15));
                    }
                    let (glyph, color) = if entry.success {
                        (Icon::Check, theme::SUCCESS)
                    } else {
                        (Icon::Alert, theme::DANGER)
                    };
                    icons::paint(
                        &mut p,
                        Rect::from_center_size(
                            rect.left_top() + vec2(16.0, 16.0),
                            Vec2::splat(12.0),
                        ),
                        glyph,
                        color,
                    );
                    let text_left = rect.left() + 30.0;
                    let font = theme::ui(11.5);
                    p.text_truncated(
                        text_left,
                        rect.top() + 9.0 + font.line_height() / 2.0,
                        &one_line(&entry.sql),
                        font,
                        if hovered {
                            theme::TEXT
                        } else {
                            theme::SECONDARY
                        },
                        rect.right() - text_left - 10.0,
                    );
                    p.text(
                        pos2(text_left, rect.top() + 29.0),
                        time,
                        theme::ui(11.0),
                        theme::FAINT,
                    );
                    if response.clicked {
                        picked = Some(entry.sql.clone());
                    }
                }
            },
        );
        picked
    }

    fn query_result_ui(&mut self, ui: &mut Ui, rect: Rect, tab_id: u64, profile: Uuid) {
        let Some(result) = self.query_results.get(&tab_id) else {
            let text = "Results will appear here.";
            let w = ui.ctx.fonts.width(text, theme::ui(12.5));
            ui.painter().text_mid(
                rect.center().x - w / 2.0,
                rect.center().y,
                text,
                theme::ui(12.5),
                theme::FAINT,
            );
            return;
        };
        if self
            .tabs
            .iter()
            .find(|t| t.id == tab_id)
            .is_some_and(|t| t.embedded.is_some())
        {
            self.table_ui(ui, rect, tab_id, profile, true);
            return;
        }
        let mut c = ui.child(rect, Dir::Down);
        c.spacing.y = 0.0;
        for notice in &result.notices {
            let mut row = c.child(c.available_rect().inset(14.0, 6.0, 14.0, 0.0), Dir::Down);
            let r = row.label_wrapped(notice, theme::ui(11.5), theme::MUTED, false);
            c.advance_past(r.expand(0.0).with_max_y(r.bottom() + 6.0));
        }
        if result.columns.is_empty() {
            // `.query-empty`: a statement with no result set.
            let text = "Statement completed without a result set.";
            let w = c.ctx.fonts.width(text, theme::ui(12.5));
            c.painter().text_mid(
                rect.center().x - w / 2.0,
                rect.center().y,
                text,
                theme::ui(12.5),
                theme::FAINT,
            );
            return;
        }
        let enabled = !self.busy(RequestKind::Query, Some(tab_id)) && !self.exporting(tab_id);
        let rows = result.rows.len() as u64;
        let truncated = result.truncated;
        c.add_space(6.0);
        let bar = c.allocate_row(28.0).inset(14.0, 0.0, 14.0, 0.0);
        let (copy, export) = {
            let mut row = c.child(bar, Dir::Right);
            row.set_enabled(enabled);
            let copy = icon_button(&mut row, Icon::Copy, Some(&format!("Copy results ({})", theme::count(rows))), "Copy the displayed query results to the clipboard as CSV, including column headers").clicked;
            let saving = if self.exporting(tab_id) {
                "Saving…"
            } else {
                "Save CSV"
            };
            let export = icon_button(
                &mut row,
                Icon::Download,
                Some(saving),
                "Save the displayed query results to a CSV file",
            )
            .clicked;
            row.set_enabled(true);
            if truncated {
                row.label(
                    &format!("Showing first {} rows.", theme::count(rows)),
                    theme::ui(11.5),
                    theme::MODIFIED,
                );
            }
            (copy, export)
        };
        c.add_space(6.0);
        if copy {
            let columns: Vec<_> = result.columns.iter().map(|c| c.name.clone()).collect();
            c.ctx.copy_text(&csv_document(&columns, &result.rows));
            self.query_errors.insert(
                tab_id,
                Message::notice(
                    format!("Copied {} result rows as CSV.", theme::count(rows)),
                    self.now,
                ),
            );
        }
        let grid_rect = c.available_rect().inset(14.0, 0.0, 14.0, 12.0);
        let result = self.query_results.get(&tab_id).expect("checked above");
        let numeric: Vec<bool> = result
            .columns
            .iter()
            .map(|c| dbm_core::cell_values::numeric_column(&c.data_type))
            .collect();
        let widths_id = Id::new(("result-widths", tab_id, result.columns.len()));
        let mut widths: Vec<f32> = c.ctx.data(widths_id).unwrap_or_else(|| {
            // Like the desktop's auto-width table, columns share the full
            // width when they would otherwise leave it empty.
            let w =
                ((grid_rect.width() - 2.0) / result.columns.len() as f32).max(DEFAULT_COLUMN_WIDTH);
            vec![w; result.columns.len()]
        });
        {
            let mut g = c.child(grid_rect, Dir::Down);
            g.set_clip(grid_rect.shrink(1.0));
            let columns = &result.columns;
            let rows = &result.rows;
            grid::grid(
                &mut g,
                grid_rect.shrink(1.0),
                Id::new(("result", tab_id)),
                &mut widths,
                rows.len(),
                |ui, column, r| grid::static_header(ui, r, &columns[column].name, numeric[column]),
                |_, _, _, _| {},
                |ui, row, column, r| {
                    if let Some(value) = rows[row].get(column) {
                        grid::value_cell(ui, r, value, None, false, numeric[column], true);
                    }
                },
            );
        }
        c.painter()
            .rect_stroke(grid_rect, 10.0, Stroke::new(1.0, theme::BORDER));
        c.ctx.insert_data(widths_id, widths);
        if export {
            self.save_result(tab_id);
        }
    }
}
