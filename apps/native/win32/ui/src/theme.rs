//! Graphite design tokens (`docs/design-system.md`), value for value with
//! the egui app's `theme.rs` so both Windows clients look the same.

use crate::geom::Color32;
use crate::text::Font;

pub const BG: Color32 = Color32::from_rgb(0x16, 0x16, 0x18);
pub const CHROME: Color32 = Color32::from_rgb(0x1c, 0x1c, 0x1e);
pub const SIDEBAR: Color32 = Color32::from_rgb(0x1f, 0x1f, 0x21);
pub const GRID_HEADER: Color32 = Color32::from_rgb(0x1a, 0x1a, 0x1c);
pub const CONTROL: Color32 = Color32::from_rgb(0x2a, 0x2a, 0x2d);
pub const CONTROL_HOVER: Color32 = Color32::from_rgb(0x32, 0x32, 0x35);
pub const CONTROL_ACTIVE: Color32 = Color32::from_rgb(0x3a, 0x3a, 0x3d);
pub const POPOVER: Color32 = Color32::from_rgb(0x26, 0x26, 0x29);
pub const HAIRLINE: Color32 = Color32::from_rgb(0x20, 0x20, 0x23);
pub const BORDER: Color32 = Color32::from_rgb(0x2a, 0x2a, 0x2d);
pub const BORDER_STRONG: Color32 = Color32::from_rgb(0x35, 0x35, 0x38);
pub const TEXT_STRONG: Color32 = Color32::from_rgb(0xf5, 0xf5, 0xf7);
pub const TEXT: Color32 = Color32::from_rgb(0xe8, 0xe8, 0xea);
pub const SECONDARY: Color32 = Color32::from_rgb(0xc7, 0xc7, 0xcc);
pub const MUTED: Color32 = Color32::from_rgb(0xa0, 0xa0, 0xa6);
pub const FAINT: Color32 = Color32::from_rgb(0x80, 0x80, 0x87);
pub const ACCENT: Color32 = Color32::from_rgb(0x4c, 0x9a, 0xff);
pub const ACCENT_STRONG: Color32 = Color32::from_rgb(0x3b, 0x78, 0xc7);
pub const ACCENT_TEXT: Color32 = Color32::from_rgb(0x7d, 0xb6, 0xff);
pub const ACCENT_SOFT: Color32 = Color32::from_rgba_premultiplied(11, 22, 36, 36);
pub const EDIT_SURFACE: Color32 = Color32::from_rgb(0x10, 0x19, 0x2a);
pub const MODIFIED: Color32 = Color32::from_rgb(0xf0, 0xb1, 0x4c);
pub const MODIFIED_SOFT: Color32 = Color32::from_rgba_premultiplied(29, 21, 9, 31);
pub const MODIFIED_ROW_SOFT: Color32 = Color32::from_rgba_premultiplied(12, 9, 4, 13);
pub const SUCCESS: Color32 = Color32::from_rgb(0x5a, 0xd3, 0x94);
pub const DANGER: Color32 = Color32::from_rgb(0xff, 0x8a, 0x80);
pub const DANGER_SOFT: Color32 = Color32::from_rgba_premultiplied(23, 10, 9, 23);

/// Connection colors offered in the profile editor.
pub const CONNECTION_COLORS: [&str; 8] = [
    "#4c9aff", "#ff9f43", "#3dd6c6", "#b48cff", "#ff6b8a", "#7ed957", "#f0b14c", "#8e8e93",
];
pub const DEFAULT_CONNECTION_COLOR: &str = "#4c9aff";

pub const ROW_HEIGHT: f32 = 32.0;
pub const HEADER_HEIGHT: f32 = 34.0;
pub const TOOLBAR_HEIGHT: f32 = 44.0;
pub const PENDING_BAR_HEIGHT: f32 = 48.0;
pub const STATUS_BAR_HEIGHT: f32 = 28.0;
pub const INSPECTOR_WIDTH: f32 = 300.0;

pub fn parse_color(hex: &str) -> Color32 {
    let h = hex.trim_start_matches('#');
    if h.len() == 6
        && let Ok(v) = u32::from_str_radix(h, 16)
    {
        return Color32::from_hex(v);
    }
    ACCENT
}

/// Space Mono Regular: all UI, code and grid text.
pub const fn ui(size: f32) -> Font {
    Font::regular(size)
}

/// Space Mono Bold, for headings and primary buttons.
pub const fn bold(size: f32) -> Font {
    Font::bold(size)
}

/// A count with thousands separators, like `toLocaleString()` in en-US.
pub fn count(value: impl Into<u64>) -> String {
    let digits = value.into().to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn counts_use_thousands_separators() {
        assert_eq!(super::count(0u32), "0");
        assert_eq!(super::count(999u32), "999");
        assert_eq!(super::count(1_000u32), "1,000");
        assert_eq!(super::count(1_234_567u64), "1,234,567");
    }
}
