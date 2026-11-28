//! The zone's in-game tutorial, as browser play shows it.
//!
//! A zone that runs a tutorial talks to a client that said it can show one (the
//! `?tutorialclient` command, sent on every arena entry by the browser build) in
//! hint lines: arena messages to that one player that start with [`PREFIX`]. They
//! are the page's, which draws the mentor's box and its buttons, so they never
//! reach the chat log; the only one the game acts on itself is `reveal`, which
//! glows round the part of the HUD that has just been uncovered. Each `say` also
//! arrives as an ordinary arena message, which stays in the chat as the
//! scroll-back copy.
//!
//! The page answers with [`queue_command`] (its Skip, Leave, Try and arena
//! buttons), which the game sends as if typed, and asks [`key_for`] for the keys
//! the player has, to fill the `{action}` placeholders in a hint's text.

use std::collections::VecDeque;
use std::sync::Mutex;

use oss_session::chat_type;

use super::chat::{ChatLog, Message};
use super::input::{Action, Bindings};

/// What every hint line starts with.
pub const PREFIX: &str = "~tut~ ";

/// Tells the zone, on entering an arena, that this client shows hint lines.
pub const CLIENT_COMMAND: &str = "?tutorialclient";

/// A part of the HUD the tutorial uncovers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HudPart {
    Radar,
    Energy,
    Statbox,
    /// The item and weapon icons down both sides.
    Items,
    Chat,
}

impl HudPart {
    fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "radar" => HudPart::Radar,
            "energy" => HudPart::Energy,
            "statbox" => HudPart::Statbox,
            "items" => HudPart::Items,
            "chat" => HudPart::Chat,
            _ => return None,
        })
    }
}

/// A prompt the page shows with a button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Prompt {
    /// A step is taking a while: Skip is pointed out.
    Skip,
    /// "Try the tutorial", for a newcomer it was not started for.
    Try,
}

/// One hint line, read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Hint {
    /// The mentor's line for step `step` of `of`; `text` keeps its `{action}`
    /// placeholders.
    Say {
        step: u32,
        of: u32,
        text: String,
    },
    /// That part of the HUD has just been uncovered.
    Reveal(HudPart),
    Prompt(Prompt),
    /// The tutorial is over and the zone is about to move the player on; with
    /// `list` the page offers its arena chooser.
    End {
        list: bool,
    },
}

/// Reads a hint line (with its [`PREFIX`]). Anything else, or a hint this
/// client doesn't know, is `None`.
pub fn parse_hint(text: &str) -> Option<Hint> {
    let body = text.strip_prefix(PREFIX)?;
    let (word, rest) = body.split_once(' ').unwrap_or((body, ""));
    match word {
        "say" => {
            let (count, text) = rest.split_once(' ').unwrap_or((rest, ""));
            let (step, of) = count.split_once('/')?;
            let (step, of) = (step.parse().ok()?, of.parse().ok()?);
            if step == 0 || step > of {
                return None;
            }
            Some(Hint::Say {
                step,
                of,
                text: text.to_string(),
            })
        }
        "reveal" => HudPart::from_name(rest).map(Hint::Reveal),
        "prompt" => match rest {
            "skip" => Some(Hint::Prompt(Prompt::Skip)),
            "try" => Some(Hint::Prompt(Prompt::Try)),
            _ => None,
        },
        "end" => match rest {
            "list" => Some(Hint::End { list: true }),
            "none" => Some(Hint::End { list: false }),
            _ => None,
        },
        _ => None,
    }
}

/// Whether a line from the zone is a hint line: an arena message with no
/// sender, starting with [`PREFIX`]. A player can't send one.
pub fn is_hint_line(kind: u8, sender: Option<&str>, text: &str) -> bool {
    kind == chat_type::ARENA && sender.is_none() && text.starts_with(PREFIX)
}

/// Puts a line that arrived in the chat log, unless it is a hint line, which is
/// handed back instead for the page.
pub fn log_or_hint(
    log: &mut ChatLog,
    kind: u8,
    sender: Option<String>,
    text: String,
) -> Option<String> {
    if is_hint_line(kind, sender.as_deref(), &text) {
        return Some(text);
    }
    log::debug!("message: {text}");
    log.push(Message::received(kind, sender, text));
    None
}

