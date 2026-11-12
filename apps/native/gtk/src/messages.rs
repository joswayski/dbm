//! Inline errors, notices, and export results inside query and table views.
//! Errors are centered; notices read left to right; an export result links
//! the file and offers Show in folder.

use std::cell::RefCell;
use std::path::PathBuf;

use gtk::prelude::*;

use crate::app::{App, open_path};
use crate::icons::Icon;
use crate::state::{Kind, Message, count};
use crate::ui;

pub struct Slot {
    pub root: gtk::Box,
    signature: RefCell<String>,
}

impl Slot {
    pub fn new() -> Self {
        let root = ui::vbox(0);
        root.set_visible(false);
        Self {
            root,
            signature: RefCell::new(String::new()),
        }
    }

    /// Shows `message`, rebuilding only when it changes. `dismiss` clears it
    /// from the state.
    pub fn render(&self, app: &App, message: Option<&Message>, dismiss: impl Fn(&App) + 'static) {
        let signature = message.map_or_else(String::new, |m| {
            format!("{:?}|{}|{:?}|{}", m.kind, m.text, m.export, m.shown_at)
        });
        if *self.signature.borrow() == signature {
            return;
        }
        *self.signature.borrow_mut() = signature;
        ui::clear(&self.root);
        self.root.set_visible(message.is_some());
        let Some(message) = message else { return };
        let row = ui::hbox(6);
        row.add_css_class("message");
        row.add_css_class(if message.kind == Kind::Error {
            "error"
        } else {
            "notice"
        });
        match &message.export {
            Some((path, rows)) => export_body(app, &row, path.clone(), *rows),
            None => {
                let text = ui::label(&message.text, &[]);
                text.set_wrap(true);
                text.set_hexpand(true);
                if message.kind == Kind::Error {
                    text.set_xalign(0.5);
                    text.set_justify(gtk::Justification::Center);
                    // A matching gutter on the left keeps the text centered.
                    text.set_margin_start(24);
                }
                row.append(&text);
            }
        }
        let close = ui::button(
            "dismiss",
            Some(Icon::Close),
            "",
            if message.export.is_some() {
                "Dismiss export result"
            } else {
                "Dismiss message"
            },
        );
        let handle = app.clone();
        close.connect_clicked(move |_| dismiss(&handle));
        row.append(&close);
        self.root.append(&row);
    }
}

fn export_body(app: &App, row: &gtk::Box, path: PathBuf, rows: u64) {
    let name = path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    let noun = if rows == 1 { "row" } else { "rows" };
    let text = ui::hbox(0);
    text.set_hexpand(true);
    text.append(&ui::label(
        &format!("Exported {} {noun} to ", count(rows)),
        &[],
    ));
    let link = gtk::Button::new();
    link.add_css_class("link-button");
    let link_label = ui::label(&name, &["link"]);
    link.set_child(Some(&link_label));
    link.set_tooltip_text(Some("Open the exported file"));
    let handle = app.clone();
    let open = path.clone();
    link.connect_clicked(move |_| open_path(&handle.window, &open, false));
    text.append(&link);
    text.append(&ui::label(".", &[]));
    row.append(&text);
    let reveal = ui::button(
        "secondary-button",
        Some(Icon::Folder),
        "Show in folder",
        "Reveal the exported file",
    );
    reveal.add_css_class("small");
    let handle = app.clone();
    reveal.connect_clicked(move |_| open_path(&handle.window, &path, true));
    row.append(&reveal);
}
