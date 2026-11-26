//! `--host`: runs an arena's bots as lineup messages on standard input list
//! them, each on its own thread with its own connection, until standard input
//! closes. `--script` and `--count` bots are described as a lineup too.

use std::io::BufRead;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::time::Duration;

use oss_session::map::MapCache;
use oss_session::{SPECTATOR, State};

use crate::lineup::{self, Change, Entry, Lineup, Seat};
use crate::script::Script;
use crate::{Bot, Options, Siblings, start_bot};

/// Where a lineup's bots go: its arena (empty for any public one) and the
/// arena owner they take flight orders from.
#[derive(Clone)]
pub struct Place {
    pub arena: String,
    pub controller: Option<String>,
}

impl Place {
    pub fn of(lineup: &Lineup) -> Self {
        Place {
            arena: lineup.arena.clone(),
            controller: lineup.controller.clone(),
        }
    }
}

/// The bots the options (and a `--script`) describe, as a lineup: one bot, or
/// `--count` of them numbered.
pub fn lineup_from_options(opts: &Options, script: Option<&Script>) -> Lineup {
    let script = script.cloned().unwrap_or_default();
    let ship = script.ship.unwrap_or(opts.ship);
    let bots = (0..opts.count)
        .map(|i| Entry {
            name: script.name.clone().unwrap_or_else(|| {
                if opts.count > 1 {
                    format!("{}{}", opts.name, i + 1)
                } else {
                    opts.name.clone()
                }
            }),
            password: script.password.clone().unwrap_or(opts.password.clone()),
            seat: if ship == SPECTATOR {
                Seat::Spectating
            } else {
                Seat::In
            },
            team: script.freq.unwrap_or(0),
            script: Script {
                ship: (ship != SPECTATOR).then_some(ship),
                ..script.clone()
            },
        })
        .collect();
    Lineup {
        arena: script
            .arena
            .clone()
            .or(opts.arena.clone())
            .unwrap_or_default(),
        controller: script.controller.clone(),
        bots,
    }
}

/// What the host tells a running bot.
#[derive(Debug, Clone, PartialEq)]
enum Order {
    Seat(Seat),
    /// The zone sets the bot's freq itself; the bot only hears about it.
    Team(i16),
    Controller(Option<String>),
    /// A fresh one-time pass: a bot whose session has closed logs in again.
    Password(String),
    Leave,
}

/// One thing the host does to go from one lineup to the next.
#[derive(Debug, Clone, PartialEq)]
enum Step {
    Start(Box<Entry>),
    Stop(String),
    Tell(String, Order),
}

/// What the host does to go from `old` to `new`, `old` being what it runs.
fn plan(old: &Lineup, new: &Lineup) -> Vec<Step> {
    let mut steps = Vec::new();
    if !new.arena.eq_ignore_ascii_case(&old.arena) {
        // Another arena: every bot goes again.
        steps.extend(old.bots.iter().map(|e| Step::Stop(e.name.clone())));
        steps.extend(new.bots.iter().cloned().map(|e| Step::Start(Box::new(e))));
        return steps;
    }
    if new.controller != old.controller {
        for e in &old.bots {
            let order = Order::Controller(new.controller.clone());
            steps.push(Step::Tell(e.name.clone(), order));
        }
    }
    for change in lineup::diff(old, new) {
        match change {
            Change::Add(entry) => steps.push(Step::Start(Box::new(entry))),
            Change::Remove(name) => steps.push(Step::Stop(name)),
            Change::Restart(entry) => {
                steps.push(Step::Stop(entry.name.clone()));
                steps.push(Step::Start(Box::new(entry)));
            }
            Change::Seat(name, seat) => steps.push(Step::Tell(name, Order::Seat(seat))),
            Change::Team(name, team) => steps.push(Step::Tell(name, Order::Team(team))),
        }
    }
    // A new pass alone is no change to the lineup, but a bot that has lost
    // its connection needs it to come back.
    for n in &new.bots {
        let restarted = steps
            .iter()
            .any(|s| matches!(s, Step::Start(e) if e.name.eq_ignore_ascii_case(&n.name)));
        let old_pass = old
            .bots
            .iter()
            .find(|o| o.name.eq_ignore_ascii_case(&n.name))
            .map(|o| &o.password);
        if !restarted && old_pass.is_some_and(|p| *p != n.password) {
            steps.push(Step::Tell(
                n.name.clone(),
                Order::Password(n.password.clone()),
            ));
        }
    }
    steps
}

