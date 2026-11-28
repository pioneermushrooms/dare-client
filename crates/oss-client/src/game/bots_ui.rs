//! The bots window: Esc then B.
//!
//! Drawn in the game's menu box like the Esc menu and the Options window: a green title, white
//! rows, a highlighted selection, a yellow line of keys at the foot. Every row and foot key is
//! clickable, and a click is the same keystroke as typing it. The window never touches the zone
//! itself: keys turn into the `?bots …` commands it hands back ([`BotsAction::Send`]), and the
//! zone's answers come in as a fresh list (`?bots data`, see `bots_view`) and as `Bots: …`
//! replies, shown on the window's status line instead of in chat.
//!
//! Anyone sees the list. Only the arena's owner gets Create, editing and the action keys.

use winit::keyboard::KeyCode;

use crate::game::bots_view::BotsView;
use crate::game::input::Stroke;
use crate::game::start_spots::Rect;
use crate::game::view::{Canvas, color};
use crate::gfx::Renderer;

/// What the window asks the game to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BotsAction {
    None,
    /// Send this `?bots …` command to the zone.
    Send(String),
    /// Send these, in order.
    SendMany(Vec<String>),
    /// Close the window.
    Close,
}

/// Characters across the box (the Esc menu's width), and the room inside it.
pub const COLUMNS: usize = 40;
const INNER: usize = COLUMNS - 1;
/// Seconds between `?bots data` refreshes while the window is open.
pub const REFRESH_SECS: f32 = 2.0;

/// One drawn line: its text, colour, whether it is the selection, and the keystroke a click on it
/// means.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub text: String,
    pub color: u32,
    pub selected: bool,
    pub stroke: Option<Stroke>,
}

/// The bots window's state.
#[derive(Debug, Default)]
pub struct BotsUi {
    view: BotsView,
    /// The selected list row (owners have "Create a bot" as row 0).
    sel: usize,
    mode: Mode,
    /// The zone's last `Bots: …` answer, shown above the keys.
    status: Option<String>,
    /// Digits typed for the team or priority line, sent on Enter.
    typed: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
enum Mode {
    #[default]
    List,
    /// One bot's settings; `field` indexes [`FIELDS`].
    Bot {
        field: usize,
    },
    Confirm(Ask),
    /// Team sizes; `row` indexes [`BotsUi::teams`].
    Matches {
        row: usize,
    },
    /// Making a bot, one step at a time.
    Create(Draft),
}

/// A bot being made: [`STEPS`] in order, then a summary (`step == STEPS.len()`).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Draft {
    step: usize,
    name: String,
    /// Index into [`BEHAVIOURS`].
    behaviour: usize,
    ship: u8,
    /// Typed digits; empty means 0.
    team: String,
    chat: bool,
}

impl Default for Draft {
    fn default() -> Self {
        Draft {
            step: 0,
            name: String::new(),
            behaviour: 3, // fighter
            ship: 1,
            team: "0".into(),
            chat: false,
        }
    }
}

impl Draft {
    fn team(&self) -> i32 {
        self.team.parse().unwrap_or(0)
    }

    fn command(&self) -> String {
        format!(
            "?bots create {} {} ship {} team {} chat {}",
            self.name.trim(),
            BEHAVIOURS[self.behaviour],
            self.ship,
            self.team(),
            if self.chat { "on" } else { "off" }
        )
    }
}

/// The steps of Create, in order.
const STEPS: [&str; 5] = ["Name", "Behaviour", "Ship", "Team", "Chat"];
/// Longest bot name (the zone's rule).
const NAME_MAX: usize = 20;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Ask {
    Remove(String),
    StopAll,
}

/// The behaviours, in the order Left/Right steps through them.
const BEHAVIOURS: [&str; 4] = ["idle", "patrol", "sentry", "fighter"];
const SHIPS: [&str; 8] = [
    "Warbird",
    "Javelin",
    "Spider",
    "Leviathan",
    "Terrier",
    "Weasel",
    "Lancaster",
    "Shark",
];
/// The lines of a bot's settings, in order.
const FIELDS: [&str; 5] = ["Behaviour", "Ship", "Team", "Priority", "Chat"];

fn letter(code: KeyCode, ch: char) -> Stroke {
    Stroke::press(code).typing(ch)
}

/// `text` cut to fit the box.
fn fit(text: String) -> String {
    text.chars().take(INNER).collect()
}

