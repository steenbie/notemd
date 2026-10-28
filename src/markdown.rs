//! Markdown highlighting for the editor, plus "markdown actions"
//! (toggle checklists, open links, jump to [[other notes]]).

use eframe::egui::{text::LayoutJob, Color32, FontFamily, FontId, Stroke, TextFormat};

#[derive(Clone, Copy)]
pub struct Palette {
    pub text: Color32,
    pub strong: Color32,
    pub dim: Color32,
    pub accent: Color32,
    pub link: Color32,
    pub code: Color32,
    pub code_bg: Color32,
    pub heading: Color32,
}

impl Palette {
    pub fn for_visuals(dark: bool) -> Self {
        if dark {
            Self {
                text: Color32::from_rgb(0xD8, 0xDB, 0xE2),
                strong: Color32::from_rgb(0xFF, 0xFF, 0xFF),
                dim: Color32::from_rgb(0x7D, 0x84, 0x92),
                accent: Color32::from_rgb(0x5F, 0xD3, 0x9A),
                link: Color32::from_rgb(0x6C, 0xB6, 0xFF),
                code: Color32::from_rgb(0xF2, 0xB8, 0x6B),
                code_bg: Color32::from_rgb(0x2A, 0x2E, 0x36),
                heading: Color32::from_rgb(0xFF, 0xFF, 0xFF),
            }
        } else {
            Self {
                text: Color32::from_rgb(0x2B, 0x2F, 0x36),
                strong: Color32::from_rgb(0x00, 0x00, 0x00),
                dim: Color32::from_rgb(0x8A, 0x90, 0x9C),
                accent: Color32::from_rgb(0x12, 0x8C, 0x5A),
                link: Color32::from_rgb(0x1A, 0x6D, 0xD8),
                code: Color32::from_rgb(0xB3, 0x55, 0x00),
                code_bg: Color32::from_rgb(0xEE, 0xF0, 0xF3),
                heading: Color32::from_rgb(0x10, 0x12, 0x16),
            }
        }
    }
}

struct Fmt<'a> {
    p: &'a Palette,
    size: f32,
    family: FontFamily,
}

impl Fmt<'_> {
    fn base(&self, color: Color32) -> TextFormat {
        TextFormat { font_id: FontId::new(self.size, self.family.clone()), color, ..Default::default() }
    }
    fn plain(&self) -> TextFormat {
        self.base(self.p.text)
    }
    fn mono(&self) -> TextFormat {
        TextFormat {
            font_id: FontId::new(self.size * 0.95, FontFamily::Monospace),
            color: self.p.code,
            background: self.p.code_bg,
            ..Default::default()
        }
    }
}

/// Build a coloured layout for the whole note.
pub fn highlight(text: &str, p: &Palette, size: f32, monospace: bool) -> LayoutJob {
    let family = if monospace { FontFamily::Monospace } else { FontFamily::Proportional };
    let f = Fmt { p, size, family };
    let mut job = LayoutJob::default();
    let mut in_fence = false;

    for line in text.split_inclusive('\n') {
        let body = line.trim_end_matches(['\n', '\r']);
        let nl = &line[body.len()..];
        let trimmed = body.trim_start();

        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            job.append(line, 0.0, f.mono());
            continue;
        }
        if in_fence {
            job.append(line, 0.0, f.mono());
            continue;
        }

        // Headings
        let hashes = trimmed.chars().take_while(|c| *c == '#').count();
        if (1..=6).contains(&hashes) && trimmed[hashes..].starts_with(' ') || (1..=6).contains(&hashes) && trimmed.len() == hashes {
            let scale = [1.55, 1.35, 1.2, 1.1, 1.05, 1.0][hashes - 1];
            let indent = body.len() - trimmed.len();
            let mut marker = f.base(p.dim);
            marker.font_id.size = size * scale;
            job.append(&body[..indent + hashes], 0.0, marker);
            let mut h = f.base(p.heading);
            h.font_id.size = size * scale;
            inline(&mut job, &body[indent + hashes..], &f, h);
            job.append(nl, 0.0, f.plain());
            continue;
        }

        // Block quote
        if trimmed.starts_with('>') {
            let mut q = f.base(p.dim);
            q.italics = true;
            inline(&mut job, body, &f, q);
            job.append(nl, 0.0, f.plain());
            continue;
        }

        // Horizontal rule
        let t = trimmed.trim_end();
        if t.len() >= 3 && (t.chars().all(|c| c == '-') || t.chars().all(|c| c == '*') || t.chars().all(|c| c == '_')) {
            job.append(line, 0.0, f.base(p.dim));
            continue;
        }

        // Checklist item
        if let Some((marker_end, checked)) = checkbox_marker(body) {
            job.append(&body[..marker_end], 0.0, f.base(p.accent));
            let mut rest = f.plain();
            if checked {
                rest.color = p.dim;
                rest.strikethrough = Stroke::new(1.0, p.dim);
            }
            inline(&mut job, &body[marker_end..], &f, rest);
            job.append(nl, 0.0, f.plain());
            continue;
        }

        // Bullet / numbered list
        if let Some(marker_end) = list_marker(body) {
            job.append(&body[..marker_end], 0.0, f.base(p.accent));
            inline(&mut job, &body[marker_end..], &f, f.plain());
            job.append(nl, 0.0, f.plain());
            continue;
        }

        inline(&mut job, body, &f, f.plain());
        job.append(nl, 0.0, f.plain());
    }
    job
}

