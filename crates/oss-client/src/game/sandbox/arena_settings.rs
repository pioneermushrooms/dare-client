//! The arena settings window (Esc, then E): the in-game settings editor.
//!
//! Continuum opens its own settings window when the zone sends a `server.set`
//! file in answer to `?quickfix`, one `Section:Key:Value:Min:Max:Help` line per
//! setting, and sends the changes back as C2S `0x1D`. This is the same exchange
//! with a window of our own, drawn in the game's box like the Options window:
//! sections down the left, the section's settings on the right, the picked
//! setting's help underneath. The zone decides who may open it and which
//! settings are listed; it checks every change again and answers in chat, so
//! the window sits at the top of the screen with the chat log in view below.
//!
//! The menu row is only there where the zone says the player may use it: on
//! the first Esc in each arena the client asks with `?settingseditor`, and the
//! zone's one-line answer (beginning [`PROBE_REPLY`]) is kept out of the chat.
//!
//! Keys: Up/Down pick a setting, Left/Right or Tab a section, PgUp/PgDn page,
//! Enter edits (or flips an on/off setting), typing starts an edit, Esc
//! cancels an edit or closes, Ctrl+S sends the changes. Clicks pick sections
//! and settings, and a second click on the picked setting edits it.

use winit::keyboard::KeyCode;

use super::Sandbox;
use crate::game::chat::wrap;
use crate::game::input::Stroke;
use crate::game::start_spots::Rect;
use crate::game::view::{Canvas, color, ui_color};
use crate::gfx::Renderer;

/// The start of the zone's answer to `?settingseditor`.
pub(super) const PROBE_REPLY: &str = "Arena settings editor: ";

/// What the client knows about the settings editor in the arena it is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum Access {
    /// Not asked yet in this arena.
    #[default]
    Unknown,
    /// Asked; the answer has not come.
    Asked,
    Yes,
    No,
}

/// Reads the zone's answer to `?settingseditor`, or `None` for any other line.
pub(super) fn probe_answer(text: &str) -> Option<bool> {
    let rest = text.strip_prefix(PROBE_REPLY)?;
    Some(rest.starts_with("available"))
}

/// One setting, as the zone listed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Row {
    pub section: String,
    pub key: String,
    pub value: String,
    pub min: Option<i64>,
    pub max: Option<i64>,
    pub help: String,
    /// The player's new value, not sent yet.
    pub edited: Option<String>,
}

impl Row {
    fn shown(&self) -> &str {
        self.edited.as_deref().unwrap_or(&self.value)
    }

    fn is_toggle(&self) -> bool {
        self.min == Some(0) && self.max == Some(1)
    }

    /// Checks a typed value against the range the zone gave; the zone checks again.
    fn check(&self, value: &str) -> Result<String, String> {
        let value = value.trim();
        if value.contains(':') {
            return Err("Values can't contain ':'.".into());
        }
        if let (Some(lo), Some(hi)) = (self.min, self.max) {
            let Ok(n) = value.parse::<i64>() else {
                return Err(format!(
                    "{} takes a whole number from {lo} to {hi}.",
                    self.key
                ));
            };
            if n < lo || n > hi {
                return Err(format!(
                    "{} takes {lo} to {hi}; {n} is out of range.",
                    self.key
                ));
            }
            return Ok(n.to_string());
        }
        Ok(value.to_string())
    }
}