/// Longest arena name the page may ask to go to (the server's own limit is 15).
const MAX_ARENA: usize = 16;

/// The only lines the page may have the game send: `?skip`, `?leave`,
/// `?tutorial` and `?go <arena>`. Anything else is `None`.
pub fn accept_command(cmd: &str) -> Option<String> {
    let cmd = cmd.trim();
    match cmd {
        "?skip" | "?leave" | "?tutorial" => return Some(cmd.to_string()),
        _ => {}
    }
    let arena = cmd.strip_prefix("?go ")?;
    let ok = !arena.is_empty()
        && arena.chars().count() <= MAX_ARENA
        && arena.chars().all(|c| c.is_ascii_alphanumeric() || c == '#');
    ok.then(|| format!("?go {arena}"))
}

/// Lines the page asked for, oldest first, until the game sends them.
static COMMANDS: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());

/// The key table in use, for [`key_for`]. `None` until a game has started.
static KEYS: Mutex<Option<Bindings>> = Mutex::new(None);

/// Queues one of the page's commands (see [`accept_command`]) for the game to
/// send on its next update.
// Called by the page in browser play.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub fn queue_command(cmd: &str) -> Result<(), String> {
    let line = accept_command(cmd).ok_or_else(|| {
        format!("{cmd:?} is not a tutorial command (?skip, ?leave, ?tutorial or ?go <arena>)")
    })?;
    COMMANDS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push_back(line);
    Ok(())
}

/// Takes every queued command, oldest first.
pub fn take_commands() -> Vec<String> {
    COMMANDS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .drain(..)
        .collect()
}

/// Keeps the key table [`key_for`] reads in step with the game's.
// Only browser play has a page to ask.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub fn note_keys(bindings: &Bindings) {
    let mut keys = KEYS.lock().unwrap_or_else(|e| e.into_inner());
    if keys.as_ref() != Some(bindings) {
        *keys = Some(bindings.clone());
    }
}

