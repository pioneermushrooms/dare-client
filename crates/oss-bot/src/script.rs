//! A bot's script: who it is, where it goes, how it behaves and what it says.
//!
//! One setting per line, `key value`, ended by a line `end` (or the end of the
//! file). Blank lines and lines starting with `#` are skipped:
//!
//! ```text
//! name Patrol One
//! password secret
//! arena duel
//! ship 3
//! freq 100
//! behaviour patrol
//! point 500 500
//! point 520 500
//! range 30
//! greet Hello {name}, shoot me if you can
//! announce 300 Type !rules for the rules
//! reply !rules First to ten kills wins
//! end
//! ```
//!
//! `behaviour` is `idle` (sits still), `patrol` (flies the `point`s in a loop and
//! never fires), `sentry` (holds the first `point` and fires at anyone within
//! `range` tiles), `fighter` (chases the nearest pilot and fires) or `swarm`
//! (holds a ring `range` tiles out from the nearest pilot, facing them, and fires
//! a bullet every 1.5 s at most; never afterburns or dodges). Points are
//! tiles, 0-1023. `greet` is said when a pilot arrives (`{name}` is their name),
//! `announce` every so many seconds (60 or more), and `reply` answers a chat line
//! that starts with the keyword, publicly or privately as it was asked. A bot
//! fights anyone on another freq, bot or pilot; an old `peers` line is still
//! accepted and does nothing.

/// How a bot flies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Behaviour {
    #[default]
    Idle,
    Patrol,
    Sentry,
    Fighter,
    /// The tutorial's training swarm: holds a ring `range` tiles out from the
    /// nearest pilot and fires slowly, with no dodging.
    Swarm,
}

impl Behaviour {
    fn from_name(s: &str) -> Option<Self> {
        Some(match s.to_ascii_lowercase().as_str() {
            "idle" => Self::Idle,
            "patrol" => Self::Patrol,
            "sentry" => Self::Sentry,
            "fighter" => Self::Fighter,
            "swarm" => Self::Swarm,
            _ => return None,
        })
    }
}

/// Everything a script sets.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Script {
    pub name: Option<String>,
    pub password: Option<String>,
    pub arena: Option<String>,
    /// 0-based ship.
    pub ship: Option<u8>,
    pub freq: Option<i16>,
    pub behaviour: Behaviour,
    /// Tiles.
    pub points: Vec<(i32, i32)>,
    /// Tiles.
    pub range: Option<i32>,
    pub greeting: Option<String>,
    /// (every so many seconds, text).
    pub announcements: Vec<(u32, String)>,
    pub banter: Option<(u32, Vec<String>)>,
    /// (keyword, lower case; reply).
    pub replies: Vec<(String, String)>,
    pub controller: Option<String>,
}

/// Most of each list a script may hold; anything past it is an error.
pub const MAX_POINTS: usize = 64;
pub const MAX_ANNOUNCEMENTS: usize = 10;
pub const MAX_REPLIES: usize = 20;
/// Longest line the bot will say.
pub const MAX_TEXT: usize = 200;

/// Makes `text` safe to say: printable ASCII only, at most [`MAX_TEXT`]
/// characters, and never starting with a character the zone would take as a
/// command. `None` when nothing is left.
pub fn speakable(text: &str) -> Option<String> {
    let s: String = text
        .chars()
        .filter(|c| c.is_ascii() && !c.is_ascii_control())
        .take(MAX_TEXT)
        .collect();
    let s = s.trim_start_matches(['?', '*', '/', ';', ':', '\'', '"', '=', ' ']);
    let s = s.trim_end();
    (!s.is_empty()).then(|| s.to_string())
}

