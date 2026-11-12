//! Small widget builders shared by the views: Graphite buttons, labels,
//! chips, connection-colored dots, and menus.

use gtk::prelude::*;

use crate::icons::{self, Icon};

pub fn label(text: &str, classes: &[&str]) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.set_xalign(0.0);
    for class in classes {
        label.add_css_class(class);
    }
    label
}

/// A single-line label that ends in "…" when cut off.
pub fn ellipsized(text: &str, classes: &[&str]) -> gtk::Label {
    let label = label(text, classes);
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    label.set_single_line_mode(true);
    label
}

pub fn hbox(spacing: i32) -> gtk::Box {
    gtk::Box::new(gtk::Orientation::Horizontal, spacing)
}

pub fn vbox(spacing: i32) -> gtk::Box {
    gtk::Box::new(gtk::Orientation::Vertical, spacing)
}

pub fn spacer() -> gtk::Box {
    let spacer = hbox(0);
    spacer.set_hexpand(true);
    spacer
}

pub fn clear(container: &gtk::Box) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}

pub fn clear_list(list: &gtk::ListBox) {
    list.remove_all();
}

/// Icon and text side by side, the content of most buttons.
pub fn icon_label(icon: Option<Icon>, text: &str, icon_size: i32) -> gtk::Box {
    let content = hbox(6);
    content.set_halign(gtk::Align::Center);
    if let Some(icon) = icon {
        content.append(&icons::image(icon, icon_size));
    }
    if !text.is_empty() {
        let label = gtk::Label::new(Some(text));
        content.append(&label);
    }
    content
}

/// A button with Graphite classes: `primary`, `secondary-button`,
/// `danger-button`, `accent-button`, `toolbar-button`, or `text-button`.
pub fn button(class: &str, icon: Option<Icon>, text: &str, tooltip: &str) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class(class);
    if text.is_empty() {
        button.add_css_class("icon-only");
    }
    button.set_child(Some(&icon_label(
        icon,
        text,
        if class == "text-button" { 12 } else { 14 },
    )));
    if !tooltip.is_empty() {
        button.set_tooltip_text(Some(tooltip));
    }
    if text.is_empty() && !tooltip.is_empty() {
        button.update_property(&[gtk::accessible::Property::Label(tooltip)]);
    }
    button.set_valign(gtk::Align::Center);
    button
}

/// Replaces a `button`'s text, keeping its icon.
pub fn set_button_text(button: &gtk::Button, text: &str) {
    let mut child = button.child().and_then(|content| content.first_child());
    let mut found = None;
    while let Some(widget) = child {
        if let Ok(label) = widget.clone().downcast::<gtk::Label>()
            && !label.has_css_class("kbd")
        {
            found = Some(label);
        }
        child = widget.next_sibling();
    }
    if let Some(label) = found
        && label.text() != text
    {
        label.set_text(text);
    }
}

/// A primary button with a keyboard shortcut pill after its text.
pub fn primary_with_shortcut(icon: Icon, text: &str, shortcut: &str) -> gtk::Button {
    let button = button("primary", Some(icon), text, "");
    if let Some(content) = button.child().and_then(|c| c.downcast::<gtk::Box>().ok()) {
        let kbd = gtk::Label::new(Some(shortcut));
        kbd.add_css_class("kbd");
        content.append(&kbd);
        // The text stays the last label for `set_button_text`.
        if let Some(text) = content.first_child().and_then(|icon| icon.next_sibling()) {
            content.reorder_child_after(&kbd, Some(&text));
        }
    }
    button
}

pub fn chip(text: &str, class: &str) -> gtk::Label {
    let chip = gtk::Label::new(Some(text));
    chip.add_css_class("chip");
    chip.add_css_class(class);
    chip.set_valign(gtk::Align::Center);
    chip
}

/// A round dot in a connection color (`state::profile_color`), filled when
/// connected and outlined otherwise.
pub fn dot(color: &str, filled: bool, size: i32) -> gtk::Box {
    let dot = hbox(0);
    dot.set_size_request(size, size);
    dot.set_valign(gtk::Align::Center);
    dot.set_halign(gtk::Align::Center);
    dot.add_css_class(if filled { "dot" } else { "dot-outline" });
    dot.add_css_class(&color_class(color));
    dot
}

