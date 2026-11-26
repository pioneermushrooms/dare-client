//! The voice menu: Tribes-style quick voice lines on a key sequence.
//!
//! `V` opens it; letters walk down the menus in [`MENUS`]; the last letter of a phrase in
//! [`PHRASES`] sends it and closes the menu in the same frame. Backspace goes up a level (and
//! closes at the top), Esc closes, and four idle seconds close it. Sending is `?v <code>`; the
//! zone answers with a chat line from the sender carrying the phrase's sound id (128-255), which
//! [`clip_name`] turns into the clip to play.
//!
//! This is **ours**: Continuum has no voice menu. The phrase table is generated into
//! `voice_table.rs`.

pub use super::voice_table::{MENUS, PHRASES};

/// Seconds without a key before an open menu closes itself.
pub const IDLE_CLOSE: f32 = 4.0;

/// Seconds between "Enter to chat" reminders.
pub const HINT_EVERY: f32 = 20.0;

/// What a key did to the menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceStep {
    /// The menu opened.
    Open,
    /// Moved to another level.
    Moved,
    /// A phrase was chosen (its code); the menu is closed.
    Send(&'static str),
    /// The menu closed without sending.
    Closed,
    /// The key means nothing here.
    Ignored,
}

/// The menu's state: closed, or open at a level (`path`, the letters typed after V).
#[derive(Debug, Default)]
pub struct VoiceMenu {
    open: bool,
    path: String,
    last_key: f32,
    /// When the "Enter to chat" hint may next be shown (`None`: any time).
    next_hint: Option<f32>,
    /// Game clock time until which the zone has silenced our voice lines.
    pub silenced_until: f32,
}

impl VoiceMenu {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// The letters typed after V at the current level.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// A character key. With the menu closed only `v` (either case) does anything.
    pub fn press(&mut self, c: char, now: f32) -> VoiceStep {
        let c = c.to_ascii_lowercase();
        if !self.open {
            if c != 'v' {
                return VoiceStep::Ignored;
            }
            self.open = true;
            self.path.clear();
            self.last_key = now;
            return VoiceStep::Open;
        }
        if !c.is_ascii_lowercase() {
            return VoiceStep::Ignored;
        }
        let code = format!("{}{c}", self.path);
        if let Some(p) = PHRASES.iter().find(|p| p.code == code) {
            self.close();
            return VoiceStep::Send(p.code);
        }
        if MENUS.iter().any(|m| m.code == code) {
            self.path = code;
            self.last_key = now;
            return VoiceStep::Moved;
        }
        VoiceStep::Ignored
    }

    /// Backspace: up one level, or closed from the top.
    pub fn back(&mut self, now: f32) -> VoiceStep {
        if !self.open {
            return VoiceStep::Ignored;
        }
        if self.path.pop().is_none() {
            self.close();
            return VoiceStep::Closed;
        }
        self.last_key = now;
        VoiceStep::Moved
    }

    pub fn close(&mut self) {
        self.open = false;
        self.path.clear();
    }

    /// Closes the menu after [`IDLE_CLOSE`] seconds without a key.
    pub fn tick(&mut self, now: f32) {
        if self.open && now - self.last_key >= IDLE_CLOSE {
            self.close();
        }
    }

    /// Whether to remind the player that Enter opens chat (a letter went nowhere);
    /// at most once every [`HINT_EVERY`] seconds.
    pub fn hint_due(&mut self, now: f32) -> bool {
        if self.next_hint.is_some_and(|t| now < t) {
            return false;
        }
        self.next_hint = Some(now + HINT_EVERY);
        true
    }

    /// The panel's title: `V G C`, plus who hears this level's lines once a menu is
    /// chosen. `None` while closed.
    pub fn header(&self) -> Option<String> {
        if !self.open {
            return None;
        }
        let mut h = String::from("V");
        for c in self.path().chars() {
            h.push(' ');
            h.push(c.to_ascii_uppercase());
        }
        let team = PHRASES
            .iter()
            .find(|p| !self.path.is_empty() && p.code.starts_with(self.path.as_str()))
            .map(|p| p.team);
        match team {
            Some(true) => h += "  (team)",
            Some(false) => h += "  (everyone)",
            None => {}
        }
        Some(h)
    }