impl BotsUi {
    /// A fresh list from the zone.
    pub fn set_view(&mut self, view: BotsView) {
        self.view = view;
        let rows = self.rows();
        if self.sel >= rows {
            self.sel = rows.saturating_sub(1);
        }
    }

    fn owner(&self) -> bool {
        self.view.owner
    }

    /// Rows in the list: the bots, plus "Create a bot" for owners.
    fn rows(&self) -> usize {
        self.view.bots.len() + usize::from(self.owner())
    }

    /// The index into the bots of the selected row, when it is a bot.
    fn selected(&self) -> Option<usize> {
        let i = if self.owner() {
            self.sel.checked_sub(1)?
        } else {
            self.sel
        };
        (i < self.view.bots.len()).then_some(i)
    }

    /// Teams with a size or a bot, in order, and each one's size.
    fn teams(&self) -> Vec<(i32, Option<i32>)> {
        let mut teams: Vec<i32> = self
            .view
            .bots
            .iter()
            .map(|b| b.team)
            .chain(self.view.matches.iter().map(|m| m.0))
            .collect();
        teams.sort_unstable();
        teams.dedup();
        teams
            .into_iter()
            .map(|t| {
                let size = self.view.matches.iter().find(|m| m.0 == t).map(|m| m.1);
                (t, size)
            })
            .collect()
    }

    /// Keys while the window is up. Stops at the first key that asks for something.
    pub fn input(&mut self, strokes: &[Stroke]) -> BotsAction {
        for stroke in strokes {
            let action = self.key(*stroke);
            if action != BotsAction::None {
                return action;
            }
        }
        BotsAction::None
    }

    fn key(&mut self, stroke: Stroke) -> BotsAction {
        let code = stroke.first_press();
        let ch = stroke.text.map(|c| c.to_ascii_lowercase());
        if code == Some(KeyCode::Escape) {
            return if self.back() {
                BotsAction::None
            } else {
                BotsAction::Close
            };
        }
        let up = code == Some(KeyCode::ArrowUp);
        let down = code == Some(KeyCode::ArrowDown);
        match self.mode.clone() {
            Mode::List => {
                if up || down {
                    let rows = self.rows();
                    if rows > 0 {
                        self.sel = if up {
                            self.sel.saturating_sub(1)
                        } else {
                            (self.sel + 1).min(rows - 1)
                        };
                    }
                    return BotsAction::None;
                }
                if !self.owner() {
                    return BotsAction::None;
                }
                if ch == Some('c') || (code == Some(KeyCode::Enter) && self.sel == 0) {
                    self.mode = Mode::Create(Draft::default());
                } else if code == Some(KeyCode::Enter) {
                    if self.selected().is_some() {
                        self.mode = Mode::Bot { field: 0 };
                        self.typed.clear();
                    }
                } else if ch == Some('d') {
                    return self.deploy_toggle();
                } else if matches!(code, Some(KeyCode::Delete | KeyCode::Backspace)) {
                    if let Some(i) = self.selected() {
                        self.mode = Mode::Confirm(Ask::Remove(self.view.bots[i].name.clone()));
                    }
                } else if ch == Some('s') {
                    self.mode = Mode::Confirm(Ask::StopAll);
                } else if ch == Some('m') {
                    self.mode = Mode::Matches { row: 0 };
                }
                BotsAction::None
            }
            Mode::Bot { field } => {
                if up || down {
                    self.typed.clear();
                    let field = if up {
                        field.saturating_sub(1)
                    } else {
                        (field + 1).min(FIELDS.len() - 1)
                    };
                    self.mode = Mode::Bot { field };
                    return BotsAction::None;
                }
                if ch == Some('d') {
                    return self.deploy_toggle();
                }
                if let Some(digit) = stroke.text.filter(|c| c.is_ascii_digit())
                    && (field == 2 || field == 3)
                {
                    if self.typed.len() < 4 {
                        self.typed.push(digit);
                    }
                    return BotsAction::None;
                }
                let step = match code {
                    Some(KeyCode::ArrowRight) => 1,
                    Some(KeyCode::ArrowLeft) => -1,
                    Some(KeyCode::Enter) => 0,
                    _ => return BotsAction::None,
                };
                self.change(field, step)
            }
            Mode::Confirm(ask) => {
                match ch {
                    Some('y') => {
                        self.mode = Mode::List;
                        return BotsAction::Send(match ask {
                            Ask::Remove(name) => format!("?bots remove {name}"),
                            Ask::StopAll => "?bots stopall".into(),
                        });
                    }
                    Some('n') => self.mode = Mode::List,
                    _ => {}
                }
                BotsAction::None
            }
            Mode::Create(mut draft) => {
                let action = self.create_key(&mut draft, stroke, code, ch);
                if let Mode::Create(_) = self.mode {
                    self.mode = Mode::Create(draft);
                }
                action
            }
            Mode::Matches { row } => {
                let teams = self.teams();
                if up || down {
                    let row = if up {
                        row.saturating_sub(1)
                    } else {
                        (row + 1).min(teams.len().saturating_sub(1))
                    };
                    self.mode = Mode::Matches { row };
                    return BotsAction::None;
                }
                let Some(&(team, size)) = teams.get(row) else {
                    return BotsAction::None;
                };
                let next = match (code, size) {
                    (Some(KeyCode::ArrowRight), None) => Some(1),
                    (Some(KeyCode::ArrowRight), Some(n)) => Some((n + 1).min(16)),
                    (Some(KeyCode::ArrowLeft), Some(0)) => None,
                    (Some(KeyCode::ArrowLeft), Some(n)) => Some(n - 1),
                    _ => return BotsAction::None,
                };
                self.view.matches.retain(|m| m.0 != team);
                match next {
                    Some(n) => {
                        self.view.matches.push((team, n));
                        self.view.matches.sort_unstable();
                        BotsAction::Send(format!("?bots match {team} {n}"))
                    }
                    None => BotsAction::Send(format!("?bots match {team} off")),
                }
            }
        }
    }

