//! Text measurement and line layout. Every Anybase face is Space Mono, so
//! advances are almost always the same per character; the backend still
//! reports each one (fallback fonts for CJK or emoji are wider), and the
//! layout here is what both the renderer and hit testing use.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Range;

/// Space Mono's line box (hhea ascender 1120 + descender 361 per 1000 em).
pub const LINE_HEIGHT: f32 = 1.481;
/// The baseline below the top of the line box, in em.
pub const ASCENT: f32 = 1.12;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Font {
    pub size: f32,
    pub bold: bool,
    pub italic: bool,
}

impl Font {
    pub const fn regular(size: f32) -> Self {
        Font {
            size,
            bold: false,
            italic: false,
        }
    }

    pub const fn bold(size: f32) -> Self {
        Font {
            size,
            bold: true,
            italic: false,
        }
    }

    pub const fn italics(mut self) -> Self {
        self.italic = true;
        self
    }

    pub fn line_height(&self) -> f32 {
        self.size * LINE_HEIGHT
    }
}

/// What a renderer knows about glyphs: the advance of `ch` in `font`, in DIPs.
pub trait FontBackend {
    fn advance(&self, ch: char, font: Font) -> f32;
}

/// A fixed-pitch stand-in for tests: Space Mono's 0.612 em everywhere and
/// 1 em for wide (CJK and emoji) characters.
pub struct MonoMetrics;

impl FontBackend for MonoMetrics {
    fn advance(&self, ch: char, font: Font) -> f32 {
        let wide = matches!(ch as u32, 0x1100..=0x115F | 0x2E80..=0xA4CF | 0xAC00..=0xD7A3 | 0xF900..=0xFAFF | 0xFE30..=0xFE4F | 0xFF00..=0xFF60 | 0x1F300..=0x1FAFF);
        if ch == '\n' {
            0.0
        } else if wide {
            font.size
        } else {
            font.size * 0.612
        }
    }
}

pub struct Fonts {
    backend: Box<dyn FontBackend>,
    cache: RefCell<HashMap<(char, u32, u8), f32>>,
}

/// One laid-out line: its byte range in the source and its width.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub range: Range<usize>,
    pub width: f32,
}

impl Fonts {
    pub fn new(backend: Box<dyn FontBackend>) -> Self {
        Fonts {
            backend,
            cache: RefCell::default(),
        }
    }

    pub fn advance(&self, ch: char, font: Font) -> f32 {
        if ch == '\n' || ch == '\r' {
            return 0.0;
        }
        if ch == '\t' {
            return self.advance(' ', font) * 4.0;
        }
        let key = (
            ch,
            font.size.to_bits(),
            font.bold as u8 | (font.italic as u8) << 1,
        );
        if let Some(w) = self.cache.borrow().get(&key) {
            return *w;
        }
        let w = self.backend.advance(ch, font);
        self.cache.borrow_mut().insert(key, w);
        w
    }

    pub fn width(&self, text: &str, font: Font) -> f32 {
        text.chars().map(|c| self.advance(c, font)).sum()
    }

    /// `text` cut to fit `max_width` with a trailing ellipsis, and whether
    /// it was cut. Newlines end the line.
    pub fn truncate(&self, text: &str, font: Font, max_width: f32) -> (String, bool) {
        let line = text.split('\n').next().unwrap_or("");
        let cut = line.len() < text.len();
        if !cut && self.width(line, font) <= max_width {
            return (line.to_owned(), false);
        }
        let ellipsis = self.advance('…', font);
        let mut out = String::new();
        let mut used = 0.0;
        for c in line.chars() {
            let w = self.advance(c, font);
            if used + w + ellipsis > max_width {
                break;
            }
            used += w;
            out.push(c);
        }
        out.push('…');
        (out, true)
    }

