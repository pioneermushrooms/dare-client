//! The zone's bot list for the in-game bots window.
//!
//! `?bots data` answers with lines that start with a tab, then `bots`, then a tab (the zone keeps
//! tabs in its own messages but turns tabs in players' chat into `_`, so a pilot can't fake one).
//! Fields are tab-separated: a `head` line, one `bot` line per bot, one `match` line per team
//! size, then `end`. The lines never reach the chat log; a list replaces the shown one only when
//! its `end` arrives, so a cut-off answer never shows half a list.

/// What starts every data line.
pub const MARKER: &str = "\tbots\t";

/// One bot as the zone reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BotRow {
    pub name: String,
    pub behaviour: String,
    pub ship: u8,
    pub team: i32,
    pub priority: u8,
    pub chat: bool,
    pub deployed: bool,
    /// in, spectating, out, stopped or not launched.
    pub status: String,
    pub problem: Option<String>,
}

/// One complete answer.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BotsView {
    pub arena: String,
    /// Whether this pilot owns the arena (and so may change things).
    pub owner: bool,
    pub bots: Vec<BotRow>,
    /// (team, match size), in team order.
    pub matches: Vec<(i32, i32)>,
}

/// Collects data lines as they arrive.
#[derive(Debug, Default)]
pub struct BotsFeed {
    building: Option<BotsView>,
    latest: Option<BotsView>,
    /// Whether `latest` arrived since the last `take_fresh`.
    fresh: bool,
}

impl BotsFeed {
    /// A server chat line. True when it was a data line (swallow it, even a malformed one).
    pub fn line(&mut self, text: &str) -> bool {
        let Some(rest) = text.strip_prefix(MARKER) else {
            return false;
        };
        let fields: Vec<&str> = rest.split('\t').collect();
        match fields.as_slice() {
            ["head", arena, owner] => {
                self.building = Some(BotsView {
                    arena: arena.to_string(),
                    owner: *owner == "yes",
                    ..Default::default()
                });
            }
            [
                "bot",
                name,
                behaviour,
                ship,
                team,
                priority,
                chat,
                deployed,
                status,
                problem,
            ] => {
                if let (Some(view), Ok(ship), Ok(team), Ok(priority)) = (
                    self.building.as_mut(),
                    ship.parse(),
                    team.parse(),
                    priority.parse(),
                ) {
                    view.bots.push(BotRow {
                        name: name.to_string(),
                        behaviour: behaviour.to_string(),
                        ship,
                        team,
                        priority,
                        chat: *chat == "on",
                        deployed: *deployed == "yes",
                        status: status.to_string(),
                        problem: (!problem.is_empty()).then(|| problem.to_string()),
                    });
                }
            }
            ["match", team, size] => {
                if let (Some(view), Ok(team), Ok(size)) =
                    (self.building.as_mut(), team.parse(), size.parse())
                {
                    view.matches.push((team, size));
                }
            }
            ["end"] => {
                if let Some(view) = self.building.take() {
                    self.latest = Some(view);
                    self.fresh = true;
                }
            }
            _ => {}
        }
        true
    }

    /// The newest complete answer, once: None until another one finishes.
    pub fn take_fresh(&mut self) -> Option<BotsView> {
        if !std::mem::take(&mut self.fresh) {
            return None;
        }
        self.latest.clone()
    }

    /// The last complete answer.
    #[allow(dead_code)] // kept for tests and callers that only peek
    pub fn latest(&self) -> Option<&BotsView> {
        self.latest.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(lines: &[&str]) -> BotsFeed {
        let mut f = BotsFeed::default();
        for l in lines {
            assert!(f.line(l), "{l:?} should be swallowed");
        }
        f
    }

    #[test]
    fn parses_a_full_answer() {
        let f = feed(&[
            "\tbots\thead\tbotlab\tyes",
            "\tbots\tbot\tSparring\tfighter\t3\t0\t5\ton\tyes\tin\t",
            "\tbots\tbot\tPatrol\tpatrol\t1\t2\t9\toff\tno\tstopped\t",
            "\tbots\tmatch\t0\t3",
            "\tbots\tend",
        ]);
        let v = f.latest().expect("a complete answer");
        assert_eq!(v.arena, "botlab");
        assert!(v.owner);
        assert_eq!(v.matches, vec![(0, 3)]);
        assert_eq!(
            v.bots[0],
            BotRow {
                name: "Sparring".into(),
                behaviour: "fighter".into(),
                ship: 3,
                team: 0,
                priority: 5,
                chat: true,
                deployed: true,
                status: "in".into(),
                problem: None,
            }
        );
        assert_eq!(v.bots[1].status, "stopped");
        assert!(!v.bots[1].deployed);
    }

    #[test]
    fn parses_a_name_with_spaces_and_an_empty_problem() {
        let f = feed(&[
            "\tbots\thead\ta\tno",
            "\tbots\tbot\tBig Bob\tsentry\t8\t9999\t1\toff\tyes\tnot launched\tNot launched: a pilot uses this name",
            "\tbots\tbot\tX\tidle\t1\t0\t5\toff\tyes\tout\t",
            "\tbots\tend",
        ]);
        let v = f.latest().unwrap();
        assert!(!v.owner);
        assert_eq!(v.bots[0].name, "Big Bob");
        assert_eq!(v.bots[0].team, 9999);
        assert_eq!(
            v.bots[0].problem.as_deref(),
            Some("Not launched: a pilot uses this name")
        );
        assert_eq!(v.bots[1].problem, None);
    }

    #[test]
    fn an_unfinished_answer_keeps_the_last_list() {
        let mut f = feed(&[
            "\tbots\thead\ta\tyes",
            "\tbots\tbot\tOne\tidle\t1\t0\t5\toff\tyes\tin\t",
            "\tbots\tend",
        ]);
        assert!(f.line("\tbots\thead\ta\tyes"));
        assert!(f.line("\tbots\tbot\tTwo\tidle\t1\t0\t5\toff\tyes\tin\t"));
        // no end yet: still the first list
        assert_eq!(f.latest().unwrap().bots.len(), 1);
        assert_eq!(f.latest().unwrap().bots[0].name, "One");
        assert!(f.line("\tbots\tend"));
        assert_eq!(f.latest().unwrap().bots[0].name, "Two");
    }

    #[test]
    fn a_fresh_answer_is_handed_over_once() {
        let mut f = feed(&["\tbots\thead\ta\tyes", "\tbots\tend"]);
        assert_eq!(f.take_fresh().map(|v| v.arena), Some("a".to_string()));
        assert!(f.take_fresh().is_none());
        assert!(f.line("\tbots\thead\tb\tyes"));
        assert!(f.take_fresh().is_none(), "not finished yet");
        assert!(f.line("\tbots\tend"));
        assert_eq!(f.take_fresh().map(|v| v.arena), Some("b".to_string()));
    }

    #[test]
    fn ignores_ordinary_chat() {
        let mut f = BotsFeed::default();
        assert!(!f.line("Bots: created Sparring (fighter, Spider)."));
        assert!(!f.line("bots\thead\ta\tyes"));
        assert!(f.latest().is_none());
    }

    #[test]
    fn a_malformed_line_is_swallowed_but_ignored() {
        let mut f = feed(&["\tbots\thead\ta\tyes", "\tbots\tend"]);
        assert!(f.line("\tbots\tbot\tonly\ttwo"));
        assert!(f.line("\tbots\twhat"));
        assert!(f.line("\tbots\tend"));
        assert_eq!(f.latest().unwrap().bots.len(), 0);
    }
}