    /// A key while making a bot.
    fn create_key(
        &mut self,
        draft: &mut Draft,
        stroke: Stroke,
        code: Option<KeyCode>,
        ch: Option<char>,
    ) -> BotsAction {
        let enter = code == Some(KeyCode::Enter);
        let step = match code {
            Some(KeyCode::ArrowRight) => 1,
            Some(KeyCode::ArrowLeft) => -1,
            _ => 0,
        };
        match draft.step {
            0 => {
                if enter {
                    if !draft.name.trim().is_empty() {
                        draft.step = 1;
                    }
                    return BotsAction::None;
                } else if code == Some(KeyCode::Backspace) {
                    draft.name.pop();
                } else if let Some(c) = stroke.text.filter(|c| c.is_ascii_graphic() || *c == ' ')
                    && draft.name.chars().count() < NAME_MAX
                {
                    draft.name.push(c);
                }
            }
            1 => draft.behaviour = (draft.behaviour as i32 + step).rem_euclid(4) as usize,
            2 => draft.ship = ((draft.ship as i32 - 1 + step).rem_euclid(8) + 1) as u8,
            3 => {
                if let Some(d) = stroke.text.filter(|c| c.is_ascii_digit()) {
                    if draft.team == "0" {
                        draft.team.clear();
                    }
                    if draft.team.len() < 4 {
                        draft.team.push(d);
                    }
                } else if code == Some(KeyCode::Backspace) {
                    draft.team.pop();
                } else if step != 0 {
                    draft.team = (draft.team() + step).clamp(0, 9999).to_string();
                }
            }
            4 => {
                if step != 0 || ch == Some(' ') {
                    draft.chat = !draft.chat;
                }
            }
            _ => {
                let name = draft.name.trim().to_string();
                let create = draft.command();
                let action = match ch {
                    Some('d') => BotsAction::Send(create),
                    Some('s') => BotsAction::SendMany(vec![create, format!("?bots stop {name}")]),
                    _ => return BotsAction::None,
                };
                self.mode = Mode::List;
                self.status = Some(format!("Making {name}..."));
                return action;
            }
        }
        if enter && draft.step > 0 && draft.step < STEPS.len() {
            draft.step += 1;
        }
        BotsAction::None
    }

    /// D: deploy a stopped bot, stop a deployed one.
    fn deploy_toggle(&mut self) -> BotsAction {
        let Some(i) = self.selected() else {
            return BotsAction::None;
        };
        let bot = &mut self.view.bots[i];
        bot.deployed = !bot.deployed;
        let verb = if bot.deployed { "deploy" } else { "stop" };
        BotsAction::Send(format!("?bots {verb} {}", bot.name))
    }