/// A bot the host runs: the way to tell it things, and its thread.
struct Hosted {
    name: String,
    orders: Sender<Order>,
    thread: std::thread::JoinHandle<()>,
}

/// A hosted bot's thread: connects, then works until told to leave (or the
/// host goes away). Once its session has closed (kicked, zone lost, login
/// refused) it waits, and logs in again when a fresh pass arrives.
fn run_hosted(
    mut entry: Entry,
    mut place: Place,
    opts: Arc<Options>,
    cache: Arc<MapCache>,
    n: usize,
    siblings: Siblings,
    orders: Receiver<Order>,
) {
    let connect =
        |entry: &Entry, place: &Place| match start_bot(entry, place, &opts, n, siblings.clone()) {
            Ok(b) => Some(b),
            Err(e) => {
                log::warn!("[{}] {e}", entry.name);
                None
            }
        };
    let mut bot: Option<Bot> = connect(&entry, &place);
    loop {
        let mut fresh_pass = false;
        loop {
            match orders.try_recv() {
                Ok(Order::Seat(seat)) => {
                    entry.seat = seat;
                    if let Some(b) = bot.as_mut() {
                        b.take_seat(seat);
                    }
                }
                // Nothing to do: the zone sets the freq, not the bot.
                Ok(Order::Team(team)) => {
                    entry.team = team;
                    if let Some(b) = bot.as_ref() {
                        b.log(format_args!("the zone puts me on freq {team}"));
                    }
                }
                Ok(Order::Controller(c)) => {
                    place.controller = c.clone();
                    if let Some(b) = bot.as_mut() {
                        b.controller = c;
                        b.flight_order = None;
                    }
                }
                Ok(Order::Password(p)) => {
                    entry.password = p;
                    fresh_pass = true;
                }
                Ok(Order::Leave) | Err(TryRecvError::Disconnected) => {
                    if let Some(b) = bot.as_mut() {
                        b.leave();
                        b.log("left");
                    }
                    return;
                }
                Err(TryRecvError::Empty) => break,
            }
        }
        let closed = bot
            .as_ref()
            .is_none_or(|b| b.session.state() == State::Closed);
        // A new pass for a bot still in the zone does nothing.
        if closed && fresh_pass {
            log::info!("[{}] logging in again", entry.name);
            bot = connect(&entry, &place);
        }
        let open = bot.as_mut().is_some_and(|b| b.step(&cache, &opts.chat));
        std::thread::sleep(Duration::from_millis(if open { 5 } else { 50 }));
    }
}

/// Reads lineup messages from `input`, handing each to `on` at its `done`,
/// until `input` ends. A line that is not text spoils its message, which is
/// then reported as malformed; only the end of input stops the reading.
fn read_messages(mut input: impl BufRead, mut on: impl FnMut(Result<Lineup, String>)) {
    let (mut text, mut spoiled, mut raw) = (String::new(), None, Vec::new());
    loop {
        raw.clear();
        match input.read_until(b'\n', &mut raw) {
            Ok(0) => return,
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => {
                log::warn!("standard input: {e}");
                return;
            }
        }
        let line = match std::str::from_utf8(&raw) {
            Ok(line) => line.trim_end_matches(['\n', '\r']),
            Err(_) => {
                spoiled.get_or_insert_with(|| "a line that is not UTF-8 text".to_string());
                continue;
            }
        };
        text.push_str(line);
        text.push('\n');
        if line.trim() != "done" {
            continue;
        }
        let message = std::mem::take(&mut text);
        on(match spoiled.take() {
            Some(why) => Err(why),
            None => lineup::read(&message),
        });
    }
}

/// The bots `--host` runs, and the lineup they follow.
pub struct Host {
    opts: Arc<Options>,
    cache: Arc<MapCache>,
    siblings: Siblings,
    current: Lineup,
    bots: Vec<Hosted>,
    started: usize,
}

impl Host {
    pub fn new(opts: Arc<Options>, cache: Arc<MapCache>) -> Self {
        Host {
            opts,
            cache,
            siblings: Siblings::default(),
            current: Lineup {
                arena: String::new(),
                controller: None,
                bots: Vec::new(),
            },
            bots: Vec::new(),
            started: 0,
        }
    }