    /// Word-wrapped lines of `text` no wider than `max_width` (a word longer
    /// than the line breaks anywhere). `\n` always breaks.
    pub fn wrap(&self, text: &str, font: Font, max_width: f32) -> Vec<Line> {
        let mut lines = Vec::new();
        let mut start = 0;
        for paragraph in text.split('\n') {
            let base = start;
            self.wrap_paragraph(paragraph, base, font, max_width, &mut lines);
            start = base + paragraph.len() + 1;
        }
        lines
    }

    fn wrap_paragraph(
        &self,
        text: &str,
        base: usize,
        font: Font,
        max_width: f32,
        out: &mut Vec<Line>,
    ) {
        if text.is_empty() {
            out.push(Line {
                range: base..base,
                width: 0.0,
            });
            return;
        }
        let mut line_start = 0;
        let mut width = 0.0;
        // The last break opportunity: byte offset after a space, and the
        // line width up to (and excluding) that space.
        let mut last_break: Option<(usize, f32)> = None;
        let chars: Vec<(usize, char)> = text.char_indices().collect();
        let mut i = 0;
        while i < chars.len() {
            let (offset, c) = chars[i];
            let w = self.advance(c, font);
            if c == ' ' {
                last_break = Some((offset + 1, width));
                width += w;
                i += 1;
                continue;
            }
            if width + w > max_width && offset > line_start {
                match last_break.take() {
                    Some((at, line_width)) if at > line_start => {
                        out.push(Line {
                            range: base + line_start..base + at - 1,
                            width: line_width,
                        });
                        line_start = at;
                        // Re-measure from the break to this character.
                        width = text[at..offset].chars().map(|c| self.advance(c, font)).sum();
                    }
                    _ => {
                        out.push(Line {
                            range: base + line_start..base + offset,
                            width,
                        });
                        line_start = offset;
                        width = 0.0;
                    }
                }
                continue;
            }
            width += w;
            i += 1;
        }
        out.push(Line {
            range: base + line_start..base + text.len(),
            width,
        });
    }

    /// The x offset of every char boundary in `line`, starting with 0.
    pub fn positions(&self, line: &str, font: Font) -> Vec<f32> {
        let mut out = Vec::with_capacity(line.len() + 1);
        let mut x = 0.0;
        out.push(0.0);
        for c in line.chars() {
            x += self.advance(c, font);
            out.push(x);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fonts() -> Fonts {
        Fonts::new(Box::new(MonoMetrics))
    }

    #[test]
    fn truncation_keeps_whole_characters_and_reports_cuts() {
        let f = fonts();
        let font = Font::regular(10.0);
        // 6.12 px per character.
        assert_eq!(f.truncate("abc", font, 30.0), ("abc".into(), false));
        let (text, cut) = f.truncate("abcdefghij", font, 30.0);
        assert!(cut);
        assert_eq!(text, "abc…");
        assert!(f.width(&text, font) <= 30.0);
        // A second line counts as cut even when the first fits.
        assert_eq!(f.truncate("ab\ncd", font, 100.0), ("ab…".into(), true));
    }

    #[test]
    fn wrapping_breaks_at_spaces_then_anywhere() {
        let f = fonts();
        let font = Font::regular(10.0);
        let text = "hello world foo";
        // 7 characters per line (42.8 px).
        let lines = f.wrap(text, font, 45.0);
        let parts: Vec<&str> = lines.iter().map(|l| &text[l.range.clone()]).collect();
        assert_eq!(parts, ["hello", "world", "foo"]);
        let text = "abcdefghijklmnop";
        let lines = f.wrap(text, font, 45.0);
        let parts: Vec<&str> = lines.iter().map(|l| &text[l.range.clone()]).collect();
        assert_eq!(parts, ["abcdefg", "hijklmn", "op"]);
        let text = "a\n\nb";
        let lines = f.wrap(text, font, 45.0);
        let parts: Vec<&str> = lines.iter().map(|l| &text[l.range.clone()]).collect();
        assert_eq!(parts, ["a", "", "b"]);
    }
}