/// Reads a script. Errors name the line.
pub fn parse(text: &str) -> Result<Script, String> {
    let mut s = Script::default();
    for (n, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let at = |e: &str| format!("line {}: {e}", n + 1);
        let (key, rest) = line.split_once(' ').unwrap_or((line, ""));
        let rest = rest.trim();
        let num = |v: &str, what: &str| {
            v.parse::<i64>()
                .map_err(|_| at(&format!("{what} must be a number")))
        };
        match key {
            "end" => break,
            "name" => s.name = Some(rest.to_string()).filter(|v| !v.is_empty()),
            "password" => s.password = Some(rest.to_string()),
            "arena" => s.arena = Some(rest.to_string()).filter(|v| !v.is_empty()),
            "ship" => {
                let v = num(rest, "ship")?;
                if !(1..=8).contains(&v) {
                    return Err(at("ship must be 1-8"));
                }
                s.ship = Some(v as u8 - 1);
            }
            "freq" => {
                let v = num(rest, "freq")?;
                if !(0..=9999).contains(&v) {
                    return Err(at("freq must be 0-9999"));
                }
                s.freq = Some(v as i16);
            }
            "behaviour" | "behavior" => {
                s.behaviour = Behaviour::from_name(rest)
                    .ok_or_else(|| at("behaviour must be idle, patrol, sentry, fighter or swarm"))?
            }
            "point" => {
                let mut it = rest.split_whitespace();
                let (Some(x), Some(y), None) = (it.next(), it.next(), it.next()) else {
                    return Err(at("point takes two numbers, x and y"));
                };
                let (x, y) = (num(x, "x")?, num(y, "y")?);
                if !(0..1024).contains(&x) || !(0..1024).contains(&y) {
                    return Err(at("points are tiles, 0-1023"));
                }
                if s.points.len() == MAX_POINTS {
                    return Err(at("too many points"));
                }
                s.points.push((x as i32, y as i32));
            }
            "range" => {
                let v = num(rest, "range")?;
                if !(1..=200).contains(&v) {
                    return Err(at("range must be 1-200 tiles"));
                }
                s.range = Some(v as i32);
            }
            // Bots once spared names ending in this; they now go by freq.
            "peers" => {}
            "controller" => s.controller = Some(rest.to_string()).filter(|v| !v.is_empty()),
            "greet" => s.greeting = speakable(rest),
            "banter" => {
                let (every, text) = rest.split_once(' ').unwrap_or((rest, ""));
                let every = num(every, "banter interval")?;
                if !(60..=86_400).contains(&every) {
                    return Err(at("banter is every 60-86400 seconds"));
                }
                let (_, phrases) = s.banter.get_or_insert((every as u32, Vec::new()));
                if phrases.len() == 8 {
                    return Err(at("too many banter lines"));
                }
                if let Some(text) = speakable(text) {
                    phrases.push(text);
                }
            }
            "announce" => {
                let (every, text) = rest.split_once(' ').unwrap_or((rest, ""));
                let every = num(every, "announce interval")?;
                if !(60..=86_400).contains(&every) {
                    return Err(at("announcements are every 60-86400 seconds"));
                }
                if s.announcements.len() == MAX_ANNOUNCEMENTS {
                    return Err(at("too many announcements"));
                }
                if let Some(t) = speakable(text) {
                    s.announcements.push((every as u32, t));
                }
            }
            "reply" => {
                let (word, text) = rest.split_once(' ').unwrap_or((rest, ""));
                if word.is_empty() || word.len() > 24 {
                    return Err(at("a reply needs a keyword of 1-24 characters"));
                }
                if s.replies.len() == MAX_REPLIES {
                    return Err(at("too many replies"));
                }
                if let Some(t) = speakable(text) {
                    s.replies.push((word.to_ascii_lowercase(), t));
                }
            }
            other => return Err(at(&format!("unknown setting {other:?}"))),
        }
    }
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_script() {
        let s = parse(
            "# a comment\nname Rex [bot]\npassword p w\narena duel\nship 3\nfreq 100\n\
             behaviour Fighter\npoint 1 2\npoint 1023 0\nrange 30\ngreet  Hi {name}\n\
             announce 60 ?go elsewhere\nreply !Rules no camping\npeers  [bot]\nend\nname ignored",
        )
        .unwrap();
        assert_eq!(s.name.as_deref(), Some("Rex [bot]"));
        assert_eq!(s.password.as_deref(), Some("p w"));
        assert_eq!((s.ship, s.freq), (Some(2), Some(100)));
        assert_eq!(s.behaviour, Behaviour::Fighter);
        assert_eq!(s.points, vec![(1, 2), (1023, 0)]);
        assert_eq!(s.greeting.as_deref(), Some("Hi {name}"));
        // A line that would be a command loses the command character.
        assert_eq!(s.announcements, vec![(60, "go elsewhere".to_string())]);
        assert_eq!(
            s.replies,
            vec![("!rules".to_string(), "no camping".to_string())]
        );
    }

    #[test]
    fn refuses_bad_lines() {
        for bad in [
            "ship 9",
            "point 1",
            "point 1 2 3",
            "point -1 5",
            "behaviour dance",
            "announce 5 too often",
            "range 0",
            "colour red",
        ] {
            assert!(parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn says_only_safe_text() {
        assert_eq!(speakable("*arena hi"), Some("arena hi".into()));
        assert_eq!(speakable("  ?? "), None);
        assert_eq!(speakable("caf\u{e9}\u{7} ok"), Some("caf ok".into()));
        assert_eq!(speakable(&"x".repeat(500)).unwrap().len(), MAX_TEXT);
    }
}
