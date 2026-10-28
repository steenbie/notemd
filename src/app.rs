use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use eframe::egui::{
    self, text::CCursor, text::CCursorRange, Align, Color32, CornerRadius, FontId, Id, Key, KeyboardShortcut,
    Layout, Margin, Modifiers, RichText, Sense, Stroke, Vec2,
};

use crate::config::{Config, ThemeMode};
use crate::markdown::{self, Action, ListContinue, Palette};
use crate::store::{NoteStore, SearchHit};

const AUTOSAVE_DELAY: Duration = Duration::from_millis(350);
const REFRESH_EVERY: Duration = Duration::from_millis(1500);

fn search_id() -> Id {
    Id::unique("nn-search")
}
fn editor_id() -> Id {
    Id::unique("nn-editor")
}

struct OpenNote {
    name: String,
    path: PathBuf,
    text: String,
    dirty: bool,
    last_edit: Instant,
    disk_mtime: SystemTime,
}

#[derive(PartialEq)]
enum Row {
    Note(usize),
    Create,
}

enum Modal {
    None,
    Rename(String),
    Delete,
    Settings { dir: String },
    Help,
}

#[derive(Clone, Copy, PartialEq)]
enum Focus {
    /// Focus the search box and select its text.
    Search,
    /// Focus the editor and put the cursor at the end.
    EditorEnd,
    /// Focus the editor, keep the cursor where it is.
    Editor,
}

pub struct NimbleApp {
    cfg: Config,
    store: NoteStore,
    query: String,
    hits: Vec<SearchHit>,
    rows: Vec<Row>,
    selected: usize,
    open: Option<OpenNote>,
    pending_focus: Option<Focus>,
    scroll_to_selected: bool,
    last_refresh: Instant,
    modal: Modal,
    status: Option<(String, Instant, bool)>,
    applied_dark: Option<bool>,
    title: String,
    search_had_focus: bool,
}