/// Inline spans: `code`, **bold**, *italic*, [[wiki]], [text](url), bare URLs.
fn inline(job: &mut LayoutJob, s: &str, f: &Fmt, base: TextFormat) {
    let p = f.p;
    let mut plain_start = 0;
    let mut i = 0;
    let b = s.as_bytes();

    let flush = |job: &mut LayoutJob, from: usize, to: usize| {
        if to > from {
            job.append(&s[from..to], 0.0, base.clone());
        }
    };

    while i < s.len() {
        if !s.is_char_boundary(i) {
            i += 1;
            continue;
        }
        let rest = &s[i..];

        // `code`
        if b[i] == b'`' {
            if let Some(end) = rest[1..].find('`') {
                flush(job, plain_start, i);
                let mut m = f.mono();
                m.font_id.size = base.font_id.size * 0.95;
                job.append(&rest[..end + 2], 0.0, m);
                i += end + 2;
                plain_start = i;
                continue;
            }
        }
        // [[wiki link]]
        if rest.starts_with("[[") {
            if let Some(end) = rest[2..].find("]]") {
                if end > 0 && !rest[2..2 + end].contains('\n') {
                    flush(job, plain_start, i);
                    let mut w = base.clone();
                    w.color = p.accent;
                    w.underline = Stroke::new(1.0, p.accent);
                    job.append(&rest[..end + 4], 0.0, w);
                    i += end + 4;
                    plain_start = i;
                    continue;
                }
            }
        }
        // [text](url)
        if b[i] == b'[' {
            if let Some(close) = rest.find("](") {
                if let Some(paren) = rest[close + 2..].find(')') {
                    let total = close + 2 + paren + 1;
                    flush(job, plain_start, i);
                    let mut d = base.clone();
                    d.color = p.dim;
                    let mut l = base.clone();
                    l.color = p.link;
                    l.underline = Stroke::new(1.0, p.link);
                    job.append("[", 0.0, d.clone());
                    job.append(&rest[1..close], 0.0, l);
                    job.append(&rest[close..total], 0.0, d);
                    i += total;
                    plain_start = i;
                    continue;
                }
            }
        }
        // bare URL
        if rest.starts_with("http://") || rest.starts_with("https://") {
            let len = url_len(rest);
            flush(job, plain_start, i);
            let mut l = base.clone();
            l.color = p.link;
            l.underline = Stroke::new(1.0, p.link);
            job.append(&rest[..len], 0.0, l);
            i += len;
            plain_start = i;
            continue;
        }
        // **bold** / __bold__
        if rest.starts_with("**") || rest.starts_with("__") {
            let delim = &rest[..2];
            if let Some(end) = rest[2..].find(delim) {
                if end > 0 {
                    flush(job, plain_start, i);
                    let mut d = base.clone();
                    d.color = p.dim;
                    let mut bold = base.clone();
                    bold.color = p.strong;
                    job.append(delim, 0.0, d.clone());
                    job.append(&rest[2..2 + end], 0.0, bold);
                    job.append(delim, 0.0, d);
                    i += end + 4;
                    plain_start = i;
                    continue;
                }
            }
        }
        // *italic* / _italic_ (only at a word boundary, to avoid snake_case)
        if (b[i] == b'*' || b[i] == b'_') && (i == 0 || !b[i - 1].is_ascii_alphanumeric()) {
            let delim = b[i] as char;
            if let Some(end) = rest[1..].find(delim) {
                let inner = &rest[1..1 + end];
                if end > 0 && !inner.starts_with(' ') && !inner.ends_with(' ') {
                    flush(job, plain_start, i);
                    let mut it = base.clone();
                    it.italics = true;
                    job.append(&rest[..end + 2], 0.0, it);
                    i += end + 2;
                    plain_start = i;
                    continue;
                }
            }
        }
        // ~~strike~~
        if rest.starts_with("~~") {
            if let Some(end) = rest[2..].find("~~") {
                flush(job, plain_start, i);
                let mut st = base.clone();
                st.color = p.dim;
                st.strikethrough = Stroke::new(1.0, p.dim);
                job.append(&rest[..end + 4], 0.0, st);
                i += end + 4;
                plain_start = i;
                continue;
            }
        }
        i += rest.chars().next().map(|c| c.len_utf8()).unwrap_or(1);
    }
    flush(job, plain_start, s.len());
}