    /// The current level: (key, label, is a sub-menu), in table order.
    pub fn entries(&self) -> Vec<(char, &'static str, bool)> {
        if !self.open {
            return Vec::new();
        }
        let child =
            |code: &str| code.len() == self.path.len() + 1 && code.starts_with(self.path.as_str());
        let last = |code: &str| code.chars().last().unwrap_or(' ');
        MENUS
            .iter()
            .filter(|m| child(m.code))
            .map(|m| (last(m.code), m.label, true))
            .chain(
                PHRASES
                    .iter()
                    .filter(|p| child(p.code))
                    .map(|p| (last(p.code), p.label, false)),
            )
            .collect()
    }
}

/// The zone's notice when it silences our voice lines (`Voice silenced for N s - ...`):
/// the seconds, so the menu can count down.
pub fn silence_from_notice(text: &str) -> Option<u32> {
    let rest = text.strip_prefix("Voice silenced for ")?;
    let (n, tail) = rest.split_once(' ')?;
    if !tail.starts_with('s') {
        return None;
    }
    n.parse().ok()
}

/// Where a typed character goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CharRoute {
    Chat,
    Voice,
}

/// With Enter-to-chat on, characters type only while the chat line is open and drive the voice
/// menu otherwise; with it off, every character types, as in Continuum.
pub fn route_char(enter_to_chat: bool, chat_open: bool, _voice_open: bool) -> CharRoute {
    if !enter_to_chat || chat_open {
        CharRoute::Chat
    } else {
        CharRoute::Voice
    }
}

/// Whether keys that type characters belong to the chat line or the voice menu right now,
/// rather than to game bindings and macros. With Enter-to-chat on: while either is open;
/// off: once the chat line has something on it (Continuum).
pub fn keys_type(
    enter_to_chat: bool,
    chat_open: bool,
    chat_has_text: bool,
    voice_open: bool,
) -> bool {
    voice_open
        || if enter_to_chat {
            chat_open
        } else {
            chat_has_text
        }
}