/// Reads a `server.set` file. Lines that are not `Section:Key:Value:Min:Max:Help`
/// are skipped; the help may itself hold colons.
pub(super) fn parse(data: &[u8]) -> Vec<Row> {
    // Zones write the Windows code page; anything not UTF-8 is read as Latin-1.
    let text = match std::str::from_utf8(data) {
        Ok(t) => t.to_string(),
        Err(_) => data.iter().map(|&b| b as char).collect(),
    };
    text.lines()
        .filter_map(|line| {
            let mut parts = line.trim_end_matches('\r').splitn(6, ':');
            let section = parts.next()?.trim();
            let key = parts.next()?.trim();
            let value = parts.next()?;
            let min = parts.next()?.trim().parse().ok();
            let max = parts.next()?.trim().parse().ok();
            let help = parts.next().unwrap_or("").trim();
            if section.is_empty() || key.is_empty() {
                return None;
            }
            let value = if value == "<unset>" { "" } else { value };
            Some(Row {
                section: section.to_string(),
                key: key.to_string(),
                value: value.to_string(),
                min,
                max,
                help: help.to_string(),
                edited: None,
            })
        })
        .collect()
}

/// The zone sends a ship setting's help once, with the first ship, and points
/// the other seven at it (`As for Warbird:Key`), so the list arrives sooner.
/// This puts the text back.
fn fill_shared_help(rows: &mut [Row]) {
    let pointer = |h: &str| -> Option<(String, String)> {
        let rest = h.strip_prefix("As for ")?;
        let (s, k) = rest.split_once(':')?;
        Some((s.to_string(), k.to_string()))
    };
    let found: Vec<(usize, String)> = rows
        .iter()
        .enumerate()
        .filter_map(|(i, r)| {
            let (s, k) = pointer(&r.help)?;
            let text = rows
                .iter()
                .find(|o| o.section == s && o.key == k)?
                .help
                .clone();
            (pointer(&text).is_none()).then_some((i, text))
        })
        .collect();
    for (i, text) in found {
        rows[i].help = text;
    }
}

/// What a key or click asks the sandbox to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Outcome {
    None,
    Close,
    /// Send these `(section, key, value)` changes.
    Save(Vec<(String, String, String)>),
}

/// The window's state.
#[derive(Debug, Default)]
pub(super) struct Editor {
    rows: Vec<Row>,
    sections: Vec<String>,
    section: usize,
    /// The picked setting, within the section.
    pick: usize,
    /// The first setting shown, within the section.
    top: usize,
    /// The value being typed.
    edit: Option<String>,
    /// A line for the player under the list (an error, a reminder).
    status: Option<(String, u32)>,
    /// Esc was pressed once with changes unsent.
    warned: bool,
    /// How many settings the list has room for, from the last draw.
    lines: usize,
    spots: Vec<(Rect, Spot)>,
    /// The list is on its way: bytes so far and in all, once the zone says.
    loading: Option<(usize, usize)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Spot {
    Section(usize),
    Row(usize),
    Save,
    Close,
}

const COLS: f32 = 64.0;
const LEFT_COLS: f32 = 13.0;
const PITCH: f32 = 12.0;
const HELP_LINES: usize = 3;

impl Editor {
    /// An empty window waiting for the zone's list.
    pub(super) fn loading() -> Self {
        Self {
            lines: 12,
            loading: Some((0, 0)),
            ..Self::default()
        }
    }

    #[cfg(test)]
    fn new(rows: Vec<Row>) -> Self {
        let mut e = Self::loading();
        e.load(rows);
        e
    }

    /// Takes the zone's list. Settings the player has edited and not sent keep
    /// the new value; the rest show what the zone has now.
    pub(super) fn load(&mut self, mut rows: Vec<Row>) {
        fill_shared_help(&mut rows);
        for row in &mut rows {
            if let Some(old) = self
                .rows
                .iter()
                .find(|o| o.section == row.section && o.key == row.key)
            {
                row.edited = old.edited.clone().filter(|v| *v != row.value);
            }
        }
        let picked = self
            .picked()
            .map(|i| (self.rows[i].section.clone(), self.rows[i].key.clone()));
        let mut sections: Vec<String> = Vec::new();
        for r in &rows {
            if !sections.contains(&r.section) {
                sections.push(r.section.clone());
            }
        }
        self.rows = rows;
        self.sections = sections;
        self.loading = None;
        self.section = 0;
        self.pick = 0;
        self.top = 0;
        if let Some((section, key)) = picked
            && let Some(s) = self.sections.iter().position(|x| *x == section)
        {
            self.section = s;
            if let Some(p) = self.current().iter().position(|&i| self.rows[i].key == key) {
                self.move_pick(p as isize);
            }
        }
    }