/// The CSS class `connection_css` defines for a connection color.
pub fn color_class(color: &str) -> String {
    format!("c-{}", color.trim_start_matches('#').to_ascii_lowercase())
}

fn valid_color(color: &str) -> Option<(u8, u8, u8)> {
    let hex = color.strip_prefix('#')?;
    if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let value = u32::from_str_radix(hex, 16).ok()?;
    Some(((value >> 16) as u8, (value >> 8) as u8, value as u8))
}

/// Rules for each connection color: dots, the active tab's top edge, tab
/// icons, and the selected schema row.
pub fn connection_css<'a>(colors: impl IntoIterator<Item = &'a str>) -> String {
    let mut css = String::new();
    for color in colors {
        let Some((r, g, b)) = valid_color(color) else {
            continue;
        };
        let class = color_class(color);
        // 55% connection color mixed with white, for the selected icon.
        let mix = |c: u8| (f32::from(c) * 0.55 + 255.0 * 0.45) as u8;
        css.push_str(&format!(
            ".{class}.dot {{ background-color: {color}; border-radius: 999px; }}\n\
             .{class}.dot-outline {{ border: 1.5px solid {color}; border-radius: 999px; }}\n\
             .{class}.tab-edge {{ background-color: {color}; }}\n\
             .{class}.tab-icon {{ color: rgba({r},{g},{b},0.85); }}\n\
             .schema-row.selected.{class} {{ background-color: rgba({r},{g},{b},0.24); }}\n\
             .schema-row.selected.{class} .icon {{ color: rgb({},{},{}); }}\n",
            mix(r),
            mix(g),
            mix(b)
        ));
    }
    css
}

/// A menu item for `menu`: its label, whether it is destructive, and what
/// it does.
pub type MenuItem = (String, bool, Box<dyn Fn()>);

/// A Graphite menu: 28 px rows, accent hover, danger rows in red. The
/// popover closes before an item runs.
pub fn menu(items: Vec<MenuItem>, width: i32) -> gtk::Popover {
    let popover = gtk::Popover::new();
    popover.set_has_arrow(false);
    popover.add_css_class("menu");
    popover.set_position(gtk::PositionType::Bottom);
    let list = vbox(1);
    list.set_size_request(width - 8, -1);
    for (text, danger, action) in items {
        let item = gtk::Button::new();
        item.add_css_class("menu-item");
        if danger {
            item.add_css_class("danger");
        }
        let label = label(&text, &[]);
        item.set_child(Some(&label));
        let weak = popover.downgrade();
        item.connect_clicked(move |_| {
            if let Some(popover) = weak.upgrade() {
                popover.popdown();
            }
            action();
        });
        list.append(&item);
    }
    popover.set_child(Some(&list));
    popover
}

/// A menu button whose popover is rebuilt each time it opens.
pub fn menu_button(
    class: &str,
    content: &impl IsA<gtk::Widget>,
    tooltip: &str,
    items: impl Fn() -> Vec<MenuItem> + 'static,
    width: i32,
) -> gtk::MenuButton {
    let button = gtk::MenuButton::new();
    button.add_css_class(class);
    button.set_child(Some(content));
    button.set_always_show_arrow(false);
    button.set_tooltip_text(Some(tooltip));
    button.set_valign(gtk::Align::Center);
    button.set_create_popup_func(move |button| {
        let popover = menu(items(), width);
        button.set_popover(Some(&popover));
    });
    button
}

#[cfg(test)]
mod tests {
    #[test]
    fn connection_css_skips_colors_that_could_inject_rules() {
        let css = super::connection_css(["#4C9AFF", "red; } * {", "#12345", "#zzzzzz"]);
        assert!(css.contains(".c-4c9aff.dot { background-color: #4C9AFF;"));
        assert_eq!(css.matches(".dot {").count(), 1);
        assert!(!css.contains("red"));
    }
}