impl NimbleApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let cfg = Config::load();
        let store = NoteStore::new(cfg.notes_dir.clone());
        let mut app = Self {
            cfg,
            store,
            query: String::new(),
            hits: Vec::new(),
            rows: Vec::new(),
            selected: 0,
            open: None,
            pending_focus: Some(Focus::Search),
            scroll_to_selected: false,
            last_refresh: Instant::now(),
            modal: Modal::None,
            status: None,
            applied_dark: None,
            title: String::new(),
            search_had_focus: true,
        };
        app.run_search(false);
        cc.egui_ctx.options_mut(|o| o.zoom_with_keyboard = true);
        app
    }

    // ---------------------------------------------------------------- notes

    fn run_search(&mut self, keep_selection: bool) {
        let prev = if keep_selection { self.selected_path() } else { None };
        self.hits = self.store.search(&self.query);
        self.rows = self.hits.iter().map(|h| Row::Note(h.index)).collect();
        let q = self.query.trim();
        if !q.is_empty() && self.store.find_exact(q).is_none() {
            // "a name that doesn't already exist": offer creation first when no note *name* matches.
            let name_match = self.hits.iter().any(|h| h.score >= 1_000);
            if name_match {
                self.rows.push(Row::Create);
            } else {
                self.rows.insert(0, Row::Create);
            }
        }
        self.selected = prev
            .and_then(|p| self.rows.iter().position(|r| matches!(r, Row::Note(i) if self.store.notes[*i].path == p)))
            .unwrap_or(0);
        self.scroll_to_selected = true;
    }

    fn selected_path(&self) -> Option<PathBuf> {
        match self.rows.get(self.selected) {
            Some(Row::Note(i)) => Some(self.store.notes[*i].path.clone()),
            _ => None,
        }
    }

    /// While browsing results, show the highlighted note in the editor.
    fn preview_selected(&mut self) {
        match self.rows.get(self.selected) {
            Some(Row::Note(i)) => {
                let path = self.store.notes[*i].path.clone();
                if self.open.as_ref().map(|o| o.path != path).unwrap_or(true) {
                    self.open_path(path);
                }
            }
            Some(Row::Create) => {
                self.flush_save();
                self.open = None;
            }
            None => {}
        }
    }

    fn open_path(&mut self, path: PathBuf) {
        self.flush_save();
        if let Some(n) = self.store.notes.iter().find(|n| n.path == path) {
            self.open = Some(OpenNote {
                name: n.name.clone(),
                path: n.path.clone(),
                text: n.content.clone(),
                dirty: false,
                last_edit: Instant::now(),
                disk_mtime: n.modified,
            });
        }
    }

    fn activate_selected(&mut self) {
        match self.rows.get(self.selected) {
            Some(Row::Note(i)) => {
                let path = self.store.notes[*i].path.clone();
                self.open_path(path);
                self.pending_focus = Some(Focus::EditorEnd);
            }
            Some(Row::Create) => self.create_from_query(),
            None => {}
        }
    }

    fn create_from_query(&mut self) {
        let name = self.query.trim().to_string();
        if name.is_empty() {
            return;
        }
        self.open_or_create(&name);
    }

    fn open_or_create(&mut self, name: &str) {
        self.flush_save();
        let path = match self.store.find_exact(name) {
            Some(i) => self.store.notes[i].path.clone(),
            None => match self.store.create(name) {
                Ok(p) => {
                    self.flash(format!("Created “{name}”"), false);
                    p
                }
                Err(e) => {
                    self.flash(format!("Could not create note: {e}"), true);
                    return;
                }
            },
        };
        self.open_path(path);
        self.query = self.open.as_ref().map(|o| o.name.clone()).unwrap_or_default();
        self.run_search(false);
        self.pending_focus = Some(Focus::EditorEnd);
    }

    fn flush_save(&mut self) {
        let Some(o) = self.open.as_mut() else { return };
        if !o.dirty {
            return;
        }
        match self.store.save(&o.path, &o.text) {
            Ok(m) => {
                o.dirty = false;
                o.disk_mtime = m;
            }
            Err(e) => {
                let msg = format!("Save failed: {e}");
                self.flash(msg, true);
            }
        }
    }

    fn refresh_from_disk(&mut self) {
        self.store.refresh();
        self.run_search(true);
        // Pick up edits made to the open note by another program (only if we have no unsaved changes).
        if let Some(o) = self.open.as_mut() {
            match self.store.notes.iter().find(|n| n.path == o.path) {
                Some(n) if !o.dirty && n.modified > o.disk_mtime => {
                    o.text = n.content.clone();
                    o.disk_mtime = n.modified;
                }
                None if !o.path.exists() => {
                    self.open = None;
                }
                _ => {}
            }
        }
    }

    fn flash(&mut self, msg: impl Into<String>, error: bool) {
        self.status = Some((msg.into(), Instant::now(), error));
    }

    fn set_notes_dir(&mut self, dir: PathBuf) {
        self.flush_save();
        self.open = None;
        self.cfg.notes_dir = dir.clone();
        self.cfg.save();
        self.store = NoteStore::new(dir);
        self.query.clear();
        self.run_search(false);
    }

    // ---------------------------------------------------------------- theme

    fn apply_theme(&mut self, ctx: &egui::Context) {
        let system_dark = ctx.system_theme().map(|t| t == egui::Theme::Dark).unwrap_or(false);
        let dark = match self.cfg.theme {
            ThemeMode::System => system_dark,
            ThemeMode::Opposite => !system_dark,
            ThemeMode::Light => false,
            ThemeMode::Dark => true,
        };
        if self.applied_dark == Some(dark) {
            return;
        }
        self.applied_dark = Some(dark);
        let mut v = if dark { egui::Visuals::dark() } else { egui::Visuals::light() };
        let accent = Palette::for_visuals(dark).accent;
        if dark {
            v.panel_fill = Color32::from_rgb(0x1E, 0x21, 0x27);
            v.window_fill = Color32::from_rgb(0x25, 0x29, 0x30);
            v.extreme_bg_color = Color32::from_rgb(0x17, 0x19, 0x1E);
            v.faint_bg_color = Color32::from_rgb(0x2A, 0x2E, 0x36);
        } else {
            v.panel_fill = Color32::from_rgb(0xFB, 0xFB, 0xFC);
            v.window_fill = Color32::WHITE;
            v.extreme_bg_color = Color32::WHITE;
            v.faint_bg_color = Color32::from_rgb(0xF1, 0xF3, 0xF5);
        }
        v.selection.bg_fill = accent.gamma_multiply(0.35);
        v.selection.stroke = Stroke::new(1.0, accent);
        v.text_cursor.stroke = Stroke::new(2.0, accent);
        v.widgets.hovered.bg_stroke = Stroke::new(1.0, accent.gamma_multiply(0.6));
        v.window_corner_radius = CornerRadius::same(10);
        ctx.set_visuals(v);
    }

    fn palette(&self) -> Palette {
        Palette::for_visuals(self.applied_dark.unwrap_or(false))
    }

    // ---------------------------------------------------------------- keys

    fn handle_global_keys(&mut self, ctx: &egui::Context) {
        if !matches!(self.modal, Modal::None) {
            return;
        }
        let ctrl = Modifiers::COMMAND;
        let ctrl_shift = Modifiers::COMMAND | Modifiers::SHIFT;
        let (esc, focus_search, new_note, rename, delete, settings, help, theme, folder) = ctx.input_mut(|i| {
            (
                i.consume_key(Modifiers::NONE, Key::Escape),
                i.consume_key(ctrl, Key::L) || i.consume_key(ctrl, Key::K) || i.consume_key(ctrl, Key::F),
                i.consume_key(ctrl, Key::N),
                i.consume_key(Modifiers::NONE, Key::F2),
                i.consume_shortcut(&KeyboardShortcut::new(ctrl_shift, Key::Delete))
                    || i.consume_shortcut(&KeyboardShortcut::new(ctrl_shift, Key::Backspace)),
                i.consume_key(ctrl, Key::Comma),
                i.consume_key(Modifiers::NONE, Key::F1),
                i.consume_key(ctrl_shift, Key::T),
                i.consume_key(ctrl_shift, Key::O),
            )
        });
        let search_focused = ctx.memory(|m| m.has_focus(search_id()));

        if esc {
            if search_focused && !self.query.is_empty() {
                self.query.clear();
                self.run_search(false);
            }
            self.flush_save();
            self.pending_focus = Some(Focus::Search);
        }
        if focus_search {
            self.flush_save();
            self.pending_focus = Some(Focus::Search);
        }
        if new_note {
            self.flush_save();
            self.query.clear();
            self.run_search(false);
            self.pending_focus = Some(Focus::Search);
        }
        if rename {
            if let Some(o) = &self.open {
                self.modal = Modal::Rename(o.name.clone());
            }
        }
        if delete && self.open.is_some() {
            self.modal = Modal::Delete;
        }
        if settings {
            self.modal = Modal::Settings { dir: self.cfg.notes_dir.display().to_string() };
        }
        if help {
            self.modal = Modal::Help;
        }
        if theme {
            self.cfg.theme = self.cfg.theme.next();
            self.cfg.save();
            self.flash(format!("Theme: {}", self.cfg.theme.label()), false);
        }
        if folder {
            let _ = open::that(&self.cfg.notes_dir);
        }
    }

    // ---------------------------------------------------------------- UI pieces

    fn search_bar(&mut self, ui: &mut egui::Ui) {
        let had_focus = self.search_had_focus;
        if had_focus && !self.rows.is_empty() {
            let (up, down, pgup, pgdn, enter, force_create) = ui.input_mut(|i| {
                (
                    i.consume_key(Modifiers::NONE, Key::ArrowUp),
                    i.consume_key(Modifiers::NONE, Key::ArrowDown),
                    i.consume_key(Modifiers::NONE, Key::PageUp),
                    i.consume_key(Modifiers::NONE, Key::PageDown),
                    i.consume_key(Modifiers::NONE, Key::Enter),
                    i.consume_key(Modifiers::COMMAND, Key::Enter) || i.consume_key(Modifiers::SHIFT, Key::Enter),
                )
            });
            let n = self.rows.len();
            let before = self.selected;
            if up {
                self.selected = (self.selected + n - 1) % n;
            }
            if down {
                self.selected = (self.selected + 1) % n;
            }
            if pgup {
                self.selected = self.selected.saturating_sub(8);
            }
            if pgdn {
                self.selected = (self.selected + 8).min(n - 1);
            }
            if self.selected != before {
                self.scroll_to_selected = true;
                self.preview_selected();
            }
            if force_create {
                self.create_from_query();
            } else if enter {
                self.activate_selected();
            }
        }

        let hint = if self.store.notes.is_empty() {
            "Type a name and press Enter to create your first note…"
        } else {
            "Search or create…"
        };
        let edit = egui::TextEdit::singleline(&mut self.query)
            .id(search_id())
            .hint_text(RichText::new(hint).size(self.cfg.font_size + 5.0))
            .font(FontId::proportional(self.cfg.font_size + 5.0))
            .desired_width(f32::INFINITY)
            .margin(Margin::symmetric(12, 10))
            .return_key(None);
        let resp = ui.add(edit);
        if resp.changed() {
            self.run_search(false);
            self.preview_selected();
        }
        if self.pending_focus == Some(Focus::Search) {
            resp.request_focus();
            if let Some(mut st) = egui::TextEdit::load_state(ui.ctx(), search_id()) {
                let end = self.query.chars().count();
                st.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(0), CCursor::new(end))));
                st.store(ui.ctx(), search_id());
            }
            self.pending_focus = None;
        }
        self.search_had_focus = resp.has_focus();
    }

    fn results(&mut self, ui: &mut egui::Ui) {
        let p = self.palette();
        let mut clicked: Option<usize> = None;
        let row_h = self.cfg.font_size * 2.6;
        egui::ScrollArea::vertical().id_salt("results").auto_shrink([false, true]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            for (ri, row) in self.rows.iter().enumerate() {
                let selected = ri == self.selected;
                let (rect, resp) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), row_h), Sense::click());
                if selected {
                    ui.painter().rect_filled(rect, 6.0, p.accent.gamma_multiply(0.18));
                    ui.painter().rect_filled(
                        egui::Rect::from_min_size(rect.min, Vec2::new(3.0, rect.height())),
                        2.0,
                        p.accent,
                    );
                } else if resp.hovered() {
                    ui.painter().rect_filled(rect, 6.0, ui.visuals().faint_bg_color);
                }
                let x = rect.left() + 14.0;
                let fs = self.cfg.font_size;
                match row {
                    Row::Note(i) => {
                        let n = &self.store.notes[*i];
                        let sub = self
                            .hits
                            .iter()
                            .find(|h| h.index == *i)
                            .and_then(|h| h.snippet.clone())
                            .unwrap_or_else(|| n.preview());
                        let has_sub = !sub.is_empty();
                        let title_y = if has_sub { rect.top() + row_h * 0.30 } else { rect.center().y };
                        ui.painter().text(
                            egui::pos2(x, title_y),
                            egui::Align2::LEFT_CENTER,
                            &n.name,
                            FontId::proportional(fs),
                            if selected { p.strong } else { p.text },
                        );
                        if has_sub {
                            ui.painter().text(
                                egui::pos2(x, rect.top() + row_h * 0.70),
                                egui::Align2::LEFT_CENTER,
                                sub,
                                FontId::proportional(fs * 0.82),
                                p.dim,
                            );
                        }
                        ui.painter().text(
                            egui::pos2(rect.right() - 12.0, title_y),
                            egui::Align2::RIGHT_CENTER,
                            ago(n.modified),
                            FontId::proportional(fs * 0.78),
                            p.dim,
                        );
                    }
                    Row::Create => {
                        ui.painter().text(
                            egui::pos2(x, rect.center().y),
                            egui::Align2::LEFT_CENTER,
                            format!("+  Create “{}”", self.query.trim()),
                            FontId::proportional(fs),
                            p.accent,
                        );
                        ui.painter().text(
                            egui::pos2(rect.right() - 12.0, rect.center().y),
                            egui::Align2::RIGHT_CENTER,
                            if selected { "Enter" } else { "Ctrl+Enter" },
                            FontId::proportional(fs * 0.78),
                            p.dim,
                        );
                    }
                }
                if selected && self.scroll_to_selected {
                    resp.scroll_to_me(None);
                }
                if resp.clicked() {
                    clicked = Some(ri);
                }
            }
            if self.rows.is_empty() {
                ui.add_space(8.0);
                ui.label(RichText::new("No notes yet").color(p.dim));
            }
        });
        self.scroll_to_selected = false;
        if let Some(ri) = clicked {
            self.selected = ri;
            self.activate_selected();
        }
    }

    fn editor(&mut self, ui: &mut egui::Ui) {
        let p = self.palette();
        let size = self.cfg.font_size;
        let mono = self.cfg.monospace_editor;
        let ctx = ui.ctx().clone();
        let focused = ctx.memory(|m| m.has_focus(editor_id()));

        // --- markdown actions & list continuation (handled before the TextEdit sees the keys)
        let mut action_result: Option<Action> = None;
        if focused {
            if let Some(o) = self.open.as_mut() {
                let state = egui::TextEdit::load_state(&ctx, editor_id());
                let range = state.as_ref().and_then(|s| s.cursor.char_range());
                let (ctrl_enter, enter) = ctx.input_mut(|i| {
                    let ce = i.consume_key(Modifiers::COMMAND, Key::Enter);
                    let e = range.map(|r| r.primary == r.secondary).unwrap_or(false)
                        && i.key_pressed(Key::Enter)
                        && i.modifiers.is_none()
                        && range.map(|r| markdown::list_continuation(&o.text, r.primary.index.0).is_some()).unwrap_or(false)
                        && i.consume_key(Modifiers::NONE, Key::Enter);
                    (ce, e)
                });
                if let (Some(r), Some(mut st)) = (range, state) {
                    let cur = r.primary.index.0;
                    let mut new_cursor = None;
                    if ctrl_enter {
                        match markdown::action_at(&o.text, cur) {
                            Some(Action::ToggleCheck { byte, checked }) => {
                                o.text = markdown::toggle_check(&o.text, byte, checked);
                                o.dirty = true;
                                o.last_edit = Instant::now();
                            }
                            Some(a) => action_result = Some(a),
                            None => {}
                        }
                    } else if enter {
                        match markdown::list_continuation(&o.text, cur) {
                            Some(ListContinue::Continue(prefix)) => {
                                let b = markdown::char_to_byte(&o.text, cur);
                                let ins = format!("{}{}", markdown::newline_for(&o.text), prefix);
                                o.text.insert_str(b, &ins);
                                new_cursor = Some(cur + ins.chars().count());
                            }
                            Some(ListContinue::End { line_start_byte }) => {
                                let b = markdown::char_to_byte(&o.text, cur);
                                o.text.replace_range(line_start_byte..b, "");
                                new_cursor = Some(markdown::byte_to_char(&o.text, line_start_byte));
                            }
                            None => {}
                        }
                        o.dirty = true;
                        o.last_edit = Instant::now();
                    }
                    if let Some(c) = new_cursor {
                        st.cursor.set_char_range(Some(CCursorRange::one(CCursor::new(c))));
                        st.store(&ctx, editor_id());
                    }
                }
            }
        }
        match action_result {
            Some(Action::OpenNote(name)) => {
                self.open_or_create(&name);
                return;
            }
            Some(Action::OpenUrl(url)) => {
                let looks_external = url.contains("://") || url.starts_with("mailto:") || url.starts_with("www.");
                if looks_external {
                    let target = if url.starts_with("www.") { format!("https://{url}") } else { url.clone() };
                    if let Err(e) = open::that(&target) {
                        self.flash(format!("Could not open link: {e}"), true);
                    }
                } else {
                    // Relative link → treat it as another note.
                    let name = url.trim_end_matches(".md").replace("%20", " ");
                    self.open_or_create(&name);
                    return;
                }
            }
            _ => {}
        }

        let Some(o) = self.open.as_mut() else {
            ui.centered_and_justified(|ui| {
                let msg = if self.query.trim().is_empty() {
                    "Type to search your notes, or a new name to create one.".to_string()
                } else {
                    format!("Press Enter to create “{}”", self.query.trim())
                };
                ui.label(RichText::new(msg).color(p.dim).size(size));
            });
            return;
        };

        let mut layouter = |ui: &egui::Ui, buf: &dyn egui::TextBuffer, wrap_width: f32| -> Arc<egui::Galley> {
            let mut job = markdown::highlight(buf.as_str(), &p, size, mono);
            job.wrap.max_width = wrap_width;
            ui.fonts_mut(|f| f.layout_job(job))
        };

        let avail = ui.available_size();
        let out = egui::ScrollArea::vertical().id_salt("editor-scroll").auto_shrink([false, false]).show(ui, |ui| {
            egui::TextEdit::multiline(&mut o.text)
                .id(editor_id())
                .frame(egui::Frame::NONE.inner_margin(Margin::symmetric(22, 16)))
                .lock_focus(true)
                .desired_width(f32::INFINITY)
                .min_size(Vec2::new(avail.x, avail.y - 4.0))
                .layouter(&mut layouter)
                .show(ui)
        });
        let resp = out.inner.response;
        if resp.changed() {
            o.dirty = true;
            o.last_edit = Instant::now();
        }
        match self.pending_focus {
            Some(Focus::EditorEnd) => {
                resp.request_focus();
                if let Some(mut st) = egui::TextEdit::load_state(&ctx, editor_id()) {
                    let end = o.text.chars().count();
                    st.cursor.set_char_range(Some(CCursorRange::one(CCursor::new(end))));
                    st.store(&ctx, editor_id());
                }
                self.pending_focus = None;
            }
            Some(Focus::Editor) => {
                resp.request_focus();
                self.pending_focus = None;
            }
            _ => {}
        }
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        let p = self.palette();
        let small = self.cfg.font_size * 0.8;
        ui.horizontal(|ui| {
            if let Some((msg, at, err)) = &self.status {
                if at.elapsed() < Duration::from_secs(4) {
                    ui.label(RichText::new(msg).size(small).color(if *err { Color32::from_rgb(0xE5, 0x48, 0x4D) } else { p.accent }));
                } else {
                    self.status = None;
                }
            }
            if self.status.is_none() {
                let text = match &self.open {
                    Some(o) => format!("{}  ·  {}", o.name, if o.dirty { "editing…" } else { "saved" }),
                    None => format!("{} notes  ·  {}", self.store.notes.len(), self.cfg.notes_dir.display()),
                };
                ui.label(RichText::new(text).size(small).color(p.dim));
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let hints = if self.search_had_focus {
                    "Up/Down select   Enter open   Ctrl+Enter create   F1 help"
                } else {
                    "Esc search   Ctrl+Enter action   F2 rename   F1 help"
                };
                ui.label(RichText::new(hints).size(small).color(p.dim));
            });
        });
    }

    fn modals(&mut self, ctx: &egui::Context) {
        let p = self.palette();
        match std::mem::replace(&mut self.modal, Modal::None) {
            Modal::None => {}
            Modal::Rename(mut name) => {
                let mut keep = true;
                egui::Modal::new(Id::unique("rename")).show(ctx, |ui| {
                    ui.set_width(380.0);
                    ui.heading("Rename note");
                    ui.add_space(6.0);
                    let r = ui.add(egui::TextEdit::singleline(&mut name).desired_width(f32::INFINITY).return_key(None));
                    r.request_focus();
                    ui.add_space(6.0);
                    ui.label(RichText::new("Enter to rename · Esc to cancel").color(p.dim).small());
                    let (enter, esc) = ui.input_mut(|i| (i.consume_key(Modifiers::NONE, Key::Enter), i.consume_key(Modifiers::NONE, Key::Escape)));
                    if esc {
                        keep = false;
                    }
                    if enter && !name.trim().is_empty() {
                        keep = false;
                        self.flush_save();
                        if let Some(o) = self.open.as_ref() {
                            let from = o.path.clone();
                            match self.store.rename(&from, name.trim()) {
                                Ok(to) => {
                                    self.open_path(to);
                                    self.query = self.open.as_ref().map(|o| o.name.clone()).unwrap_or_default();
                                    self.run_search(false);
                                    self.flash("Renamed", false);
                                }
                                Err(e) => self.flash(format!("Rename failed: {e}"), true),
                            }
                        }
                        self.pending_focus = Some(Focus::Editor);
                    }
                });
                if keep {
                    self.modal = Modal::Rename(name);
                } else if self.pending_focus.is_none() {
                    self.pending_focus = Some(Focus::Editor);
                }
            }
            Modal::Delete => {
                let mut keep = true;
                let name = self.open.as_ref().map(|o| o.name.clone()).unwrap_or_default();
                egui::Modal::new(Id::unique("delete")).show(ctx, |ui| {
                    ui.set_width(380.0);
                    ui.heading(format!("Delete “{name}”?"));
                    ui.add_space(4.0);
                    ui.label("The note will be moved to the Recycle Bin.");
                    ui.add_space(8.0);
                    let (enter, esc) = ui.input_mut(|i| (i.consume_key(Modifiers::NONE, Key::Enter), i.consume_key(Modifiers::NONE, Key::Escape)));
                    let mut del = enter;
                    ui.horizontal(|ui| {
                        del |= ui.button("Delete (Enter)").clicked();
                        if ui.button("Cancel (Esc)").clicked() || esc {
                            keep = false;
                        }
                    });
                    if del {
                        keep = false;
                        if let Some(o) = self.open.take() {
                            match self.store.delete(&o.path) {
                                Ok(()) => self.flash(format!("Deleted “{}”", o.name), false),
                                Err(e) => {
                                    self.flash(format!("Delete failed: {e}"), true);
                                    self.open = Some(o);
                                }
                            }
                        }
                        self.query.clear();
                        self.run_search(false);
                        self.pending_focus = Some(Focus::Search);
                    }
                });
                if keep {
                    self.modal = Modal::Delete;
                }
            }
            Modal::Settings { mut dir } => {
                let mut keep = true;
                egui::Modal::new(Id::unique("settings")).show(ctx, |ui| {
                    ui.set_width(460.0);
                    ui.heading("Settings");
                    ui.add_space(8.0);
                    ui.label("Notes folder");
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut dir).desired_width(300.0));
                        if cfg!(windows) && ui.button("Browse…").clicked() {
                            if let Some(d) = pick_folder(&dir) {
                                dir = d;
                            }
                        }
                        if ui.button("Open").on_hover_text("Open the folder in Explorer (Ctrl+Shift+O)").clicked() {
                            let _ = open::that(&self.cfg.notes_dir);
                        }
                    });
                    ui.add_space(10.0);
                    egui::Grid::new("settings-grid").num_columns(2).spacing([16.0, 10.0]).show(ui, |ui| {
                        ui.label("Theme");
                        egui::ComboBox::from_id_salt("theme").selected_text(self.cfg.theme.label()).show_ui(ui, |ui| {
                            for m in ThemeMode::ALL {
                                ui.selectable_value(&mut self.cfg.theme, m, m.label());
                            }
                        });
                        ui.end_row();
                        ui.label("Font size");
                        ui.add(egui::Slider::new(&mut self.cfg.font_size, 11.0..=26.0).step_by(1.0));
                        ui.end_row();
                        ui.label("Editor font");
                        ui.checkbox(&mut self.cfg.monospace_editor, "Monospace");
                        ui.end_row();
                    });
                    ui.add_space(12.0);
                    let esc = ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape));
                    ui.horizontal(|ui| {
                        if ui.button("Done").clicked() || esc {
                            keep = false;
                        }
                    });
                });
                // Theme/font changes apply live.
                self.applied_dark = None;
                if !keep {
                    let new_dir = PathBuf::from(dir.trim());
                    if !dir.trim().is_empty() && new_dir != self.cfg.notes_dir {
                        self.set_notes_dir(new_dir);
                    }
                    self.cfg.save();
                    self.pending_focus = Some(Focus::Search);
                } else {
                    self.modal = Modal::Settings { dir };
                }
            }
            Modal::Help => {
                let mut keep = true;
                egui::Modal::new(Id::unique("help")).show(ctx, |ui| {
                    ui.set_width(440.0);
                    ui.heading("Keyboard shortcuts");
                    ui.add_space(8.0);
                    egui::Grid::new("help-grid").num_columns(2).spacing([24.0, 6.0]).striped(true).show(ui, |ui| {
                        for (k, d) in [
                            ("Type + Enter", "Open the selected note, or create a new one"),
                            ("Up / Down", "Move through search results"),
                            ("Ctrl+Enter (search)", "Always create a note with the typed name"),
                            ("Esc", "Back to the search box (again to clear it)"),
                            ("Ctrl+L / Ctrl+K", "Back to the search box"),
                            ("Ctrl+N", "New note (clear search)"),
                            ("Ctrl+Enter (editor)", "Toggle checklist · open link · go to [[note]]"),
                            ("Enter in a list", "Continue the list / checklist"),
                            ("F2", "Rename the current note"),
                            ("Ctrl+Shift+Delete", "Delete the current note"),
                            ("Ctrl+Shift+T", "Cycle theme"),
                            ("Ctrl+Shift+O", "Open the notes folder"),
                            ("Ctrl+ + / Ctrl+ -", "Zoom"),
                            ("Ctrl+,", "Settings"),
                        ] {
                            ui.label(RichText::new(k).strong().color(p.accent));
                            ui.label(d);
                            ui.end_row();
                        }
                    });
                    ui.add_space(10.0);
                    ui.label(RichText::new("Notes are saved automatically as .md files.").color(p.dim));
                    let esc = ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape) || i.consume_key(Modifiers::NONE, Key::F1) || i.consume_key(Modifiers::NONE, Key::Enter));
                    if esc || ui.button("Close").clicked() {
                        keep = false;
                    }
                });
                if keep {
                    self.modal = Modal::Help;
                }
            }
        }
    }
}