    pub(super) fn progress(&mut self, received: usize, total: usize) {
        if self.loading.is_some() {
            self.loading = Some((received, total));
        }
    }

    pub(super) fn is_loading(&self) -> bool {
        self.loading.is_some()
    }

    /// Closed with changes unsent: they are dropped, the list is kept.
    pub(super) fn forget_edits(&mut self) {
        self.edit = None;
        self.warned = false;
        self.status = None;
        for r in &mut self.rows {
            r.edited = None;
        }
    }

    /// Indices into `rows` of the current section's settings.
    fn current(&self) -> Vec<usize> {
        let Some(name) = self.sections.get(self.section) else {
            return Vec::new();
        };
        (0..self.rows.len())
            .filter(|&i| &self.rows[i].section == name)
            .collect()
    }

    fn picked(&self) -> Option<usize> {
        self.current().get(self.pick).copied()
    }

    fn pending(&self) -> Vec<(String, String, String)> {
        self.rows
            .iter()
            .filter_map(|r| {
                let v = r.edited.as_ref()?;
                Some((r.section.clone(), r.key.clone(), v.clone()))
            })
            .collect()
    }

    fn set_section(&mut self, section: usize) {
        if section < self.sections.len() && section != self.section {
            self.commit_edit();
            self.section = section;
            self.pick = 0;
            self.top = 0;
        }
    }

    fn move_pick(&mut self, by: isize) {
        self.commit_edit();
        let n = self.current().len();
        if n == 0 {
            return;
        }
        self.pick = (self.pick as isize + by).clamp(0, n as isize - 1) as usize;
        let lines = self.lines.max(1);
        if self.pick < self.top {
            self.top = self.pick;
        } else if self.pick >= self.top + lines {
            self.top = self.pick + 1 - lines;
        }
    }

    /// Takes the typed value, if it is one the setting can hold.
    fn commit_edit(&mut self) -> bool {
        let Some(text) = self.edit.take() else {
            return true;
        };
        let Some(i) = self.picked() else {
            return true;
        };
        match self.rows[i].check(&text) {
            Ok(v) => {
                self.set_value(i, v);
                true
            }
            Err(e) => {
                self.status = Some((e, color::RED));
                self.edit = Some(text);
                false
            }
        }
    }

    fn set_value(&mut self, i: usize, v: String) {
        let row = &mut self.rows[i];
        row.edited = (v != row.value).then_some(v);
        self.status = None;
        self.warned = false;
    }

    fn activate(&mut self) {
        let Some(i) = self.picked() else { return };
        if self.rows[i].is_toggle() {
            let on = self.rows[i].shown().trim() == "1";
            self.set_value(i, if on { "0" } else { "1" }.to_string());
        } else {
            self.edit = Some(self.rows[i].shown().to_string());
        }
    }

    fn save(&mut self) -> Outcome {
        if !self.commit_edit() {
            return Outcome::None;
        }
        let changes = self.pending();
        if changes.is_empty() {
            self.status = Some(("Nothing changed yet.".into(), color::YELLOW));
            return Outcome::None;
        }
        for r in &mut self.rows {
            if let Some(v) = r.edited.take() {
                r.value = v;
            }
        }
        self.warned = false;
        let n = changes.len();
        self.status = Some((
            format!(
                "Sent {n} change{}; the zone's answer is in the chat below.",
                if n == 1 { "" } else { "s" }
            ),
            color::GREEN,
        ));
        Outcome::Save(changes)
    }

