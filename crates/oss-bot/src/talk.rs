//! What a bot says: a greeting when a pilot arrives, announcements on a timer,
//! and replies to keywords. Everything goes through one queue that sends at
//! most one line every [`GAP_MS`], so a bot can't flood the arena.

use std::collections::{HashMap, VecDeque};

use oss_session::chat_type;

use crate::script::{Script, speakable};

/// Least time between two lines the bot says.
pub const GAP_MS: u64 = 1500;
/// Lines waiting beyond this are dropped.
const QUEUE: usize = 6;
/// A pilot is greeted again only after this long.
const REGREET_MS: u64 = 10 * 60 * 1000;
/// The same keyword is answered at most this often.
const REPLY_COOLDOWN_MS: u64 = 5000;

/// A line to send: (chat type, target pid for a private line, text).
pub type Line = (u8, i16, String);

pub struct Talk {
    greeting: Option<String>,
    /// (every ms, text, next due ms).
    announcements: Vec<(u64, String, u64)>,
    replies: Vec<(String, String)>,
    greeted: HashMap<String, u64>,
    answered: HashMap<usize, u64>,
    queue: VecDeque<Line>,
    last_sent: Option<u64>,
}

impl Talk {
    pub fn new(script: &Script, now: u64) -> Self {
        Talk {
            greeting: script.greeting.clone(),
            announcements: script
                .announcements
                .iter()
                .map(|(s, t)| (*s as u64 * 1000, t.clone(), now + *s as u64 * 1000))
                .collect(),
            replies: script.replies.clone(),
            greeted: HashMap::new(),
            answered: HashMap::new(),
            queue: VecDeque::new(),
            last_sent: None,
        }
    }

    /// Whether the bot has anything to say at all.
    pub fn is_quiet(&self) -> bool {
        self.greeting.is_none() && self.announcements.is_empty() && self.replies.is_empty()
    }

    fn push(&mut self, line: Line) {
        if self.queue.len() < QUEUE {
            self.queue.push_back(line);
        }
    }

    /// A pilot called `name` is in the arena.
    pub fn arrived(&mut self, name: &str, now: u64) {
        let Some(g) = self.greeting.as_ref() else {
            return;
        };
        let key = name.to_ascii_lowercase();
        if self
            .greeted
            .get(&key)
            .is_some_and(|&t| now.saturating_sub(t) < REGREET_MS)
        {
            return;
        }
        if let Some(text) = speakable(&g.replace("{name}", name)) {
            self.greeted.insert(key, now);
            self.push((chat_type::PUBLIC, 0, text));
        }
    }

    /// Someone said `text`; `private` lines are answered privately to `from`.
    pub fn heard(&mut self, from: i16, private: bool, text: &str, now: u64) {
        let said = text.trim().to_ascii_lowercase();
        let Some(i) = self.replies.iter().position(|(word, _)| {
            said == *word
                || said
                    .strip_prefix(word.as_str())
                    .is_some_and(|r| r.starts_with(' '))
        }) else {
            return;
        };
        if !private
            && self
                .answered
                .get(&i)
                .is_some_and(|&t| now.saturating_sub(t) < REPLY_COOLDOWN_MS)
        {
            return;
        }
        self.answered.insert(i, now);
        let text = self.replies[i].1.clone();
        if private {
            self.push((chat_type::PRIVATE, from, text));
        } else {
            self.push((chat_type::PUBLIC, 0, text));
        }
    }

    /// The next line to send now, if one is due.
    pub fn next(&mut self, now: u64) -> Option<Line> {
        for i in 0..self.announcements.len() {
            let (every, ref text, due) = self.announcements[i];
            if now >= due {
                let text = text.clone();
                self.announcements[i].2 = now + every;
                self.push((chat_type::PUBLIC, 0, text));
            }
        }
        if self
            .last_sent
            .is_some_and(|t| now.saturating_sub(t) < GAP_MS)
        {
            return None;
        }
        let line = self.queue.pop_front()?;
        self.last_sent = Some(now);
        Some(line)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::script::parse;

    fn talk() -> Talk {
        let s = parse("greet Hi {name}\nannounce 60 news\nreply !rules be nice").unwrap();
        Talk::new(&s, 0)
    }

    #[test]
    fn greets_once_and_paces_lines() {
        let mut t = talk();
        t.arrived("Bob", 0);
        t.arrived("bob", 1000);
        t.arrived("Ann", 1000);
        assert_eq!(t.next(1000), Some((chat_type::PUBLIC, 0, "Hi Bob".into())));
        assert_eq!(t.next(1100), None);
        assert_eq!(t.next(2600).map(|l| l.2), Some("Hi Ann".into()));
        assert_eq!(t.next(5000), None);
        // Announcements come round on their timer.
        assert_eq!(t.next(60_000).map(|l| l.2), Some("news".into()));
        assert_eq!(t.next(100_000), None);
        assert_eq!(t.next(120_000).map(|l| l.2), Some("news".into()));
        // A pilot is greeted again after a long absence.
        t.arrived("Bob", 1_000_000);
        assert_eq!(t.next(1_000_000).map(|l| l.2), Some("Hi Bob".into()));
    }

    #[test]
    fn answers_keywords() {
        let mut t = talk();
        t.heard(7, false, "!RULES please", 0);
        t.heard(7, false, "!rules", 10);
        t.heard(7, false, "!rulesx", 20_000);
        t.heard(9, true, "!rules", 30);
        assert_eq!(t.next(0), Some((chat_type::PUBLIC, 0, "be nice".into())));
        assert_eq!(
            t.next(5000),
            Some((chat_type::PRIVATE, 9, "be nice".into()))
        );
        assert_eq!(t.next(9000), None);
        assert!(!t.is_quiet());
    }
}