fn url_len(s: &str) -> usize {
    let mut len = s.find(|c: char| c.is_whitespace() || c == '<' || c == '>' || c == '"').unwrap_or(s.len());
    // Don't swallow trailing punctuation.
    while len > 0 && s[..len].ends_with(['.', ',', ';', ':', '!', '?', ')', ']']) {
        if s[..len].ends_with(')') && s[..len].matches('(').count() >= s[..len].matches(')').count() {
            break;
        }
        len -= 1;
    }
    len
}

/// `- [ ] ` / `* [x] ` → (byte index after the marker, checked)
pub fn checkbox_marker(line: &str) -> Option<(usize, bool)> {
    let indent = line.len() - line.trim_start().len();
    let t = &line[indent..];
    let mut chars = t.chars();
    let bullet = chars.next()?;
    if !matches!(bullet, '-' | '*' | '+') {
        return None;
    }
    let rest = &t[1..];
    if rest.len() >= 4 && rest.starts_with(" [") && &rest[3..4] == "]" {
        let c = &rest[2..3];
        if c == " " || c == "x" || c == "X" {
            let after = indent + 5;
            let end = if line[after..].starts_with(' ') { after + 1 } else { after };
            return Some((end, c != " "));
        }
    }
    None
}

/// `- `, `* `, `+ `, `12. `, `3) ` → byte index after the marker.
pub fn list_marker(line: &str) -> Option<usize> {
    let indent = line.len() - line.trim_start().len();
    let t = &line[indent..];
    if t.starts_with("- ") || t.starts_with("* ") || t.starts_with("+ ") {
        return Some(indent + 2);
    }
    let digits = t.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits > 0 && digits < 10 && (t[digits..].starts_with(". ") || t[digits..].starts_with(") ")) {
        return Some(indent + digits + 2);
    }
    None
}

#[derive(Debug, PartialEq)]
pub enum Action {
    /// Replace the single byte at this index with ' ' or 'x'.
    ToggleCheck { byte: usize, checked: bool },
    OpenUrl(String),
    OpenNote(String),
}

/// What Ctrl+Enter should do with the cursor at `char_idx`.
pub fn action_at(text: &str, char_idx: usize) -> Option<Action> {
    let byte = char_to_byte(text, char_idx);
    let line_start = text[..byte].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let line_end = text[byte..].find('\n').map(|i| byte + i).unwrap_or(text.len());
    let line = &text[line_start..line_end];
    let col = byte - line_start;

    // Collect link-ish spans on the line, preferring the one under the cursor.
    let mut spans: Vec<(usize, usize, Action)> = Vec::new();
    let mut i = 0;
    while i < line.len() {
        if !line.is_char_boundary(i) {
            i += 1;
            continue;
        }
        let rest = &line[i..];
        if rest.starts_with("[[") {
            if let Some(end) = rest[2..].find("]]") {
                let name = rest[2..2 + end].split('|').next().unwrap_or("").trim().to_string();
                if !name.is_empty() {
                    spans.push((i, i + end + 4, Action::OpenNote(name)));
                }
                i += end + 4;
                continue;
            }
        }
        if rest.starts_with('[') {
            if let Some(close) = rest.find("](") {
                if let Some(paren) = rest[close + 2..].find(')') {
                    let url = rest[close + 2..close + 2 + paren].trim().to_string();
                    let total = close + 3 + paren;
                    spans.push((i, i + total, Action::OpenUrl(url)));
                    i += total;
                    continue;
                }
            }
        }
        if rest.starts_with("http://") || rest.starts_with("https://") {
            let len = url_len(rest);
            spans.push((i, i + len, Action::OpenUrl(rest[..len].to_string())));
            i += len;
            continue;
        }
        i += rest.chars().next().map(|c| c.len_utf8()).unwrap_or(1);
    }

    if let Some(pos) = spans.iter().position(|(s, e, _)| col >= *s && col <= *e) {
        return Some(spans.swap_remove(pos).2);
    }
    if let Some((_, checked)) = checkbox_marker(line) {
        let indent = line.len() - line.trim_start().len();
        return Some(Action::ToggleCheck { byte: line_start + indent + 3, checked });
    }
    spans.into_iter().next().map(|s| s.2)
}