    fn close(&mut self) -> Outcome {
        if !self.pending().is_empty() && !self.warned {
            self.warned = true;
            self.status = Some((
                "Changes not sent: Ctrl+S sends them, Esc again throws them away.".into(),
                color::YELLOW,
            ));
            return Outcome::None;
        }
        Outcome::Close
    }

    pub(super) fn input(&mut self, strokes: &[Stroke], ctrl: bool) -> Outcome {
        for stroke in strokes {
            if ctrl && stroke.first_press() == Some(KeyCode::KeyS) {
                let out = self.save();
                if out != Outcome::None {
                    return out;
                }
                continue;
            }
            if let Some(buf) = self.edit.as_mut() {
                match stroke.key {
                    Some(KeyCode::Escape) if !stroke.repeat => {
                        self.edit = None;
                        self.status = None;
                    }
                    Some(KeyCode::Enter | KeyCode::NumpadEnter) if !stroke.repeat => {
                        self.commit_edit();
                    }
                    Some(KeyCode::Backspace) => {
                        buf.pop();
                    }
                    Some(KeyCode::ArrowUp) => self.move_pick(-1),
                    Some(KeyCode::ArrowDown) => self.move_pick(1),
                    _ => {
                        if let Some(c) = stroke.text.filter(|c| !c.is_control())
                            && buf.chars().count() < 200
                        {
                            buf.push(c);
                        }
                    }
                }
                continue;
            }
            match stroke.key {
                Some(KeyCode::Escape) if !stroke.repeat => {
                    let out = self.close();
                    if out != Outcome::None {
                        return out;
                    }
                }
                Some(KeyCode::ArrowUp) => self.move_pick(-1),
                Some(KeyCode::ArrowDown) => self.move_pick(1),
                Some(KeyCode::PageUp) => self.move_pick(-(self.lines.max(1) as isize)),
                Some(KeyCode::PageDown) => self.move_pick(self.lines.max(1) as isize),
                Some(KeyCode::ArrowLeft) => self.set_section(self.section.saturating_sub(1)),
                Some(KeyCode::ArrowRight | KeyCode::Tab) => {
                    let next = if self.section + 1 < self.sections.len() {
                        self.section + 1
                    } else if stroke.key == Some(KeyCode::Tab) {
                        0
                    } else {
                        self.section
                    };
                    self.set_section(next);
                }
                Some(KeyCode::Enter | KeyCode::NumpadEnter | KeyCode::Space) if !stroke.repeat => {
                    self.activate()
                }
                _ => {
                    // Typing a value starts editing it.
                    if let Some(c) = stroke.text.filter(|c| !c.is_control() && *c != ' ')
                        && let Some(i) = self.picked()
                        && !self.rows[i].is_toggle()
                    {
                        self.edit = Some(c.to_string());
                    }
                }
            }
        }
        Outcome::None
    }

    /// A click at (x, y); `true` when it landed on the window.
    pub(super) fn click(&mut self, x: f32, y: f32) -> (bool, Outcome) {
        let Some(&(_, spot)) = self.spots.iter().find(|(r, _)| r.holds(x, y)) else {
            return (false, Outcome::None);
        };
        let out = match spot {
            Spot::Section(s) => {
                self.set_section(s);
                Outcome::None
            }
            Spot::Row(p) => {
                if p == self.pick && self.edit.is_none() {
                    self.activate();
                } else {
                    self.move_pick(p as isize - self.pick as isize);
                }
                Outcome::None
            }
            Spot::Save => self.save(),
            Spot::Close => self.close(),
        };
        (true, out)
    }

