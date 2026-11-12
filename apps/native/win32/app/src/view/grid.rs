//! The data grid both query results and table tabs use: a sticky header
//! band, resizable columns, 32 px rows laid out only while visible, and
//! overlay scrollbars on both axes.

use dbm_ui::a11y::{Node, Role};
use dbm_ui::icons::Icon;
use dbm_ui::scroll::ScrollState;
use dbm_ui::widgets::icon_button_in;
use dbm_ui::{
    Color32, CursorIcon, Dir, Id, Rect, ScrollArea, Sense, Stroke, Ui, pos2, theme, vec2,
};
use serde_json::Value;

use crate::table::display_prefix;

pub const MIN_WIDTH: f32 = 60.0;
pub const MAX_WIDTH: f32 = 800.0;
/// `.data-grid th { border-right: 1px solid #242427 }`
pub const HEADER_RULE: Color32 = Color32::from_rgb(0x24, 0x24, 0x27);

/// What happened to a row this frame.
#[derive(Clone, Default)]
pub struct RowEvents {
    pub clicked: bool,
    pub double_clicked: bool,
    pub secondary_clicked: bool,
    pub hovered: bool,
    pub response: dbm_ui::Response,
}

/// Draws the grid in `rect`. `header(ui, column, rect)` draws a header
/// cell; `cell(ui, row, column, rect)` a body cell; `row_ui(ui, row, rect,
/// events)` runs first for each visible row with its click state, to paint
/// row backgrounds. Widths are dragged at the header's column edges.
pub fn grid(
    ui: &mut Ui,
    rect: Rect,
    id: Id,
    widths: &mut [f32],
    rows: usize,
    mut header: impl FnMut(&mut Ui, usize, Rect),
    mut row_ui: impl FnMut(&mut Ui, usize, Rect, &RowEvents),
    mut cell: impl FnMut(&mut Ui, usize, usize, Rect),
) -> ScrollState {
    let total: f32 = widths.iter().sum();
    let (head, body) = rect.split_top(theme::HEADER_HEIGHT);
    let scroll_id = id.with("scroll");
    let clip = body.intersect(ui.clip_rect());
    let mut c = ui.child(body, Dir::Down);
    c.set_clip(clip);
    c.spacing = vec2(0.0, 0.0);
    let out =
        ScrollArea::both(scroll_id).show_rows(&mut c, theme::ROW_HEIGHT, rows, |ui, range| {
            let left = ui.max_rect().left();
            for index in range {
                let top = ui.cursor().y;
                let row_rect = Rect::from_min_size(
                    pos2(left, top),
                    vec2(total.max(body.width()), theme::ROW_HEIGHT),
                );
                ui.advance_past(Rect::from_min_size(
                    row_rect.min,
                    vec2(total, theme::ROW_HEIGHT),
                ));
                let rid = id.with(("row", index));
                let r = ui.interact(row_rect, rid, Sense::CLICK.plain().unfocusable());
                let events = RowEvents {
                    clicked: r.clicked,
                    double_clicked: r.double_clicked,
                    secondary_clicked: r.secondary_clicked,
                    hovered: r.hovered,
                    response: r.clone(),
                };
                {
                    let mut p = ui.painter();
                    if r.hovered {
                        p.rect_filled(row_rect, 0.0, Color32::from_white_alpha(6));
                    }
                    p.hline(
                        row_rect.left()..=row_rect.left() + total,
                        row_rect.bottom() - 0.5,
                        Stroke::new(1.0, theme::HAIRLINE),
                    );
                }
                row_ui(ui, index, row_rect, &events);
                let mut x = left;
                for (column, w) in widths.iter().enumerate() {
                    let cell_rect = Rect::from_min_size(pos2(x, top), vec2(*w, theme::ROW_HEIGHT));
                    x += w;
                    if cell_rect.intersects(ui.clip_rect()) {
                        let clip = cell_rect.intersect(ui.clip_rect());
                        let mut cu = ui.child(cell_rect, Dir::Right);
                        cu.set_clip(clip);
                        cell(&mut cu, index, column, cell_rect);
                    }
                }
            }
        });
    // The header follows the body's horizontal scroll.
    let offset = out.state.offset.x;
    {
        let mut p = ui.painter();
        p.rect_filled(head, 0.0, theme::GRID_HEADER);
        p.hline(
            head.left()..=head.right(),
            head.bottom() - 0.5,
            Stroke::new(1.0, theme::BORDER),
        );
    }
    let mut x = head.left() - offset;
    let resize_id = id.with("resize");
    for column in 0..widths.len() {
        let w = widths[column];
        let cell_rect = Rect::from_min_size(pos2(x, head.top()), vec2(w, head.height()));
        x += w;
        if !cell_rect.intersects(head) {
            continue;
        }
        let clip = cell_rect.intersect(head).intersect(ui.clip_rect());
        {
            let mut hu = ui.child(cell_rect, Dir::Right);
            hu.set_clip(clip);
            hu.painter().vline(
                cell_rect.right() - 0.5,
                cell_rect.top()..=cell_rect.bottom(),
                Stroke::new(1.0, HEADER_RULE),
            );
            header(&mut hu, column, cell_rect);
        }
        // `.column-resize-handle`: an accent line while hovered or dragged.
        let edge = Rect::from_ltrb(
            cell_rect.right() - 4.0,
            head.top(),
            cell_rect.right() + 3.0,
            head.bottom(),
        )
        .intersect(head);
        if edge.width() <= 0.0 {
            continue;
        }
        let hid = resize_id.with(column);
        let r = ui.interact(edge, hid, Sense::DRAG.unfocusable());
        if r.hovered || r.dragged {
            ui.ctx.set_cursor(CursorIcon::ResizeHorizontal);
            ui.painter().vline(
                cell_rect.right() - 0.5,
                head.top()..=body.bottom(),
                Stroke::new(1.0, theme::ACCENT),
            );
        }
        if r.dragged {
            widths[column] = (w + r.drag_delta.x).clamp(MIN_WIDTH, MAX_WIDTH);
        }
    }
    out.state
}