    /// Follows the lineup messages on `input` until it ends; then every bot
    /// leaves.
    pub fn run(&mut self, input: impl BufRead) {
        read_messages(input, |message| match message {
            Ok(new) => self.follow(new),
            // The last good lineup stays in force.
            Err(e) => log::warn!("lineup ignored: {e}"),
        });
        log::info!("standard input closed; {} bot(s) leaving", self.bots.len());
        for b in &self.bots {
            let _ = b.orders.send(Order::Leave);
        }
        for b in self.bots.drain(..) {
            let _ = b.thread.join();
        }
    }

    /// Changes the running bots to match `new`.
    fn follow(&mut self, new: Lineup) {
        *self.siblings.lock().unwrap_or_else(|e| e.into_inner()) =
            new.bots.iter().map(|e| e.name.clone()).collect();
        let place = Place::of(&new);
        for step in plan(&self.current, &new) {
            match step {
                Step::Start(entry) => self.start(*entry, &place),
                Step::Stop(name) => self.stop(&name),
                Step::Tell(name, order) => self.tell(&name, order),
            }
        }
        self.current = new;
    }

    fn start(&mut self, entry: Entry, place: &Place) {
        let (orders, inbox) = mpsc::channel();
        let name = entry.name.clone();
        let (opts, cache, siblings) =
            (self.opts.clone(), self.cache.clone(), self.siblings.clone());
        let (place, n) = (place.clone(), self.started);
        self.started += 1;
        log::info!("[{name}] starting");
        let spawned = std::thread::Builder::new()
            .name(name.clone())
            .spawn(move || run_hosted(entry, place, opts, cache, n, siblings, inbox));
        match spawned {
            Ok(thread) => self.bots.push(Hosted {
                name,
                orders,
                thread,
            }),
            Err(e) => log::warn!("[{name}] couldn't start: {e}"),
        }
    }

    /// Tells the bot to leave, and waits until it has.
    fn stop(&mut self, name: &str) {
        let Some(i) = self
            .bots
            .iter()
            .position(|b| b.name.eq_ignore_ascii_case(name))
        else {
            return;
        };
        let b = self.bots.remove(i);
        let _ = b.orders.send(Order::Leave);
        let _ = b.thread.join();
    }

    fn tell(&self, name: &str, order: Order) {
        if let Some(b) = self.bots.iter().find(|b| b.name.eq_ignore_ascii_case(name)) {
            let _ = b.orders.send(order);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(args: &str) -> Options {
        crate::parse_args(args.split_whitespace().map(String::from))
            .unwrap()
            .unwrap()
    }

    fn lineup(text: &str) -> Lineup {
        lineup::read(text).unwrap()
    }

    const ONE: &str = "lineup duel\nbot A\npassword one\nseat in\nteam 1\nship 2\nend\ndone\n";

    #[test]
    fn count_bots_are_numbered_and_seated() {
        let l = lineup_from_options(&options("--server x:1 --name Bot --count 3 --ship 2"), None);
        let names: Vec<&str> = l.bots.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["Bot1", "Bot2", "Bot3"]);
        assert!(
            l.bots
                .iter()
                .all(|e| e.seat == Seat::In && e.script.ship == Some(1))
        );
        let spec = lineup_from_options(&options("--server x:1 --count 2"), None);
        assert!(spec.bots.iter().all(|e| e.seat == Seat::Spectating));
        assert_eq!(spec.arena, "");
    }

    #[test]
    fn a_new_password_alone_is_passed_to_the_bot() {
        let old = lineup(ONE);
        let new = lineup(&ONE.replace("password one", "password two"));
        assert_eq!(
            plan(&old, &new),
            [Step::Tell("A".into(), Order::Password("two".into()))]
        );
        // A restarted bot already starts with its new pass.
        let restarted = lineup(
            &ONE.replace("password one", "password two")
                .replace("ship 2", "ship 3"),
        );
        assert_eq!(
            plan(&old, &restarted),
            [
                Step::Stop("A".into()),
                Step::Start(Box::new(restarted.bots[0].clone()))
            ]
        );
        assert!(plan(&old, &old).is_empty());
    }

    #[test]
    fn a_line_that_is_not_text_spoils_only_its_message() {
        let mut input = b"lineup duel\nbot A\npassword \xff\xfe\nend\ndone\n".to_vec();
        input.extend_from_slice(ONE.as_bytes());
        let mut got = Vec::new();
        read_messages(&input[..], |m| got.push(m));
        assert_eq!(got.len(), 2);
        assert!(got[0].as_ref().unwrap_err().contains("UTF-8"));
        assert_eq!(got[1].as_ref().unwrap().bots[0].name, "A");
    }
}