    pub(super) fn draw(&mut self, c: &Canvas, r: &mut Renderer, clock: f32) {
        self.spots.clear();
        let w = (COLS * 8.0).min(c.view_w - 8.0).floor();
        let left_w = LEFT_COLS * 8.0;
        // The top part of the screen, so the chat log (where the zone answers)
        // stays in view beneath it.
        let room = ((c.view_h * 0.62 - 14.0 - 12.0 - (HELP_LINES as f32 + 2.0) * PITCH - 14.0)
            / PITCH)
            .floor();
        self.lines = (room.max(4.0) as usize).min(24);
        let list_h = self.lines as f32 * PITCH;
        let h = 16.0 + 12.0 + list_h + 6.0 + (HELP_LINES as f32 + 1.0) * PITCH + 16.0;
        let x = ((c.view_w - w) / 2.0).floor().max(4.0);
        let top = 4.0;
        c.panel(r, x, top, w, h);
        let pending = self.pending().len();
        let title = if pending > 0 {
            format!("-= Arena settings ({pending} unsent) =-")
        } else {
            "-= Arena settings =-".to_string()
        };
        c.text_center(r, x + w / 2.0, top + 1.0, color::GREEN, &title);
        c.separator(r, x, top + 13.0, w);
        let y0 = top + 18.0;

        // Sections, down the left (scrolled to keep the current one in view).
        c.text(r, x + 4.0, y0, color::DARK_RED, "Section");
        let first = self.section.saturating_sub(self.lines.saturating_sub(1));
        for (n, s) in self
            .sections
            .iter()
            .enumerate()
            .skip(first)
            .take(self.lines)
        {
            let y = y0 + 12.0 + (n - first) as f32 * PITCH;
            let rect = Rect::new(x + 2.0, y - 1.0, left_w - 4.0, 12.0);
            self.spots.push((rect, Spot::Section(n)));
            let changed = self
                .rows
                .iter()
                .any(|r| &r.section == s && r.edited.is_some());
            if n == self.section {
                c.color_rect(r, ui_color::HIGHLIGHT, rect.x, rect.y, rect.w, rect.h);
            }
            let colour = if changed {
                color::YELLOW
            } else if n == self.section {
                color::WHITE
            } else {
                color::GREEN
            };
            let name: String = s.chars().take(LEFT_COLS as usize - 1).collect();
            c.text(r, x + 4.0, y, colour, &name);
        }
        c.color_rect(
            r,
            ui_color::BORDER1,
            x + left_w + 1.0,
            y0 - 3.0,
            1.0,
            list_h + 15.0,
        );

        // The section's settings.
        let px = x + left_w + 8.0;
        let pw = x + w - 4.0 - px;
        let key_cols = 22usize;
        let vx = px + key_cols as f32 * 8.0;
        let value_cols = ((x + w - 6.0 - vx) / 8.0).max(4.0) as usize;
        c.text(r, px, y0, color::DARK_RED, "Setting");
        c.text(r, vx, y0, color::DARK_RED, "Value");
        let rows = self.current();
        for (n, &i) in rows.iter().enumerate().skip(self.top).take(self.lines) {
            let y = y0 + 12.0 + (n - self.top) as f32 * PITCH;
            let rect = Rect::new(px - 2.0, y - 1.0, pw, 12.0);
            self.spots.push((rect, Spot::Row(n)));
            let row = &self.rows[i];
            if n == self.pick {
                c.color_rect(r, ui_color::HIGHLIGHT, rect.x, rect.y, rect.w, rect.h);
            }
            let key: String = row.key.chars().take(key_cols - 1).collect();
            c.text(r, px, y, color::GREEN, &key);
            let editing = n == self.pick && self.edit.is_some();
            let (text, colour) = if editing {
                let caret = if clock.fract() < 0.5 { "_" } else { " " };
                (
                    format!("{}{caret}", self.edit.as_deref().unwrap_or("")),
                    color::WHITE,
                )
            } else if row.is_toggle() {
                let on = row.shown().trim() == "1";
                (
                    if on { "1  (on)" } else { "0  (off)" }.to_string(),
                    if row.edited.is_some() {
                        color::YELLOW
                    } else {
                        color::WHITE
                    },
                )
            } else {
                let shown = if row.shown().is_empty() {
                    "(unset)"
                } else {
                    row.shown()
                };
                (
                    shown.to_string(),
                    if row.edited.is_some() {
                        color::YELLOW
                    } else {
                        color::WHITE
                    },
                )
            };
            // Keep the end of a long value (where the caret is) in view.
            let chars: Vec<char> = text.chars().collect();
            let from = chars.len().saturating_sub(value_cols);
            let text: String = chars[from..].iter().collect();
            c.text(r, vx, y, colour, &text);
        }
        if let Some((got, total)) = self.loading.filter(|_| self.rows.is_empty()) {
            let line = match (got * 100).checked_div(total) {
                Some(pct) => format!("Loading the arena's settings... {pct}%"),
                None => "Asking the zone for the arena's settings...".to_string(),
            };
            c.text(r, px, y0 + 12.0, color::YELLOW, &line);
        } else if rows.is_empty() {
            c.text(r, px, y0 + 12.0, color::WHITE, "No settings here.");
        }
        if rows.len() > self.lines {
            let more = format!(
                "{}-{} of {}",
                self.top + 1,
                (self.top + self.lines).min(rows.len()),
                rows.len()
            );
            c.text_right(r, x + w - 4.0, y0, color::DARK_RED, &more);
        }

        // The picked setting's help (or the status line), under the list.
        let hy = y0 + 12.0 + list_h + 4.0;
        c.separator(r, x, hy - 3.0, w);
        let cols = ((w - 8.0) / 8.0) as usize;
        let mut lines: Vec<(String, u32)> = Vec::new();
        if let Some((msg, colour)) = &self.status {
            lines.extend(wrap(msg, cols, true).into_iter().map(|l| (l, *colour)));
        }
        if let Some(i) = self.picked() {
            let row = &self.rows[i];
            let mut head = format!("{}:{}", row.section, row.key);
            if row.edited.is_some() {
                head.push_str(&format!(
                    "  (was {})",
                    if row.value.is_empty() {
                        "unset"
                    } else {
                        &row.value
                    }
                ));
            }
            lines.push((head, color::WHITE));
            lines.extend(
                wrap(&row.help, cols, true)
                    .into_iter()
                    .map(|l| (l, color::GREEN)),
            );
        }
        for (n, (line, colour)) in lines.iter().take(HELP_LINES + 1).enumerate() {
            c.text(r, x + 4.0, hy + n as f32 * PITCH, *colour, line);
        }

        // The keys, which are buttons too.
        let fy = top + h - 13.0;
        let foot = if self.edit.is_some() {
            "Enter = Keep   Esc = Undo   Ctrl+S = Send"
        } else {
            "Arrows = Pick   Enter = Edit   Ctrl+S = Send   Esc = Close"
        };
        c.text(r, x + 4.0, fy, color::YELLOW, foot);
        if let Some(at) = foot.find("Ctrl+S") {
            let rect = Rect::new(x + 4.0 + at as f32 * 8.0, fy - 1.0, 13.0 * 8.0, 12.0);
            self.spots.push((rect, Spot::Save));
        }
        if let Some(at) = foot.find("Esc") {
            let rect = Rect::new(x + 4.0 + at as f32 * 8.0, fy - 1.0, 11.0 * 8.0, 12.0);
            self.spots.push((rect, Spot::Close));
        }
    }
}

impl Sandbox {
    /// E on the menu: open the window. The list is the zone's; it comes as a
    /// file, so the first time in an arena the window waits for it, and after
    /// that it opens with the list it has while a fresh one is fetched.
    pub(super) fn open_arena_settings(&mut self) {
        let editor = self.arena_settings.get_or_insert_with(Editor::loading);
        if !editor.is_loading() {
            editor.status = Some((
                "Checking for changes made elsewhere...".into(),
                color::YELLOW,
            ));
        }
        self.arena_settings_open = true;
        self.settings_wanted = true;
        self.send_chat_line("?quickfix");
    }

