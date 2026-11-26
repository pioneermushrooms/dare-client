//! The host protocol: one `lineup` message per arena, and what changed
//! between two of them.

use crate::script::{self, Script};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Seat {
    In,
    Spectating,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub name: String,
    pub password: String,
    pub seat: Seat,
    pub team: i16,
    pub script: Script,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Lineup {
    pub arena: String,
    pub controller: Option<String>,
    pub bots: Vec<Entry>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    Add(Entry),
    Remove(String),
    Restart(Entry),
    Seat(String, Seat),
    Team(String, i16),
}

/// A bot block being read.
struct Block {
    name: String,
    password: String,
    seat: Seat,
    team: i16,
    script: String,
}

impl Block {
    fn finish(self) -> Result<Entry, String> {
        let script = script::parse(&self.script).map_err(|e| format!("bot {}: {e}", self.name))?;
        Ok(Entry {
            name: self.name,
            password: self.password,
            seat: self.seat,
            team: self.team,
            script,
        })
    }
}

/// Reads one lineup message (`lineup ...` through `done`).
pub fn read(text: &str) -> Result<Lineup, String> {
    let mut arena = None;
    let mut controller = None;
    let mut bots = Vec::new();
    let mut block: Option<Block> = None;
    let mut done = false;
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || (line.starts_with('#') && block.is_none()) {
            continue;
        }
        let (key, rest) = line.split_once(' ').unwrap_or((line, ""));
        let rest = rest.trim();
        if done {
            return Err("text after done".into());
        }
        let Some(b) = block.as_mut() else {
            match key {
                "lineup" => arena = Some(rest.to_string()),
                "controller" => controller = Some(rest.to_string()).filter(|v| !v.is_empty()),
                "bot" => {
                    if rest.is_empty() {
                        return Err("bot needs a name".into());
                    }
                    block = Some(Block {
                        name: rest.to_string(),
                        password: String::new(),
                        seat: Seat::In,
                        team: 0,
                        script: String::new(),
                    });
                }
                "done" => done = true,
                _ => return Err(format!("unexpected line outside a bot: {line}")),
            }
            continue;
        };
        match key {
            "end" => {
                if let Some(b) = block.take() {
                    bots.push(b.finish()?);
                }
            }
            "password" => b.password = rest.to_string(),
            "seat" => {
                b.seat = match rest {
                    "in" => Seat::In,
                    "spec" => Seat::Spectating,
                    _ => return Err(format!("bot {}: seat must be in or spec", b.name)),
                }
            }
            "team" => {
                b.team = rest
                    .parse()
                    .ok()
                    .filter(|t| (0..=9999).contains(t))
                    .ok_or_else(|| format!("bot {}: team must be 0-9999", b.name))?
            }
            "bot" | "lineup" | "done" => {
                return Err(format!("bot {}: missing end before {key}", b.name));
            }
            _ => {
                b.script.push_str(line);
                b.script.push('\n');
            }
        }
    }
    if let Some(b) = block {
        return Err(format!("bot {}: missing end", b.name));
    }
    if !done {
        return Err("missing done".into());
    }
    let arena = arena.ok_or("missing lineup line")?;
    Ok(Lineup {
        arena,
        controller,
        bots,
    })
}

/// What the host must do to go from `old` to `new`. Names compare
/// case-insensitively; a password change alone is nothing.
pub fn diff(old: &Lineup, new: &Lineup) -> Vec<Change> {
    let find = |l: &'_ Lineup, name: &str| {
        l.bots
            .iter()
            .position(|e| e.name.eq_ignore_ascii_case(name))
    };
    let mut out = Vec::new();
    for e in &old.bots {
        if find(new, &e.name).is_none() {
            out.push(Change::Remove(e.name.clone()));
        }
    }
    for n in &new.bots {
        let Some(i) = find(old, &n.name) else {
            out.push(Change::Add(n.clone()));
            continue;
        };
        let o = &old.bots[i];
        if o.script != n.script {
            out.push(Change::Restart(n.clone()));
            continue;
        }
        if o.seat != n.seat {
            out.push(Change::Seat(n.name.clone(), n.seat));
        }
        if o.team != n.team {
            out.push(Change::Team(n.name.clone(), n.team));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(name: &str, extra: &str) -> String {
        format!("bot {name}\npassword pw-{name}\nseat in\nteam 1\nship 2\n{extra}end\n")
    }

    fn msg(blocks: &[String]) -> String {
        format!("lineup duel\ncontroller Chris\n{}done\n", blocks.concat())
    }

    fn lineup(blocks: &[String]) -> Lineup {
        read(&msg(blocks)).unwrap()
    }

    fn same(mut a: Vec<Change>, mut b: Vec<Change>) -> bool {
        let key = |c: &Change| format!("{c:?}");
        a.sort_by_key(key);
        b.sort_by_key(key);
        a == b
    }

    #[test]
    fn reads_a_lineup_with_two_bots() {
        let l = read(
            "lineup duel\ncontroller Chris\n\
             bot Red Baron\npassword abc\nseat in\nteam 3\nship 4\nbehaviour fighter\nend\n\
             bot Blue\npassword def\nseat spec\nteam 7\nend\ndone\n",
        )
        .unwrap();
        assert_eq!(l.arena, "duel");
        assert_eq!(l.controller.as_deref(), Some("Chris"));
        assert_eq!(l.bots.len(), 2);
        let a = &l.bots[0];
        assert_eq!(a.name, "Red Baron");
        assert_eq!(a.password, "abc");
        assert_eq!(a.seat, Seat::In);
        assert_eq!(a.team, 3);
        assert_eq!(a.script.ship, Some(3));
        assert_eq!(a.script.behaviour, script::Behaviour::Fighter);
        assert_eq!(l.bots[1].seat, Seat::Spectating);
        assert_eq!(l.bots[1].team, 7);
    }

    #[test]
    fn diff_adds_removes_and_restarts_only_what_changed() {
        let old = lineup(&[block("A", ""), block("B", ""), block("C", "")]);
        let b_spec = block("B", "").replace("seat in", "seat spec");
        let new = lineup(&[
            block("A", "").replace("ship 2", "ship 5"),
            b_spec,
            block("D", ""),
        ]);
        let got = diff(&old, &new);
        let want = vec![
            Change::Restart(new.bots[0].clone()),
            Change::Seat("B".into(), Seat::Spectating),
            Change::Remove("C".into()),
            Change::Add(new.bots[2].clone()),
        ];
        assert!(same(got, want));
    }

    #[test]
    fn a_new_password_alone_changes_nothing() {
        let old = lineup(&[block("A", "")]);
        let new = lineup(&[block("A", "").replace("pw-A", "other")]);
        assert!(diff(&old, &new).is_empty());
    }

    #[test]
    fn team_change_is_its_own_change() {
        let old = lineup(&[block("A", "")]);
        let new = lineup(&[block("A", "").replace("team 1", "team 9")]);
        assert_eq!(diff(&old, &new), vec![Change::Team("A".into(), 9)]);
    }

    #[test]
    fn names_compare_case_insensitively() {
        let old = lineup(&[block("Bot One", "")]);
        let new = lineup(&[block("bot one", "")]);
        assert!(diff(&old, &new).is_empty());
    }

    #[test]
    fn bad_script_line_names_the_bot() {
        let e = read("lineup x\nbot Zed\nship 99\nend\ndone\n").unwrap_err();
        assert!(e.contains("Zed"), "{e}");
    }
}