/// A grid cell's text: `NULL` in faint italics, else the first 200
/// characters on one line.
pub fn cell_text(value: &Value) -> (String, dbm_ui::Font, Color32) {
    if value.is_null() {
        ("NULL".to_owned(), theme::ui(12.0).italics(), theme::FAINT)
    } else {
        (
            display_prefix(value, 200).replace('\n', " "),
            theme::ui(12.0),
            theme::TEXT,
        )
    }
}

/// A value cell: truncated text (right aligned for numbers) with room for
/// a Copy value button that shows while the cell is hovered. Elided text
/// gets a tooltip when `tooltip` is set.
pub fn value_cell(
    ui: &mut Ui,
    rect: Rect,
    value: &Value,
    color: Option<Color32>,
    strike: bool,
    right: bool,
    tooltip: bool,
) {
    let (text, font, base) = cell_text(value);
    let color = color.unwrap_or(base);
    let max = (rect.width() - 46.0).max(0.0);
    let (shown, elided) = ui.ctx.fonts.truncate(&text, font, max);
    let w = ui.ctx.fonts.width(&shown, font);
    let x = if right {
        rect.right() - 36.0 - w
    } else {
        rect.left() + 10.0
    };
    let y = rect.center().y - font.line_height() / 2.0;
    ui.painter()
        .text_styled(pos2(x, y), shown, font, color, false, strike);
    let cid = ui.make_id(("cell", rect.left() as i32, rect.top() as i32));
    let mut node = Node::new(cid.0, Role::Cell, &text, rect);
    node.value = Some(text.clone());
    ui.a11y(node);
    let hovered = ui.rect_contains_pointer(rect);
    if elided && tooltip && hovered {
        let _ = ui
            .interact(
                Rect::from_min_size(pos2(x, rect.top()), vec2(w, rect.height())),
                cid,
                Sense::HOVER,
            )
            .on_hover_text(text);
    }
    if hovered {
        let button =
            Rect::from_center_size(rect.right_center() - vec2(18.0, 0.0), vec2(28.0, 28.0));
        if icon_button_in(ui, button, true, Icon::Copy, None, "Copy value").clicked {
            ui.ctx.copy_text(&dbm_core::sql_text::display_value(value));
        }
    }
}

/// A plain header cell: the name, right aligned for numeric columns.
pub fn static_header(ui: &mut Ui, rect: Rect, name: &str, numeric: bool) {
    let font = theme::ui(12.0);
    let (shown, elided) = ui
        .ctx
        .fonts
        .truncate(name, font, (rect.width() - 20.0).max(0.0));
    let w = ui.ctx.fonts.width(&shown, font);
    let x = if numeric {
        rect.right() - 10.0 - w
    } else {
        rect.left() + 10.0
    };
    ui.painter()
        .text_mid(x, rect.center().y, &shown, font, theme::TEXT);
    let hid = ui.make_id(("header", name));
    let mut node = Node::new(hid.0, Role::Header, name, rect);
    node.focusable = false;
    ui.a11y(node);
    if elided {
        let _ = ui
            .interact(rect, hid, Sense::HOVER)
            .on_hover_text(name.to_owned());
    }
}