    /// Left/Right (step -1/+1) or Enter (step 0) on a bot's setting: the new value, sent at once
    /// and shown at once.
    fn change(&mut self, field: usize, step: i32) -> BotsAction {
        let Some(i) = self.selected() else {
            return BotsAction::None;
        };
        let typed = std::mem::take(&mut self.typed).parse::<i32>().ok();
        let bot = &mut self.view.bots[i];
        let setting = match field {
            0 => {
                let at = BEHAVIOURS
                    .iter()
                    .position(|b| *b == bot.behaviour)
                    .unwrap_or(0) as i32;
                let next =
                    BEHAVIOURS[(at + if step == 0 { 1 } else { step }).rem_euclid(4) as usize];
                bot.behaviour = next.into();
                next.to_string()
            }
            1 => {
                let next =
                    (bot.ship as i32 - 1 + if step == 0 { 1 } else { step }).rem_euclid(8) + 1;
                bot.ship = next as u8;
                format!("ship {next}")
            }
            2 => {
                bot.team = typed.unwrap_or(bot.team + step).clamp(0, 9999);
                format!("team {}", bot.team)
            }
            3 => {
                bot.priority = typed.unwrap_or(bot.priority as i32 + step).clamp(1, 9) as u8;
                format!("priority {}", bot.priority)
            }
            _ => {
                bot.chat = !bot.chat;
                format!("chat {}", if bot.chat { "on" } else { "off" })
            }
        };
        BotsAction::Send(format!("?bots set {} {setting}", bot.name))
    }

    /// Esc: back one level. False when already at the list (the caller closes the window).
    pub fn back(&mut self) -> bool {
        self.typed.clear();
        match &mut self.mode {
            Mode::List => return false,
            Mode::Create(draft) if draft.step > 0 => draft.step -= 1,
            _ => self.mode = Mode::List,
        }
        true
    }

    /// A `Bots: …` line from the zone while the window is open: shown on the status line.
    /// True when it was one (keep it out of chat).
    pub fn reply(&mut self, text: &str) -> bool {
        match text.strip_prefix("Bots:") {
            Some(rest) => {
                self.status = Some(rest.trim().to_string());
                true
            }
            None => false,
        }
    }