/// The clip for a chat sound id: 128-255 are voice lines (`voice<id>`), 1-26 Continuum's
/// classic chat sounds (`bong<id>`); anything else plays nothing.
pub fn clip_name(sound: u8) -> Option<String> {
    match sound {
        128..=255 => Some(format!("voice{sound}")),
        1..=26 => Some(format!("bong{sound}")),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v_g_w_sends_woohoo_and_closes_at_once() {
        let mut m = VoiceMenu::new();
        assert_eq!(m.press('v', 0.0), VoiceStep::Open);
        assert_eq!(m.press('g', 0.1), VoiceStep::Moved);
        assert_eq!(m.press('w', 0.2), VoiceStep::Send("gw"));
        assert!(!m.is_open());
    }

    #[test]
    fn letters_are_case_insensitive_and_unknown_ones_ignored() {
        let mut m = VoiceMenu::new();
        assert_eq!(m.press('V', 0.0), VoiceStep::Open);
        assert_eq!(m.press('T', 0.0), VoiceStep::Moved);
        assert_eq!(m.press('x', 0.0), VoiceStep::Ignored);
        assert_eq!(m.path(), "t");
        assert_eq!(m.press('R', 0.0), VoiceStep::Send("tr"));
    }

    #[test]
    fn closed_menu_ignores_everything_but_v() {
        let mut m = VoiceMenu::new();
        assert_eq!(m.press('g', 0.0), VoiceStep::Ignored);
        assert!(!m.is_open());
    }

    #[test]
    fn backspace_goes_up_then_closes() {
        let mut m = VoiceMenu::new();
        m.press('v', 0.0);
        m.press('g', 0.0);
        m.press('t', 0.0);
        assert_eq!(m.path(), "gt");
        assert_eq!(m.back(0.0), VoiceStep::Moved);
        assert_eq!(m.path(), "g");
        assert_eq!(m.back(0.0), VoiceStep::Moved);
        assert_eq!(m.path(), "");
        assert!(m.is_open());
        assert_eq!(m.back(0.0), VoiceStep::Closed);
        assert!(!m.is_open());
    }

    #[test]
    fn idle_four_seconds_closes() {
        let mut m = VoiceMenu::new();
        m.press('v', 0.0);
        m.tick(3.9);
        assert!(m.is_open());
        m.press('g', 3.0);
        m.tick(6.9);
        assert!(m.is_open());
        m.tick(7.0);
        assert!(!m.is_open());
    }

    #[test]
    fn entries_list_the_current_level() {
        let mut m = VoiceMenu::new();
        m.press('v', 0.0);
        let top = m.entries();
        assert!(top.contains(&('g', "Global", true)));
        assert!(top.contains(&('t', "Target", true)));
        assert_eq!(top.len(), 13);
        m.press('t', 0.0);
        assert!(m.entries().contains(&('r', "He's red!", false)));
        m.press('r', 0.0);
        assert!(m.entries().is_empty());
    }

    #[test]
    fn every_phrase_is_reachable_and_ids_are_voice_range() {
        for p in PHRASES {
            assert!((128..=255).contains(&p.sound), "{}", p.code);
            let mut m = VoiceMenu::new();
            m.press('v', 0.0);
            let (path, last) = p.code.split_at(p.code.len() - 1);
            for c in path.chars() {
                assert_eq!(m.press(c, 0.0), VoiceStep::Moved, "{}", p.code);
            }
            assert_eq!(
                m.press(last.chars().next().unwrap(), 0.0),
                VoiceStep::Send(p.code)
            );
        }
    }

    #[test]
    fn v_types_when_chat_is_open() {
        assert_eq!(route_char(true, true, false), CharRoute::Chat);
    }

    #[test]
    fn v_opens_voice_menu_when_chat_is_closed() {
        assert_eq!(route_char(true, false, false), CharRoute::Voice);
        assert_eq!(route_char(true, false, true), CharRoute::Voice);
    }

    #[test]
    fn legacy_mode_types_letters_straight_into_chat() {
        assert_eq!(route_char(false, false, false), CharRoute::Chat);
    }

    #[test]
    fn silence_notice_parses() {
        assert_eq!(
            silence_from_notice("Voice silenced for 10 s - too many voice lines."),
            Some(10)
        );
        assert_eq!(
            silence_from_notice("Voice silenced for 0 s - too many voice lines."),
            Some(0)
        );
        assert_eq!(silence_from_notice("Voice is off in this arena."), None);
        assert_eq!(silence_from_notice("Voice silenced for lots s"), None);
    }

    #[test]
    fn unknown_voice_id_names_a_clip_and_never_panics() {
        for id in 0..=255u8 {
            let _ = clip_name(id);
        }
        assert_eq!(clip_name(254).as_deref(), Some("voice254"));
    }

    #[test]
    fn panel_header_says_who_hears_it() {
        let mut m = VoiceMenu::new();
        assert_eq!(m.header(), None);
        m.press('v', 0.0);
        assert_eq!(m.header().as_deref(), Some("V"));
        m.press('g', 0.0);
        assert_eq!(m.header().as_deref(), Some("V G  (everyone)"));
        m.press('c', 0.0);
        assert_eq!(m.header().as_deref(), Some("V G C  (everyone)"));
        m.back(0.0);
        m.back(0.0);
        m.press('t', 0.0);
        assert_eq!(m.header().as_deref(), Some("V T  (team)"));
    }

    #[test]
    fn keys_type_while_the_voice_menu_or_chat_is_open() {
        // (enter_to_chat, chat_open, chat_has_text, voice_open)
        assert!(
            keys_type(true, false, false, true),
            "voice menu open: letters are its"
        );
        assert!(
            keys_type(true, true, false, false),
            "Enter-opened line, still empty"
        );
        assert!(
            !keys_type(true, false, false, false),
            "nothing open: game keys are the game's"
        );
        assert!(
            keys_type(false, false, true, false),
            "Continuum: a line with text types"
        );
        assert!(!keys_type(false, false, false, false));
    }

    #[test]
    fn hint_comes_at_most_every_twenty_seconds() {
        let mut m = VoiceMenu::new();
        assert!(m.hint_due(0.0));
        assert!(!m.hint_due(5.0));
        assert!(!m.hint_due(19.9));
        assert!(m.hint_due(20.0));
    }

    #[test]
    fn clip_names() {
        assert_eq!(clip_name(135).as_deref(), Some("voice135"));
        assert_eq!(clip_name(3).as_deref(), Some("bong3"));
        assert_eq!(clip_name(26).as_deref(), Some("bong26"));
        assert_eq!(clip_name(0), None);
        assert_eq!(clip_name(27), None);
        assert_eq!(clip_name(100), None);
    }
}