/// How the player's keys for `action` (an [`Action::name`], e.g. `gun`) read,
/// the defaults before a game has started; empty for an action there is no
/// such thing as.
// Called by the page in browser play.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub fn key_for(action: &str) -> String {
    let Some(action) = Action::from_name(action) else {
        return String::new();
    };
    let keys = KEYS.lock().unwrap_or_else(|e| e.into_inner());
    match keys.as_ref() {
        Some(b) => b.label(action),
        None => Bindings::default().label(action),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tutorial_reads_a_say_line_with_its_placeholders() {
        assert_eq!(
            parse_hint("~tut~ say 3/20 Press {gun} to fire. Then {bomb}."),
            Some(Hint::Say {
                step: 3,
                of: 20,
                text: "Press {gun} to fire. Then {bomb}.".into()
            })
        );
        assert_eq!(
            parse_hint("~tut~ say 20/20 Done"),
            Some(Hint::Say {
                step: 20,
                of: 20,
                text: "Done".into()
            })
        );
    }

    #[test]
    fn tutorial_reads_reveal_prompt_and_end() {
        for (name, part) in [
            ("radar", HudPart::Radar),
            ("energy", HudPart::Energy),
            ("statbox", HudPart::Statbox),
            ("items", HudPart::Items),
            ("chat", HudPart::Chat),
        ] {
            assert_eq!(
                parse_hint(&format!("~tut~ reveal {name}")),
                Some(Hint::Reveal(part))
            );
        }
        assert_eq!(
            parse_hint("~tut~ prompt skip"),
            Some(Hint::Prompt(Prompt::Skip))
        );
        assert_eq!(
            parse_hint("~tut~ prompt try"),
            Some(Hint::Prompt(Prompt::Try))
        );
        assert_eq!(parse_hint("~tut~ end list"), Some(Hint::End { list: true }));
        assert_eq!(
            parse_hint("~tut~ end none"),
            Some(Hint::End { list: false })
        );
    }

    #[test]
    fn tutorial_junk_is_not_a_hint() {
        for junk in [
            "",
            "say 1/2 hi",
            "~tut~",
            "~tut~ ",
            "~tut~say 1/2 hi",
            "~tut~ say",
            "~tut~ say 1 hi",
            "~tut~ say 0/2 hi",
            "~tut~ say 3/2 hi",
            "~tut~ say a/b hi",
            "~tut~ say -1/2 hi",
            "~tut~ reveal",
            "~tut~ reveal map",
            "~tut~ reveal radar extra",
            "~tut~ prompt",
            "~tut~ prompt later",
            "~tut~ end",
            "~tut~ end maybe",
            "~tut~ dance",
            "Vex: Press Ctrl to fire.",
        ] {
            assert_eq!(parse_hint(junk), None, "{junk:?}");
        }
    }

    fn logged(log: &ChatLog) -> Vec<String> {
        log.lines(200, 50).into_iter().map(|l| l.text).collect()
    }

    #[test]
    fn tutorial_hint_lines_are_not_logged() {
        let mut log = ChatLog::default();
        let hint = log_or_hint(
            &mut log,
            chat_type::ARENA,
            None,
            "~tut~ say 1/20 Press {gun}".into(),
        );
        assert_eq!(hint.as_deref(), Some("~tut~ say 1/20 Press {gun}"));
        // A hint the client doesn't know is still the page's, not the log's.
        assert!(log_or_hint(&mut log, chat_type::ARENA, None, "~tut~ later".into()).is_some());
        assert!(logged(&log).is_empty(), "{:?}", logged(&log));
        // The plain copy the zone sends with it stays in the chat.
        assert_eq!(
            log_or_hint(&mut log, chat_type::ARENA, None, "Vex: Press Ctrl".into()),
            None
        );
        // A player can't make a hint line: theirs come with a name, or as
        // another kind of message.
        for (kind, sender) in [
            (chat_type::PUBLIC, Some("pilot".to_string())),
            (chat_type::ARENA, Some("pilot".to_string())),
            (chat_type::PRIVATE, None),
        ] {
            assert_eq!(
                log_or_hint(&mut log, kind, sender, "~tut~ end list".into()),
                None
            );
        }
        let lines = logged(&log);
        assert_eq!(lines.len(), 4, "{lines:?}");
        assert_eq!(lines[0], "Vex: Press Ctrl");
    }

    #[test]
    fn tutorial_only_its_four_commands_are_accepted() {
        for ok in [
            "?skip",
            "?leave",
            "?tutorial",
            "?go 0",
            "?go duel2",
            "?go #mine",
        ] {
            assert_eq!(accept_command(ok).as_deref(), Some(ok), "{ok}");
        }
        assert_eq!(accept_command(" ?skip ").as_deref(), Some("?skip"));
        for bad in [
            "",
            "?go",
            "?go ",
            "?go a b",
            "?go duel;?kill",
            "?go abcdefghijklmnopq",
            "?tutorial swarm",
            "?skipall",
            "?SKIP",
            "hello",
            "?password x",
            ":pilot:hi",
            "?go\u{e9}",
        ] {
            assert_eq!(accept_command(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn tutorial_commands_queue_in_order_and_refusals_stay_out() {
        take_commands();
        queue_command("?skip").unwrap();
        assert!(queue_command("?kill").is_err());
        queue_command("?go duel").unwrap();
        assert_eq!(take_commands(), ["?skip", "?go duel"]);
        assert!(take_commands().is_empty());
    }

    #[test]
    fn tutorial_keys_are_the_players_own() {
        assert_eq!(key_for("nosuchaction"), "");
        let mut keys = Bindings::default();
        keys.set(
            Action::Gun,
            vec![super::super::input::Binding {
                key: winit::keyboard::KeyCode::KeyJ,
                mods: super::super::input::Mods::None,
            }],
        );
        note_keys(&keys);
        assert_eq!(key_for("gun"), "J");
        assert_eq!(key_for("GUN"), "J");
        assert_eq!(key_for("repel"), keys.label(Action::Repel));
    }
}