    /// The lines the window shows now, top to bottom (title, rows, notes, status, foot keys).
    pub fn lines(&self) -> Vec<Line> {
        let mut out = Vec::new();
        let mut push = |text: String, color: u32, selected: bool, stroke: Option<Stroke>| {
            out.push(Line {
                text: fit(text),
                color,
                selected,
                stroke,
            })
        };
        let arena = if self.view.arena.is_empty() {
            "this arena".to_string()
        } else {
            self.view.arena.clone()
        };
        match &self.mode {
            Mode::List => {
                push(format!("-= Bots in {arena} =-"), color::GREEN, false, None);
                if self.owner() {
                    push("   Create a bot".into(), color::WHITE, self.sel == 0, None);
                }
                if self.view.bots.is_empty() {
                    push("   No bots yet.".into(), color::YELLOW, false, None);
                }
                for (i, b) in self.view.bots.iter().enumerate() {
                    let row = i + usize::from(self.owner());
                    let ship = &SHIPS[(b.ship.clamp(1, 8) - 1) as usize][..3];
                    push(
                        format!(
                            "{} {:<13.13} {:<7.7} {ship} t{:<4} {}",
                            if b.deployed { "+" } else { " " },
                            b.name,
                            b.behaviour,
                            b.team,
                            b.status
                        ),
                        if b.problem.is_some() {
                            color::RED
                        } else {
                            color::WHITE
                        },
                        self.sel == row,
                        None,
                    );
                }
                if !self.view.matches.is_empty() {
                    let sizes: Vec<String> = self
                        .view
                        .matches
                        .iter()
                        .map(|(t, n)| format!("t{t}={n}"))
                        .collect();
                    push(
                        format!("Team sizes: {}", sizes.join(" ")),
                        color::GREEN,
                        false,
                        None,
                    );
                }
                if let Some(problem) = self
                    .selected()
                    .and_then(|i| self.view.bots[i].problem.clone())
                {
                    push(problem, color::RED, false, None);
                }
            }
            Mode::Bot { field } => {
                let b = self.selected().map(|i| &self.view.bots[i]);
                let name = b.map_or("", |b| b.name.as_str());
                push(format!("-= Bot: {name} =-"), color::GREEN, false, None);
                if let Some(b) = b {
                    let values = [
                        b.behaviour.clone(),
                        format!("{} ({})", b.ship, SHIPS[(b.ship.clamp(1, 8) - 1) as usize]),
                        if *field == 2 && !self.typed.is_empty() {
                            format!("{}_", self.typed)
                        } else {
                            b.team.to_string()
                        },
                        if *field == 3 && !self.typed.is_empty() {
                            format!("{}_", self.typed)
                        } else {
                            b.priority.to_string()
                        },
                        if b.chat { "on" } else { "off" }.into(),
                    ];
                    for (i, (label, value)) in FIELDS.iter().zip(values).enumerate() {
                        push(
                            format!("   {label:<10} < {value} >"),
                            color::WHITE,
                            i == *field,
                            None,
                        );
                    }
                    push(
                        format!(
                            "   Status     {}",
                            if b.deployed {
                                b.status.as_str()
                            } else {
                                "stopped"
                            }
                        ),
                        color::GREEN,
                        false,
                        None,
                    );
                }
            }
            Mode::Confirm(ask) => {
                let question = match ask {
                    Ask::Remove(name) => format!("Remove {name}? It is deleted for good."),
                    Ask::StopAll => "Stop all bots? Settings are kept.".into(),
                };
                push("-= Are you sure? =-".into(), color::GREEN, false, None);
                push(question, color::YELLOW, false, None);
            }
            Mode::Create(draft) => {
                let done = draft.step >= STEPS.len();
                push(
                    if done {
                        "-= New bot: ready =-".to_string()
                    } else {
                        format!("-= New bot: step {} of {} =-", draft.step + 1, STEPS.len())
                    },
                    color::GREEN,
                    false,
                    None,
                );
                let values = [
                    format!("{}{}", draft.name, if draft.step == 0 { "_" } else { "" }),
                    format!("< {} >", BEHAVIOURS[draft.behaviour]),
                    format!("< {} ({}) >", draft.ship, SHIPS[(draft.ship - 1) as usize]),
                    format!("{}{}", draft.team, if draft.step == 3 { "_" } else { "" }),
                    format!("< {} >", if draft.chat { "on" } else { "off" }),
                ];
                for (i, (label, value)) in STEPS.iter().zip(values).enumerate().take(draft.step + 1)
                {
                    push(
                        format!("   {label:<10} {value}"),
                        color::WHITE,
                        i == draft.step,
                        None,
                    );
                }
                let hint = match draft.step {
                    0 => "Type its name (up to 20 characters).",
                    1 => "fighter chases pilots, patrol flies a route.",
                    2 => "The ship it flies.",
                    3 => "Its team (frequency), 0-9999.",
                    4 => "Chat on: it answers when named.",
                    _ => "Deploy it now, or save it stopped.",
                };
                push(hint.into(), color::GREEN, false, None);
            }
            Mode::Matches { row } => {
                push("-= Team sizes =-".into(), color::GREEN, false, None);
                push(
                    "Bots fill a team up to its size.".into(),
                    color::GREEN,
                    false,
                    None,
                );
                for (i, (team, size)) in self.teams().into_iter().enumerate() {
                    let size = size.map_or("off".to_string(), |n| n.to_string());
                    push(
                        format!("   Team {team:<5} < {size} >"),
                        color::WHITE,
                        i == *row,
                        None,
                    );
                }
            }
        }
        if let Some(status) = &self.status {
            push(status.clone(), color::YELLOW, false, None);
        }
        for (text, stroke) in self.foot() {
            push(text.into(), color::YELLOW, false, Some(stroke));
        }
        out
    }