/// Apply a checkbox toggle; returns the new text.
pub fn toggle_check(text: &str, byte: usize, checked: bool) -> String {
    let mut s = String::with_capacity(text.len());
    s.push_str(&text[..byte]);
    s.push(if checked { ' ' } else { 'x' });
    s.push_str(&text[byte + 1..]);
    s
}

/// Result of pressing Enter at the end of a list line.
pub enum ListContinue {
    /// Insert "\n" + this prefix.
    Continue(String),
    /// The item was empty: remove the marker (end the list).
    End { line_start_byte: usize },
}

/// If the line holding `char_idx` is a list item, work out what Enter should insert.
pub fn list_continuation(text: &str, char_idx: usize) -> Option<ListContinue> {
    let byte = char_to_byte(text, char_idx);
    let line_start = text[..byte].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let line_end = text[byte..].find('\n').map(|i| byte + i).unwrap_or(text.len());
    let line = &text[line_start..line_end];
    let indent = &line[..line.len() - line.trim_start().len()];

    let (marker_end, prefix) = if let Some((end, _)) = checkbox_marker(line) {
        let bullet = &line[indent.len()..indent.len() + 1];
        (end, format!("{indent}{bullet} [ ] "))
    } else if let Some(end) = list_marker(line) {
        let m = &line[indent.len()..end];
        let digits: String = m.chars().take_while(|c| c.is_ascii_digit()).collect();
        if digits.is_empty() {
            (end, format!("{indent}{m}"))
        } else {
            let n: u64 = digits.parse().unwrap_or(0);
            let sep = &m[digits.len()..];
            (end, format!("{indent}{}{sep}", n + 1))
        }
    } else {
        return None;
    };
    // Only when the cursor is after the marker.
    if byte - line_start < marker_end {
        return None;
    }
    if line[marker_end..].trim().is_empty() {
        return Some(ListContinue::End { line_start_byte: line_start });
    }
    Some(ListContinue::Continue(prefix))
}

pub fn char_to_byte(s: &str, char_idx: usize) -> usize {
    s.char_indices().nth(char_idx).map(|(b, _)| b).unwrap_or(s.len())
}

pub fn byte_to_char(s: &str, byte: usize) -> usize {
    s[..byte.min(s.len())].chars().count()
}

/// Line ending used for a freshly inserted line (keep CRLF files CRLF).
pub fn newline_for(text: &str) -> &'static str {
    if text.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checkbox_and_lists() {
        assert_eq!(checkbox_marker("- [ ] milk"), Some((6, false)));
        assert_eq!(checkbox_marker("  * [x] done"), Some((8, true)));
        assert_eq!(checkbox_marker("- milk"), None);
        assert_eq!(list_marker("12. x"), Some(4));
        assert_eq!(list_marker("- x"), Some(2));
        assert_eq!(list_marker("-x"), None);
    }

    #[test]
    fn actions() {
        let t = "hello\n- [ ] milk\nsee [[Other note]] and https://example.com.";
        assert_eq!(action_at(t, 9), Some(Action::ToggleCheck { byte: 9, checked: false }));
        let on = toggle_check(t, 9, false);
        assert!(on.contains("- [x] milk"));
        let line3 = t.find("see").unwrap();
        assert_eq!(action_at(t, byte_to_char(t, line3 + 6)), Some(Action::OpenNote("Other note".into())));
        assert_eq!(
            action_at(t, byte_to_char(t, t.find("example").unwrap())),
            Some(Action::OpenUrl("https://example.com".into()))
        );
        // cursor elsewhere on the line → first link
        assert_eq!(action_at(t, byte_to_char(t, line3)), Some(Action::OpenNote("Other note".into())));
        assert_eq!(action_at("[docs](https://a.b/c)", 2), Some(Action::OpenUrl("https://a.b/c".into())));
        assert_eq!(action_at("plain", 2), None);
    }

    #[test]
    fn continuation() {
        let t = "- [x] a";
        match list_continuation(t, 7) {
            Some(ListContinue::Continue(p)) => assert_eq!(p, "- [ ] "),
            _ => panic!(),
        }
        match list_continuation("9. foo", 6) {
            Some(ListContinue::Continue(p)) => assert_eq!(p, "10. "),
            _ => panic!(),
        }
        assert!(matches!(list_continuation("x\n- ", 4), Some(ListContinue::End { line_start_byte: 2 })));
        assert!(list_continuation("text", 4).is_none());
    }

    #[test]
    fn highlight_keeps_text() {
        let p = Palette::for_visuals(true);
        let src = "# Title\n- [ ] a **b** `c` [[d]] [e](f) https://g.h\n```\ncode\n```\n> q\n";
        let job = highlight(src, &p, 15.0, false);
        assert_eq!(job.text, src);
    }
}