impl eframe::App for NimbleApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = &ui.ctx().clone();
        self.apply_theme(ctx);

        // Autosave & periodic re-scan of the notes folder.
        if let Some(o) = &self.open {
            if o.dirty && o.last_edit.elapsed() >= AUTOSAVE_DELAY {
                self.flush_save();
            } else if o.dirty {
                ctx.request_repaint_after(AUTOSAVE_DELAY);
            }
        }
        if self.last_refresh.elapsed() >= REFRESH_EVERY {
            self.last_refresh = Instant::now();
            if self.open.as_ref().map(|o| !o.dirty).unwrap_or(true) {
                self.refresh_from_disk();
            }
        }
        ctx.request_repaint_after(REFRESH_EVERY);
        // Save when the window loses focus.
        if ctx.input(|i| !i.focused) {
            self.flush_save();
        }

        self.handle_global_keys(ctx);

        let title = match &self.open {
            Some(o) => format!("{} — nimblenote", o.name),
            None => "nimblenote".to_string(),
        };
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }

        let panel_fill = ctx.global_style().visuals.panel_fill;
        let sep = ctx.global_style().visuals.widgets.noninteractive.bg_stroke;

        egui::Panel::top("search")
            .frame(egui::Frame::new().fill(panel_fill).inner_margin(Margin { left: 14, right: 14, top: 14, bottom: 10 }))
            .show_separator_line(false)
            .show(ui, |ui| {
                self.search_bar(ui);
            });

        egui::Panel::bottom("status")
            .frame(egui::Frame::new().fill(panel_fill).inner_margin(Margin::symmetric(14, 6)).stroke(sep))
            .show_separator_line(false)
            .show(ui, |ui| self.status_bar(ui));

        if self.search_had_focus {
            let row_h = self.cfg.font_size * 2.6 + 2.0;
            let want = (self.rows.len().max(1) as f32) * row_h + 10.0;
            let max_h = (ctx.content_rect().height() * 0.42).min(want);
            egui::Panel::top("results")
                .frame(egui::Frame::new().fill(panel_fill).inner_margin(Margin { left: 14, right: 14, top: 0, bottom: 8 }))
                .show_separator_line(true)
                .resizable(false)
                .exact_size(max_h)
                .show(ui, |ui| self.results(ui));
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(ctx.global_style().visuals.extreme_bg_color))
            .show(ui, |ui| self.editor(ui));

        self.modals(ctx);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.flush_save();
        self.cfg.save();
    }
}

fn ago(t: SystemTime) -> String {
    let secs = SystemTime::now().duration_since(t).map(|d| d.as_secs()).unwrap_or(0);
    match secs {
        0..=59 => "just now".into(),
        60..=3599 => format!("{}m ago", secs / 60),
        3600..=86_399 => format!("{}h ago", secs / 3600),
        86_400..=604_799 => format!("{}d ago", secs / 86_400),
        _ => format!("{}w ago", secs / 604_800),
    }
}

#[cfg(windows)]
fn pick_folder(start: &str) -> Option<String> {
    rfd::FileDialog::new().set_directory(start).pick_folder().map(|d| d.display().to_string())
}

#[cfg(not(windows))]
fn pick_folder(_start: &str) -> Option<String> {
    None
}