    /// The key lines at the foot, each clickable.
    fn foot(&self) -> Vec<(&'static str, Stroke)> {
        let esc = Stroke::press(KeyCode::Escape);
        if !self.owner() {
            return vec![("Esc = Close", esc)];
        }
        match self.mode {
            Mode::List => vec![
                (
                    "Enter = Open   Up/Down = Pick",
                    Stroke::press(KeyCode::Enter),
                ),
                ("C = Create a bot", letter(KeyCode::KeyC, 'c')),
                ("D = Deploy / stop", letter(KeyCode::KeyD, 'd')),
                ("M = Team sizes", letter(KeyCode::KeyM, 'm')),
                ("Del = Remove", Stroke::press(KeyCode::Delete)),
                ("S = Stop all", letter(KeyCode::KeyS, 's')),
                ("Esc = Close", esc),
            ],
            Mode::Bot { .. } => vec![
                (
                    "Left/Right = Change   0-9 = Type",
                    Stroke::press(KeyCode::ArrowRight),
                ),
                ("D = Deploy / stop", letter(KeyCode::KeyD, 'd')),
                ("Esc = Back", esc),
            ],
            Mode::Confirm(_) => vec![
                ("Y = Yes", letter(KeyCode::KeyY, 'y')),
                ("N = No", letter(KeyCode::KeyN, 'n')),
            ],
            Mode::Matches { .. } => vec![
                (
                    "Left/Right = Change   Up/Down = Pick",
                    Stroke::press(KeyCode::ArrowRight),
                ),
                ("Esc = Back", esc),
            ],
            Mode::Create(ref d) if d.step >= STEPS.len() => vec![
                ("D = Deploy now", letter(KeyCode::KeyD, 'd')),
                ("S = Save stopped", letter(KeyCode::KeyS, 's')),
                ("Esc = Back", esc),
            ],
            Mode::Create(ref d) if d.step == 0 => vec![
                ("Enter = Next", Stroke::press(KeyCode::Enter)),
                ("Esc = Back", esc),
            ],
            Mode::Create(_) => vec![
                ("Left/Right = Change", Stroke::press(KeyCode::ArrowRight)),
                ("Enter = Next", Stroke::press(KeyCode::Enter)),
                ("Esc = Back", esc),
            ],
        }
    }

    /// Draws the window and returns where each clickable line is and what a click means.
    pub fn draw(&self, c: &Canvas, r: &mut Renderer) -> Vec<(Rect, Stroke)> {
        let lines = self.lines();
        let w = COLUMNS as f32 * 8.0;
        let x = ((c.view_w - w) / 2.0).floor();
        let y = 4.0;
        let h = 8.0 + lines.len() as f32 * 12.0;
        c.panel(r, x, y, w, h);
        let mut spots = Vec::new();
        for (i, line) in lines.iter().enumerate() {
            let row = y + 4.0 + i as f32 * 12.0;
            if line.selected {
                c.rect(r, x + 1.0, row - 1.0, w - 2.0, 12.0, [0.2, 0.3, 0.5, 0.9]);
            }
            c.text(r, x + 4.0, row, line.color, &line.text);
            if let Some(stroke) = line.stroke {
                spots.push((Rect::new(x, row - 1.0, w, 12.0), stroke));
            }
        }
        spots
    }
}