    /// On the first Esc in an arena: ask whether the settings editor is open to us.
    pub(super) fn probe_settings_access(&mut self) {
        if self.online() && self.settings_access == Access::Unknown {
            self.settings_access = Access::Asked;
            self.send_chat_line("?settingseditor");
        }
    }

    /// A zone message; `true` when it is one of ours and stays out of the chat:
    /// the answer to `?settingseditor`, or the zone saying it is sending the
    /// list the window asked for (the window shows that itself).
    pub(super) fn settings_chat(&mut self, text: &str) -> bool {
        if self.settings_access == Access::Asked
            && let Some(yes) = probe_answer(text)
        {
            self.settings_access = if yes { Access::Yes } else { Access::No };
            return true;
        }
        self.settings_wanted && text.starts_with("Sending ") && text.ends_with(" settings...")
    }

    /// A transfer with no map or LVZ name: the settings list, while one is awaited.
    pub(super) fn settings_progress(&mut self, name: &str, received: usize, total: usize) {
        if self.settings_wanted
            && name.is_empty()
            && let Some(editor) = self.arena_settings.as_mut()
        {
            editor.progress(received, total);
        }
    }

    /// A file the zone sent; `server.set` fills (and opens) the window.
    pub(super) fn incoming_file(&mut self, name: &str, data: &[u8]) {
        if !name.eq_ignore_ascii_case("server.set") {
            log::debug!("ignoring an incoming file: {name}");
            return;
        }
        let rows = parse(data);
        self.settings_wanted = false;
        if rows.is_empty() {
            self.message("The zone sent no settings to edit.", color::RED);
            return;
        }
        // The zone only sends the list to someone who may edit it.
        self.settings_access = Access::Yes;
        let editor = self.arena_settings.get_or_insert_with(Editor::loading);
        let refreshed = !editor.is_loading();
        editor.load(rows);
        if refreshed {
            editor.status = None;
        }
        // Typed ?quickfix opens it too, as it does in Continuum.
        self.menu = false;
        self.arena_settings_open = true;
    }

