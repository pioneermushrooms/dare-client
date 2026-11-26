//! Foundation's arenas, as the browser start screen lists them in place of zones.
//!
//! The page fetches the list and hands it over as text, one arena per line:
//! `name<TAB>players`, with the count empty when the server has none. That text is
//! untrusted: bad lines are skipped and nothing here panics on it.

// Only the browser build lists arenas so far; the desktop build keeps its zone list.
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

use oss_session::directory::ZoneInfo;

/// The arena the server puts you in when you name none.
pub const PUBLIC: &str = "public";
/// Longest arena name kept from the feed (the server's own limit is 15).
const MAX_NAME: usize = 16;
/// Most arenas kept from one feed.
const MAX_ROWS: usize = 200;

/// One arena in the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArenaRow {
    pub name: String,
    /// Pilots in it, when the server said.
    pub players: Option<u32>,
}

/// How the list is ordered (after `public`, which always comes first).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ArenaSort {
    /// Busiest first.
    #[default]
    Pilots,
    Name,
}

impl ArenaSort {
    /// The other order.
    pub fn next(self) -> Self {
        match self {
            ArenaSort::Pilots => ArenaSort::Name,
            ArenaSort::Name => ArenaSort::Pilots,
        }
    }
}

/// Reads the page's arena text: one `name<TAB>players` per line.
pub fn parse_rows(text: &str) -> Vec<ArenaRow> {
    text.lines()
        .filter_map(|line| {
            let mut fields = line.split('\t');
            let name = fields.next()?.trim();
            let players = fields.next().unwrap_or("").trim();
            if name.is_empty() || fields.next().is_some() {
                return None;
            }
            Some(ArenaRow {
                name: name.chars().take(MAX_NAME).collect(),
                players: players.parse().ok(),
            })
        })
        .take(MAX_ROWS)
        .collect()
}

/// The arena list the start screen shows.
#[derive(Debug, Clone, Default)]
pub struct ArenaList {
    pub rows: Vec<ArenaRow>,
    pub sort: ArenaSort,
    /// The last fetch failed; the rows are from the last one that worked.
    pub unavailable: bool,
}

impl ArenaList {
    /// Takes a new feed. `None` means the fetch failed, which keeps the rows.
    pub fn set_feed(&mut self, text: Option<&str>) {
        match text {
            Some(text) => {
                self.rows = parse_rows(text);
                self.unavailable = false;
            }
            None => self.unavailable = true,
        }
    }

    /// The rows in order: `public` first, then the rest by the chosen sort.
    pub fn shown(&self) -> Vec<ArenaRow> {
        let public = ArenaRow {
            name: PUBLIC.into(),
            players: self
                .rows
                .iter()
                .find(|r| r.name == PUBLIC)
                .and_then(|r| r.players),
        };
        let mut rest: Vec<ArenaRow> = self
            .rows
            .iter()
            .filter(|r| r.name != PUBLIC)
            .cloned()
            .collect();
        let by_name =
            |a: &ArenaRow, b: &ArenaRow| a.name.to_lowercase().cmp(&b.name.to_lowercase());
        match self.sort {
            ArenaSort::Pilots => rest.sort_by(|a, b| {
                b.players
                    .unwrap_or(0)
                    .cmp(&a.players.unwrap_or(0))
                    .then_with(|| by_name(a, b))
            }),
            ArenaSort::Name => rest.sort_by(by_name),
        }
        std::iter::once(public).chain(rest).collect()
    }

    /// The rows as zone entries, for the zone list's drawing: the arena name is
    /// both the name and the "address" the selection follows.
    pub fn as_zones(&self) -> Vec<ZoneInfo> {
        self.shown()
            .into_iter()
            .map(|r| ZoneInfo {
                address: r.name.clone(),
                name: r.name,
                description: String::new(),
                players: r.players.unwrap_or(0),
                ping_ms: None,
            })
            .collect()
    }
}

/// The login form's arena for a listed arena: `public` is the server's default.
pub fn arena_for_login(name: &str) -> String {
    if name == PUBLIC {
        String::new()
    } else {
        name.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(name: &str, players: Option<u32>) -> ArenaRow {
        ArenaRow {
            name: name.into(),
            players,
        }
    }

    #[test]
    fn parse_rows_reads_names_and_counts() {
        assert_eq!(
            parse_rows("duel\t3\nskyrace\t\n"),
            vec![row("duel", Some(3)), row("skyrace", None)]
        );
    }

    #[test]
    fn parse_rows_skips_bad_lines() {
        let long = "a".repeat(40);
        let text = format!("\n\t5\na\tb\tc\n{long}\t2\n  \t1\nok\t-4\n");
        assert_eq!(
            parse_rows(&text),
            vec![row(&"a".repeat(16), Some(2)), row("ok", None)]
        );
    }

    #[test]
    fn parse_rows_stops_at_200() {
        let text: String = (0..300).map(|i| format!("a{i}\t1\n")).collect();
        assert_eq!(parse_rows(&text).len(), 200);
    }

    #[test]
    fn shown_puts_public_first_and_sorts() {
        let mut list = ArenaList::default();
        list.set_feed(Some("small\t1\nzero\t\nbig\t9\n"));
        let names = |l: &ArenaList| l.shown().into_iter().map(|r| r.name).collect::<Vec<_>>();
        assert_eq!(names(&list), ["public", "big", "small", "zero"]);
        list.sort = list.sort.next();
        assert_eq!(list.sort, ArenaSort::Name);
        list.set_feed(Some("Zed\t9\nalpha\t1\nbeta\t\n"));
        assert_eq!(names(&list), ["public", "alpha", "beta", "Zed"]);
        assert_eq!(list.sort.next(), ArenaSort::Pilots);
    }

    #[test]
    fn public_line_gives_its_count_once() {
        let mut list = ArenaList::default();
        list.set_feed(Some("duel\t2\npublic\t7\n"));
        assert_eq!(
            list.shown(),
            vec![row("public", Some(7)), row("duel", Some(2))]
        );
    }

    #[test]
    fn unavailable_still_lists_public() {
        let mut list = ArenaList::default();
        list.set_feed(None);
        assert!(list.unavailable);
        assert_eq!(list.shown(), vec![row("public", None)]);
    }

    #[test]
    fn a_failed_feed_keeps_the_last_good_rows() {
        let mut list = ArenaList::default();
        list.set_feed(Some("duel\t2\n"));
        list.set_feed(None);
        assert!(list.unavailable);
        assert_eq!(list.shown().len(), 2);
        list.set_feed(Some(""));
        assert!(!list.unavailable);
    }

    #[test]
    fn as_zones_uses_the_name_as_address() {
        let mut list = ArenaList::default();
        list.set_feed(Some("duel\t2\n"));
        let zones = list.as_zones();
        assert_eq!(zones[1].name, "duel");
        assert_eq!(zones[1].address, "duel");
        assert_eq!(zones[1].players, 2);
        assert_eq!(zones[0].players, 0);
        assert_eq!(zones[0].ping_ms, None);
    }

    #[test]
    fn arena_for_login_maps_public_to_empty() {
        assert_eq!(arena_for_login("public"), "");
        assert_eq!(arena_for_login("duel"), "duel");
    }
}