/// Whether a `?bots data` refresh is due: only while the window is open, at once on opening
/// (`last` is None), then every [`REFRESH_SECS`].
pub fn refresh_due(open: bool, now: f32, last: Option<f32>) -> bool {
    open && last.is_none_or(|last| now - last >= REFRESH_SECS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::bots_view::BotRow;

    fn bot(name: &str, deployed: bool) -> BotRow {
        BotRow {
            name: name.into(),
            behaviour: "fighter".into(),
            ship: 1,
            team: 0,
            priority: 5,
            chat: false,
            deployed,
            status: if deployed { "in" } else { "stopped" }.into(),
            problem: None,
        }
    }

    fn ui(owner: bool, bots: Vec<BotRow>) -> BotsUi {
        let mut ui = BotsUi::default();
        ui.set_view(BotsView {
            arena: "botlab".into(),
            owner,
            bots,
            matches: vec![(0, 2)],
        });
        ui
    }

    fn key(k: KeyCode) -> Stroke {
        Stroke::press(k)
    }

    fn ch(c: char) -> Stroke {
        Stroke::text(c)
    }

    fn sends(ui: &mut BotsUi, strokes: &[Stroke]) -> Vec<String> {
        let mut out = Vec::new();
        for s in strokes {
            if let BotsAction::Send(cmd) = ui.input(std::slice::from_ref(s)) {
                out.push(cmd);
            }
        }
        out
    }

    #[test]
    fn d_deploys_a_stopped_bot_and_stops_a_deployed_one() {
        // owner list: row 0 is "Create a bot", then the bots
        let mut u = ui(true, vec![bot("Alpha", true), bot("Bravo", false)]);
        assert_eq!(
            sends(&mut u, &[key(KeyCode::ArrowDown), ch('d')]),
            vec!["?bots stop Alpha"]
        );
        assert_eq!(
            sends(&mut u, &[key(KeyCode::ArrowDown), ch('D')]),
            vec!["?bots deploy Bravo"]
        );
    }

    #[test]
    fn editing_ship_sends_set_ship() {
        let mut u = ui(true, vec![bot("Big Bob", true)]);
        // to the bot, open it, down to Ship (fields: behaviour, ship, team, priority, chat)
        let sent = sends(
            &mut u,
            &[
                key(KeyCode::ArrowDown),
                key(KeyCode::Enter),
                key(KeyCode::ArrowDown),
                key(KeyCode::ArrowRight),
            ],
        );
        assert_eq!(sent, vec!["?bots set Big Bob ship 2"]);
        // behaviour cycles too
        assert_eq!(
            sends(&mut u, &[key(KeyCode::ArrowUp), key(KeyCode::ArrowRight)]),
            vec!["?bots set Big Bob idle"]
        );
        // Esc goes back to the list, then the caller closes
        assert!(u.back());
        assert!(!u.back());
    }

    #[test]
    fn del_and_stop_all_ask_first() {
        let mut u = ui(true, vec![bot("Alpha", true)]);
        assert!(sends(&mut u, &[key(KeyCode::ArrowDown), key(KeyCode::Delete)]).is_empty());
        assert!(u.lines().iter().any(|l| l.text.contains("Remove Alpha?")));
        assert_eq!(sends(&mut u, &[ch('y')]), vec!["?bots remove Alpha"]);
        assert!(sends(&mut u, &[ch('s')]).is_empty());
        assert!(u.lines().iter().any(|l| l.text.contains("Stop all bots?")));
        assert!(sends(&mut u, &[ch('n')]).is_empty());
        assert!(sends(&mut u, &[ch('s')]).is_empty());
        assert_eq!(sends(&mut u, &[ch('y')]), vec!["?bots stopall"]);
    }

    #[test]
    fn match_sizes_change_with_left_and_right() {
        let mut u = ui(true, vec![bot("Alpha", true)]);
        assert!(sends(&mut u, &[ch('m')]).is_empty());
        assert_eq!(
            sends(&mut u, &[key(KeyCode::ArrowRight)]),
            vec!["?bots match 0 3"]
        );
        assert_eq!(
            sends(&mut u, &[key(KeyCode::ArrowLeft), key(KeyCode::ArrowLeft)]),
            vec!["?bots match 0 2", "?bots match 0 1"]
        );
        assert_eq!(
            sends(&mut u, &[key(KeyCode::ArrowLeft), key(KeyCode::ArrowLeft)]),
            vec!["?bots match 0 0", "?bots match 0 off"]
        );
    }

    #[test]
    fn a_non_owner_window_sends_nothing() {
        let mut u = ui(false, vec![bot("Alpha", true), bot("Bravo", false)]);
        let every = [
            key(KeyCode::ArrowDown),
            key(KeyCode::Enter),
            key(KeyCode::ArrowRight),
            ch('d'),
            ch('c'),
            ch('m'),
            ch('s'),
            ch('y'),
            key(KeyCode::Delete),
            ch('y'),
        ];
        assert!(sends(&mut u, &every).is_empty());
        assert!(!u.lines().iter().any(|l| l.text.contains("Create")));
        // every clickable line of a non-owner's window is harmless too
        for line in u.lines() {
            if let Some(s) = line.stroke {
                assert!(sends(&mut u, &[s]).is_empty(), "{:?}", line.text);
            }
        }
    }

    #[test]
    fn every_line_fits_the_box() {
        let long = BotRow {
            name: "Twenty Characters Xy".into(),
            behaviour: "fighter".into(),
            ship: 8,
            team: 9999,
            priority: 9,
            chat: true,
            deployed: true,
            status: "not launched".into(),
            problem: Some("Not launched: a pilot uses this name and more text".into()),
        };
        let mut u = ui(true, vec![long.clone(), bot("B", false)]);
        let check = |u: &BotsUi| {
            for l in u.lines() {
                assert!(
                    l.text.chars().count() <= INNER,
                    "{:?} is {} wide",
                    l.text,
                    l.text.chars().count()
                );
            }
        };
        check(&u);
        sends(&mut u, &[key(KeyCode::ArrowDown), key(KeyCode::Enter)]);
        check(&u);
        u.back();
        sends(&mut u, &[ch('m')]);
        check(&u);
        let mut v = ui(false, vec![long]);
        check(&v);
        v.reply("Bots: a very long reply from the zone that goes on and on past the edge");
        check(&v);
    }

    #[test]
    fn bots_replies_show_on_the_status_line() {
        let mut u = ui(true, vec![bot("Alpha", true)]);
        assert!(u.reply("Bots: Alpha stopped."));
        assert!(u.lines().iter().any(|l| l.text.contains("Alpha stopped.")));
        assert!(!u.reply("Welcome to the arena"));
    }

    #[test]
    fn refresh_runs_only_while_open() {
        assert!(!refresh_due(false, 10.0, None));
        assert!(!refresh_due(false, 10.0, Some(1.0)));
        assert!(refresh_due(true, 10.0, None));
        assert!(!refresh_due(true, 10.0, Some(9.0)));
        assert!(refresh_due(true, 10.0, Some(8.0)));
    }

    fn typed(text: &str) -> Vec<Stroke> {
        text.chars().map(Stroke::text).collect()
    }

    fn actions(ui: &mut BotsUi, strokes: &[Stroke]) -> Vec<BotsAction> {
        strokes
            .iter()
            .map(|s| ui.input(std::slice::from_ref(s)))
            .filter(|a| *a != BotsAction::None)
            .collect()
    }

    #[test]
    fn create_walks_the_steps_and_sends_one_create() {
        let mut u = ui(true, vec![bot("Alpha", true)]);
        let mut keys = vec![key(KeyCode::Enter)]; // "Create a bot" is row 0
        keys.extend(typed("Big Bob")); // d and s type, they don't deploy or stop all
        keys.push(key(KeyCode::Enter)); // name
        keys.push(key(KeyCode::Enter)); // behaviour: fighter
        keys.push(key(KeyCode::ArrowRight)); // ship 2
        keys.push(key(KeyCode::Enter));
        keys.extend(typed("3")); // team 3
        keys.push(key(KeyCode::Enter));
        keys.push(key(KeyCode::ArrowRight)); // chat on
        keys.push(key(KeyCode::Enter));
        assert!(actions(&mut u, &keys).is_empty());
        assert!(u.lines().iter().any(|l| l.text.contains("Big Bob")));
        assert_eq!(
            actions(&mut u, &[ch('d')]),
            vec![BotsAction::Send(
                "?bots create Big Bob fighter ship 2 team 3 chat on".into()
            )]
        );
        // back at the list
        assert!(!u.back());
    }

    #[test]
    fn c_starts_create_too_and_esc_goes_back_one_step() {
        let mut u = ui(true, vec![]);
        assert!(actions(&mut u, &[ch('c')]).is_empty());
        let mut keys = typed("Zed");
        keys.push(key(KeyCode::Enter));
        keys.push(key(KeyCode::ArrowLeft)); // behaviour: sentry
        keys.push(key(KeyCode::Enter));
        assert!(actions(&mut u, &keys).is_empty());
        assert!(u.lines().iter().any(|l| l.text.contains("Ship")));
        assert!(u.back()); // back to behaviour
        assert!(u.lines().iter().any(|l| l.text.contains("sentry")));
        assert!(u.back()); // back to name, still typed
        assert!(u.lines().iter().any(|l| l.text.contains("Zed")));
        assert!(u.back()); // back to the list
        assert!(!u.back());
    }

    #[test]
    fn save_stopped_creates_then_stops() {
        let mut u = ui(true, vec![]);
        let mut keys = vec![ch('c')];
        keys.extend(typed("Quiet"));
        keys.extend([key(KeyCode::Enter); 5]); // name, behaviour, ship, team, chat
        assert!(actions(&mut u, &keys).is_empty());
        assert_eq!(
            actions(&mut u, &[ch('s')]),
            vec![BotsAction::SendMany(vec![
                "?bots create Quiet fighter ship 1 team 0 chat off".into(),
                "?bots stop Quiet".into(),
            ])]
        );
    }

    #[test]
    fn a_name_over_20_characters_is_cut() {
        let mut u = ui(true, vec![]);
        let mut keys = vec![ch('c')];
        keys.extend(typed("Abcdefghijklmnopqrstuvwxyz"));
        keys.push(key(KeyCode::Backspace));
        keys.extend([key(KeyCode::Enter); 5]);
        assert!(actions(&mut u, &keys).is_empty());
        assert_eq!(
            actions(&mut u, &[ch('d')]),
            vec![BotsAction::Send(
                "?bots create Abcdefghijklmnopqrs fighter ship 1 team 0 chat off".into()
            )]
        );
    }

    #[test]
    fn an_empty_name_does_not_go_on() {
        let mut u = ui(true, vec![]);
        assert!(actions(&mut u, &[ch('c'), key(KeyCode::Enter), key(KeyCode::Enter)]).is_empty());
        assert!(u.lines().iter().any(|l| l.text.contains("Name")));
    }
}