    /// Keys while the window is up. It takes them all.
    pub(super) fn arena_settings_input(&mut self, strokes: &[Stroke], ctrl: bool) {
        let Some(editor) = self.arena_settings.as_mut() else {
            self.arena_settings_open = false;
            return;
        };
        let out = editor.input(strokes, ctrl);
        self.arena_settings_outcome(out);
    }

    pub(super) fn arena_settings_outcome(&mut self, out: Outcome) {
        match out {
            Outcome::None => {}
            Outcome::Close => {
                self.arena_settings_open = false;
                if let Some(editor) = self.arena_settings.as_mut() {
                    editor.forget_edits();
                }
            }
            Outcome::Save(changes) => {
                if let Some(net) = self.net.as_mut() {
                    net.session.send_setting_changes(&changes);
                }
            }
        }
    }

    pub(super) fn draw_arena_settings(&mut self, c: &Canvas, r: &mut Renderer) {
        let clock = self.clock;
        if let Some(editor) = self.arena_settings.as_mut() {
            editor.draw(c, r, clock);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows() -> Vec<Row> {
        parse(
            b"Soccer:BallBounce:1:0:1:Whether the ball bounces off walls [0 (off) or 1 (on)]\r\n\
              Soccer:Mode:All:::Goal configuration: All = any\r\n\
              Warbird:BulletSpeed:2000:-32768:32767:How fast bullets travel\r\n\
              Misc:GreetMessage:<unset>:::Greeting\r\n\
              broken line\r\n",
        )
    }

    fn key(k: KeyCode) -> Stroke {
        Stroke::press(k)
    }

    fn typed(s: &str) -> Vec<Stroke> {
        s.chars().map(Stroke::text).collect()
    }

    #[test]
    fn reads_server_set_and_keeps_colons_in_the_help() {
        let rows = rows();
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[1].help, "Goal configuration: All = any");
        assert_eq!((rows[1].min, rows[1].max), (None, None));
        assert_eq!((rows[2].min, rows[2].max), (Some(-32768), Some(32767)));
        assert_eq!(rows[3].value, "");
        assert!(rows[0].is_toggle());
    }

    #[test]
    fn the_probe_answer_is_recognised_and_nothing_else() {
        assert_eq!(
            probe_answer("Arena settings editor: available (Esc then E, or ?quickfix)."),
            Some(true)
        );
        assert_eq!(
            probe_answer("Arena settings editor: not available here."),
            Some(false)
        );
        assert_eq!(probe_answer("hello"), None);
    }

    #[test]
    fn enter_flips_a_toggle_and_ctrl_s_sends_it() {
        let mut e = Editor::new(rows());
        assert_eq!(e.input(&[key(KeyCode::Enter)], false), Outcome::None);
        let out = e.input(&[key(KeyCode::KeyS)], true);
        assert_eq!(
            out,
            Outcome::Save(vec![("Soccer".into(), "BallBounce".into(), "0".into())])
        );
        // sent: nothing left to send
        assert_eq!(e.input(&[key(KeyCode::KeyS)], true), Outcome::None);
    }

    #[test]
    fn typing_edits_and_out_of_range_is_refused_here() {
        let mut e = Editor::new(rows());
        e.input(&[key(KeyCode::ArrowRight)], false); // Warbird
        let mut strokes = typed("99999");
        strokes.push(key(KeyCode::Enter));
        e.input(&strokes, false);
        assert!(e.edit.is_some(), "a bad value stays in the edit box");
        assert!(
            e.status
                .as_ref()
                .is_some_and(|(m, _)| m.contains("-32768 to 32767"))
        );
        e.input(&[key(KeyCode::Escape)], false);
        let mut strokes = typed("3000");
        strokes.push(key(KeyCode::Enter));
        e.input(&strokes, false);
        assert_eq!(
            e.input(&[key(KeyCode::KeyS)], true),
            Outcome::Save(vec![(
                "Warbird".into(),
                "BulletSpeed".into(),
                "3000".into()
            )])
        );
    }

    #[test]
    fn esc_warns_once_about_unsent_changes() {
        let mut e = Editor::new(rows());
        e.input(&[key(KeyCode::Enter)], false);
        assert_eq!(e.input(&[key(KeyCode::Escape)], false), Outcome::None);
        assert_eq!(e.input(&[key(KeyCode::Escape)], false), Outcome::Close);
        let mut clean = Editor::new(rows());
        assert_eq!(clean.input(&[key(KeyCode::Escape)], false), Outcome::Close);
    }

    #[test]
    fn ships_after_the_first_get_its_help_back() {
        let rows = parse(
            b"Warbird:BulletSpeed:2000:-32768:32767:How fast bullets travel\r\n\
              Javelin:BulletSpeed:2000:-32768:32767:As for Warbird:BulletSpeed\r\n",
        );
        let e = Editor::new(rows);
        assert_eq!(e.rows[1].help, "How fast bullets travel");
    }

    #[test]
    fn a_fresh_list_keeps_unsent_edits_and_the_place() {
        let mut e = Editor::new(rows());
        e.input(&[key(KeyCode::ArrowRight)], false); // Warbird
        let mut strokes = typed("2500");
        strokes.push(key(KeyCode::Enter));
        e.input(&strokes, false);
        e.load(rows());
        assert_eq!(e.sections[e.section], "Warbird");
        assert_eq!(
            e.pending(),
            vec![("Warbird".into(), "BulletSpeed".into(), "2500".into())]
        );
        assert!(!e.is_loading());
        assert!(Editor::loading().is_loading());
    }

    #[test]
    fn setting_a_value_back_is_no_change() {
        let mut e = Editor::new(rows());
        e.input(&[key(KeyCode::Enter), key(KeyCode::Enter)], false);
        assert!(e.pending().is_empty());
    }
}
