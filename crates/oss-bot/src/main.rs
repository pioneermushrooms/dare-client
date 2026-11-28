//! `oss-bot`: a headless SubSpace client for tests, load swarms and arena bots.
//!
//! Connects (negotiating its encryption exactly as the graphical client does: the
//! automatic order, read from the mechanism registry by strength), logs in, enters an arena, takes the settings and map (cached),
//! answers security checks, and either sits in spectator mode or flies a simulated
//! ship. A flying bot keeps every other ship and every shot in the arena in its
//! simulation, as the game does, so it is hit, dies and comes back like a player.
//! Many bots can run in one process; a scripted bot (see `script`) flies one of
//! the behaviours in `brain` and talks (`talk`). With `--host` the process runs
//! an arena's bots as its standard input lists them (see `lineup`), each on its
//! own thread with its own connection.

mod brain;
mod host;
mod lineup;
mod nav;
mod report;
mod script;
mod talk;

use oss_session::security::{
    Identities, MechanismRegistry, SecurityMode, client_handshake, context, fingerprint,
};
use std::io::BufRead;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use oss_proto::ClientSettings;
use oss_session::map::{MapCache, default_cache_dir};
use oss_session::sim::{arena_world, remote_state, remote_weapon};
use oss_session::udp::UdpDriver;
use oss_session::{
    MAX_NAME, SPECTATOR, Session, SessionConfig, SessionEvent, State, chat_type, name_refusal,
};
use oss_sim::fixed::Rng;
use oss_sim::{Event, Pos, ShipInput, ShipState, VEL_SCALE, World, toggle};

use brain::{Brain, Contact, Leaving, SAFE_TILES, View};
use host::{Host, Place, lineup_from_options};
use lineup::{Entry, Seat};
use nav::Nav;
use report::{Reports, run_ticks};
use script::Script;
use talk::Talk;

/// The `--help` text for the mechanisms in `mechanisms`: the words `--security`
/// accepts, and the order `auto` tries, are read from them.
fn usage(mechanisms: &MechanismRegistry) -> String {
    let security =
        oss_session::security::usage(mechanisms, "Bots do not remember server keys.", 25, 80);
    USAGE.replace("{security}", &security)
}

/// The lines the bot logs at start about its security, read from its
/// mechanisms: what it supports, weakest first, and the order `auto` tries.
fn startup_lines(mechanisms: &MechanismRegistry) -> [String; 2] {
    oss_session::security::startup_lines(mechanisms)
}

/// The `--help` text, with `{security}` for the part read from the mechanisms.
const USAGE: &str = "\
Usage: oss-bot --server <host:port> [options]

Options:
  --server <host:port>   Zone to connect to (required)
  --name <name>          Login name (default oss-bot; numbered when --count > 1)
  --password <pw>        Password (default empty)
  --arena <name>         Arena to enter (default: any public arena)
  --ship <1-8|spec>      Ship to fly (default spec)
  --count <n>            Run n bots (default 1)
  --fly                  Fly around (needs a ship): thrust, turn and shoot at random
  --script <file|->      Run one scripted bot: name, ship, behaviour (idle, patrol,
                         sentry, fighter) and chat, read from the file, or from
                         standard input with -. With -, the bot leaves the zone
                         when its standard input closes. Its settings override
                         the options above; the README has the format
  --host                 Run an arena's bots as lineup messages on standard
                         input list them, each its own connection; the zone
                         sets their freqs. When standard input closes, every
                         bot leaves and the program ends
  --chat <text>          Say this in public chat once in the arena; :name:text
                         sends it privately to that player once they're here.
                         Repeat it to say several lines, in order
  --new-user             Create the name when the zone offers to (it does not
                         know it). Without this a bot declines and disconnects.
                         Subgame forgets its accounts when the zone restarts, so a
                         bot needs this against a zone that has just come up
  --duration <secs>      Disconnect after this long (default: run until Ctrl+C)
  --map-cache <dir>      Where downloaded maps are kept
  --security <mode>      {security}
  --identity <file>      Offer an OpenSec identity key, one per bot number and
                         zone, kept in <file> (made if missing); a renamed bot
                         keeps its key
  -v, --verbose          Log every bot's events (default: bot 1 only)
  -h, --help             Show this help";

struct Options {
    server: String,
    name: String,
    password: String,
    arena: Option<String>,
    ship: u8,
    count: usize,
    fly: bool,
    chat: Vec<String>,
    duration: Option<Duration>,
    map_cache: Option<PathBuf>,
    verbose: bool,
    security: SecurityMode,
    identity: Option<PathBuf>,
    new_user: bool,
    /// `-` for standard input.
    script: Option<String>,
    /// Run the bots lineup messages on standard input name.
    host: bool,
}

fn parse_args(
    mut args: impl Iterator<Item = String>,
    mechanisms: &MechanismRegistry,
) -> Result<Option<Options>, String> {
    let mut o = Options {
        server: String::new(),
        name: "oss-bot".into(),
        password: String::new(),
        arena: None,
        ship: SPECTATOR,
        count: 1,
        fly: false,
        chat: Vec::new(),
        duration: None,
        map_cache: None,
        verbose: false,
        security: SecurityMode::Auto,
        identity: None,
        new_user: false,
        script: None,
        host: false,
    };
    while let Some(a) = args.next() {
        let mut value = |n: &str| args.next().ok_or_else(|| format!("{n} needs a value"));
        match a.as_str() {
            "-h" | "--help" => return Ok(None),
            "--server" => o.server = value("--server")?,
            "--name" => o.name = value("--name")?,
            "--password" => o.password = value("--password")?,
            "--arena" => o.arena = Some(value("--arena")?),
            "--new-user" => o.new_user = true,
            "--ship" => {
                let v = value("--ship")?;
                o.ship = if v.eq_ignore_ascii_case("spec") {
                    SPECTATOR
                } else {
                    v.parse::<u8>()
                        .ok()
                        .filter(|s| (1..=8).contains(s))
                        .ok_or("--ship must be 1-8 or spec")?
                        - 1
                }
            }
            "--count" => {
                o.count = value("--count")?
                    .parse()
                    .ok()
                    .filter(|&n| (1..=1000).contains(&n))
                    .ok_or("--count must be 1-1000")?
            }
            "--fly" => o.fly = true,
            "--script" => o.script = Some(value("--script")?),
            "--host" => o.host = true,
            "--security" => {
                let word = value("--security")?;
                o.security = SecurityMode::parse(&word, mechanisms).map_err(|_| {
                    format!(
                        "--security must be {}, not `{word}`",
                        oss_session::security::words(mechanisms)
                    )
                })?
            }
            "--identity" => o.identity = Some(value("--identity")?.into()),
            "--chat" => o.chat.push(value("--chat")?),
            "--duration" => {
                let s: f64 = value("--duration")?
                    .parse()
                    .map_err(|_| "--duration must be seconds")?;
                o.duration = Some(Duration::from_secs_f64(s.max(0.0)));
            }
            "--map-cache" => o.map_cache = Some(PathBuf::from(value("--map-cache")?)),
            "-v" | "--verbose" => o.verbose = true,
            other => return Err(format!("unknown argument {other}")),
        }
    }
    if o.server.is_empty() {
        return Err("--server is required".into());
    }
    if o.script.is_some() && o.count != 1 {
        return Err("--script runs one bot; leave out --count".into());
    }
    if o.host && (o.script.is_some() || o.count != 1) {
        return Err(
            "--host runs the bots its standard input lists; leave out --script and --count".into(),
        );
    }
    // The client's name box stops at MAX_NAME, and a zone is never sent a
    // longer name. The numbered names are checked too: the number goes on the
    // end, so the last bot's is the longest.
    let longest = if o.count > 1 {
        format!("{}{}", o.name, o.count)
    } else {
        o.name.clone()
    };
    if longest.chars().count() > MAX_NAME {
        return Err(format!(
            "the name {longest:?} is longer than {MAX_NAME} characters"
        ));
    }
    if let Some(why) = name_refusal(&o.name) {
        return Err(format!("--name: {why}"));
    }
    Ok(Some(o))
}

/// Reads a script from `source` (`-` for standard input, up to its `end` line).
fn read_script(source: &str) -> Result<Script, String> {
    let text = if source == "-" {
        let mut text = String::new();
        for line in std::io::stdin().lock().lines() {
            let line = line.map_err(|e| format!("standard input: {e}"))?;
            if line.trim() == "end" {
                break;
            }
            text.push_str(&line);
            text.push('\n');
        }
        text
    } else {
        std::fs::read_to_string(source).map_err(|e| format!("{source}: {e}"))?
    };
    script::parse(&text)
}

/// Sets `stop` once standard input reaches its end: the program that started the
/// bot closed it, or went away.
fn watch_stdin(stop: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        let mut sink = String::new();
        let mut input = std::io::stdin().lock();
        while matches!(input.read_line(&mut sink), Ok(n) if n > 0) {
            sink.clear();
        }
        stop.store(true, Ordering::Relaxed);
    });
}

/// The square ships of `freq` spawn in: an arena spawn spot, or the edge of the
/// team's half or quarter on a team radar, as the game picks it.
fn spawn_area(cs: &ClientSettings, freq: u16) -> Option<((i32, i32), i32)> {
    let set: Vec<_> = cs
        .spawns
        .iter()
        .filter(|s| s.x() != 0 || s.y() != 0 || s.radius() != 0)
        .collect();
    if !set.is_empty() {
        let sp = set[freq as usize % set.len()];
        let middle = |v: u16| if v == 0 { 512 } else { v as i32 };
        return Some(((middle(sp.x()), middle(sp.y())), sp.radius() as i32));
    }
    let side = |bit: u16| (bit & 1) as i32 * 0x300 + 0x80;
    match cs.radar_mode() {
        1 | 3 => Some(((side(freq), 0x180), 0x80)),
        2 | 4 => Some(((side(freq), side(freq / 2)), 0x80)),
        _ => None,
    }
}

/// Pixels, from the simulation's position units.
fn px(p: Pos) -> (f32, f32) {
    p.to_f32()
}

/// Pixels per second, from the simulation's velocity units.
fn per_second(v: oss_sim::Vel) -> (f32, f32) {
    let k = VEL_SCALE as f32 * 10.0;
    (v.x as f32 / k, v.y as f32 / k)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OrderRequest {
    Come,
    Follow,
    Hold,
    Resume,
}

fn parse_order(text: &str) -> Option<OrderRequest> {
    let clean = text
        .trim()
        .trim_end_matches(['.', '!', '?'])
        .to_ascii_lowercase();
    match clean.as_str() {
        "come here" | "come to me" | "cme here" => Some(OrderRequest::Come),
        "follow" | "follow me" => Some(OrderRequest::Follow),
        "hold" | "hold position" | "stay here" | "stay here until i say go" => {
            Some(OrderRequest::Hold)
        }
        "go" | "resume" | "resume patrol" => Some(OrderRequest::Resume),
        _ => None,
    }
}

#[derive(Clone, Copy)]
enum FlightOrder {
    Position((f32, f32)),
    Follow(u16),
}

/// The names of the bots a host runs, shared by them all: they don't greet or
/// answer one another (they do fight, by freq).
type Siblings = Arc<Mutex<Vec<String>>>;

/// One bot: a session, its socket, and (when flying) a simulation of the arena.
struct Bot {
    label: String,
    session: Session,
    driver: UdpDriver,
    loud: bool,
    map: Option<Vec<u8>>,
    green_seed: u32,
    doors: Option<(u32, u32)>,
    /// The ship and freq it wants, and whether it already asked for the freq.
    /// A hosted bot asks for no freq: the zone sets it.
    ship: u8,
    /// The ship it flies when seated (`ship` is spectator while it isn't).
    seat_ship: u8,
    freq: Option<i16>,
    freq_asked: bool,
    /// Fly at random (`--fly`) when there is no brain.
    fly: bool,
    world: Option<World>,
    nav: Option<Nav>,
    me: u16,
    brain: Option<Brain>,
    talk: Option<Talk>,
    siblings: Siblings,
    controller: Option<String>,
    flight_order: Option<FlightOrder>,
    /// Told to spectate but still breaking off its fight.
    leaving: Option<Leaving>,
    last_order_reply: Option<u64>,
    advertised_orders: bool,
    rng: Rng,
    /// How many `--chat` lines have been said.
    chatted: usize,
    last_step_ms: u64,
    /// Its position reports (see `report`).
    reports: Reports,
}

impl Bot {
    fn log(&self, msg: impl std::fmt::Display) {
        if self.loud {
            log::info!("[{}] {msg}", self.label);
        }
    }

    fn name_of(&self, pid: i16) -> String {
        self.session
            .player(pid)
            .map(|p| p.name.clone())
            .unwrap_or_default()
    }

    /// Whether `name` is another bot of the same host.
    fn is_sibling(&self, name: &str) -> bool {
        self.siblings
            .lock()
            .is_ok_and(|s| s.iter().any(|n| n.eq_ignore_ascii_case(name)))
    }

    /// Takes the seat the host gives: the bot's ship, or spectator mode. A bot
    /// that is flying breaks off its fight first (see `fly`); going back in
    /// cancels that.
    fn take_seat(&mut self, seat: Seat) {
        match seat {
            Seat::In => {
                self.leaving = None;
                self.set_ship(self.seat_ship);
            }
            Seat::Spectating => {
                let flying = self.brain.is_some() && self.nav.is_some() && self.own_ship_flying();
                if flying && self.ship != SPECTATOR {
                    if self.leaving.is_none() {
                        self.log("breaking off before spectating");
                        self.leaving = Some(Leaving {
                            since_ms: self.driver.now_ms(),
                        });
                    }
                } else {
                    self.set_ship(SPECTATOR);
                }
            }
        }
    }

    fn set_ship(&mut self, want: u8) {
        if want == self.ship {
            return;
        }
        self.ship = want;
        self.flight_order = None;
        self.log(if want == SPECTATOR {
            "going to spectator"
        } else {
            "taking my seat"
        });
        if self.session.state() == State::Playing
            && self.session.me().is_some_and(|p| p.ship != want)
        {
            self.session.set_ship(want);
        }
    }

    /// Leaves the zone.
    fn leave(&mut self) {
        self.session.disconnect();
        let _ = self.driver.send_pending(&mut self.session);
    }

    /// One pass of the bot's work: take in what the zone sent, step the arena,
    /// fly and talk, and send. `false` once its session has closed.
    fn step(&mut self, cache: &MapCache, chat: &[String]) -> bool {
        if self.session.state() == State::Closed {
            return false;
        }
        if let Err(e) = self.driver.pump(&mut self.session) {
            log::warn!("[{}] socket error: {e}", self.label);
            self.session.disconnect();
        }
        self.handle_events(cache);
        // Acks and answers to what just came in go out now, before the brain runs,
        // so the zone's view of the bot's ping never includes its thinking.
        let _ = self.driver.send_pending(&mut self.session);
        let now = self.driver.now_ms();
        if self.world.is_none() {
            self.last_step_ms = now;
        }
        if self.session.state() == State::Playing {
            if let Some(text) = chat.get(self.chatted) {
                match private_chat(text) {
                    // :name:text waits until that player is in the arena.
                    Some((to, msg)) => {
                        let pid = self
                            .session
                            .players()
                            .find(|p| p.name.eq_ignore_ascii_case(to))
                            .map(|p| p.pid);
                        if let Some(pid) = pid {
                            self.session.send_chat(chat_type::PRIVATE, pid, msg);
                            self.chatted += 1;
                        }
                    }
                    None => {
                        self.session.send_chat(chat_type::PUBLIC, 0, text);
                        self.chatted += 1;
                    }
                }
            }
            if let Some((kind, to, text)) = self.talk.as_mut().and_then(|t| t.next(now)) {
                self.session.send_chat(kind, to, &text);
            }
            // Other ships and shots keep moving while spectating, too.
            let flying = self.own_ship_flying();
            self.fly(now);
            if !flying && self.session.position_due(now) {
                // Spectators still report in so the server knows they're alive.
                self.session.send_position(now, &Default::default());
            }
        }
        let _ = self.driver.send_pending(&mut self.session);
        true
    }

    fn handle_order(&mut self, pid: i16, text: &str, now: u64) -> bool {
        let Some(controller) = self.controller.as_deref() else {
            return false;
        };
        if pid < 0 || pid as u16 == self.me {
            return false;
        }
        if text.trim().eq_ignore_ascii_case("help") {
            self.reply_to_order(
                pid,
                "Arena owner orders: come here, follow me, stay here, resume.",
                now,
            );
            return true;
        }
        if !self
            .session
            .player(pid)
            .is_some_and(|p| p.name.eq_ignore_ascii_case(controller))
        {
            return false;
        }
        let Some(request) = parse_order(text) else {
            return false;
        };
        let position = |id: u16| {
            let world = self.world.as_ref()?;
            let ship = &world.ships[world.ship_index(id)?];
            ship.is_alive().then(|| px(ship.pos))
        };
        let reply = match request {
            OrderRequest::Come => match position(pid as u16) {
                Some(at) => {
                    self.flight_order = Some(FlightOrder::Position(at));
                    "On my way. I'll hold there."
                }
                None => "I need to see your ship first.",
            },
            OrderRequest::Follow if position(pid as u16).is_some() => {
                self.flight_order = Some(FlightOrder::Follow(pid as u16));
                "Following you."
            }
            OrderRequest::Follow => "I need to see your ship first.",
            OrderRequest::Hold => match position(self.me) {
                Some(at) => {
                    self.flight_order = Some(FlightOrder::Position(at));
                    "Holding here until you say go."
                }
                None => "I need to be in a ship to hold here.",
            },
            OrderRequest::Resume => {
                self.flight_order = None;
                "Resuming my usual behavior."
            }
        };
        self.reply_to_order(pid, reply, now);
        true
    }

    fn reply_to_order(&mut self, pid: i16, reply: &str, now: u64) {
        if self
            .last_order_reply
            .is_none_or(|last| now.saturating_sub(last) >= talk::GAP_MS)
        {
            self.session.send_chat(chat_type::PRIVATE, pid, reply);
            self.last_order_reply = Some(now);
        }
    }

    fn handle_events(&mut self, cache: &MapCache) {
        let now = self.driver.now_ms();
        let now_ticks = self.session.server_ticks(now);
        let mut sim_events = Vec::new();
        while let Some(ev) = self.session.poll_event() {
            match ev {
                SessionEvent::Connected => {
                    let cipher = self.session.cipher_name().unwrap_or("?");
                    self.log(format!("connected ({cipher})"));
                }
                SessionEvent::LoginFailed {
                    message, zone_text, ..
                } => {
                    let why = zone_text.as_deref().unwrap_or(message);
                    log::warn!("[{}] login failed: {why}", self.label)
                }
                // Only reached without --new-user: with it the session says yes
                // by itself. A bot has nobody to ask, so it declines.
                SessionEvent::NewUserRequired { name, .. } => {
                    log::warn!(
                        "[{}] the zone does not know {name:?}; pass --new-user to create it",
                        self.label
                    );
                    self.session.disconnect();
                }
                // A bot fills in no forms: sending none is legal, and the zone
                // lets it in either way.
                SessionEvent::RegistrationWanted => {
                    self.log("the zone asks for a registration form; skipping it");
                    self.session.skip_registration();
                }
                SessionEvent::LoggedIn => self.log("logged in"),
                SessionEvent::EnteredArena { pid } => {
                    self.me = pid as u16;
                    self.world = None;
                    self.flight_order = None;
                    self.advertised_orders = false;
                    self.log(format_args!("entering arena as pid {pid}"))
                }
                SessionEvent::MapNeeded { name, checksum } => {
                    if cache
                        .load(&name)
                        .is_some_and(|d| self.session.use_cached_map(&d))
                    {
                        self.log(format_args!("map {name} from cache"));
                    } else {
                        self.log(format_args!("downloading map {name} (crc {checksum:08x})"));
                        self.session.download_map();
                    }
                }
                SessionEvent::MapReady {
                    name,
                    data,
                    downloaded,
                } => {
                    if downloaded && let Err(e) = cache.store(&name, &data) {
                        log::warn!("[{}] couldn't cache map {name}: {e}", self.label);
                    }
                    self.log(format_args!("map {name} ready ({} bytes)", data.len()));
                    self.map = Some(data);
                    self.nav = None;
                }
                SessionEvent::Seeds {
                    green_seed,
                    door_seed,
                    timestamp,
                } => {
                    self.green_seed = green_seed;
                    self.doors = Some((door_seed, timestamp));
                    if let Some(w) = self.world.as_mut() {
                        w.sync_doors(door_seed, timestamp);
                    }
                }
                SessionEvent::MapRejected { name, reason } => log::error!(
                    "[{}] map {name} {reason}; left the zone rather than answer its \
                     checks with it",
                    self.label
                ),
                SessionEvent::Refused { reason } => {
                    log::error!("[{}] {reason}; left the zone", self.label)
                }
                SessionEvent::Playing => {
                    let n = self.session.players().count();
                    self.log(format_args!("in the arena with {n} player(s)"));
                    self.enter_world(now);
                    let present: Vec<String> = self
                        .session
                        .players()
                        .filter(|p| p.pid as u16 != self.me)
                        .map(|p| p.name.clone())
                        .collect();
                    for name in present {
                        self.greet(&name, now);
                    }
                    // Servers start you as a spectator, or in the ship you logged
                    // in with; ask for the one wanted now (spectator included: a
                    // host may have unseated the bot while it was logging in).
                    let current = self.session.me().map(|p| p.ship).unwrap_or(SPECTATOR);
                    if current != self.ship {
                        self.session.set_ship(self.ship);
                    }
                    self.ask_freq();
                }
                SessionEvent::PlayerEntered(pid) => {
                    let Some(p) = self.session.player(pid).cloned() else {
                        continue;
                    };
                    self.log(format_args!(
                        "{} entered (ship {}, freq {})",
                        p.name,
                        p.ship + 1,
                        p.freq
                    ));
                    if pid as u16 != self.me {
                        if let Some(w) = self.world.as_mut()
                            && p.ship != SPECTATOR
                        {
                            w.add_remote_ship(pid as u16, p.ship, p.freq as u16);
                        }
                        self.greet(&p.name, now);
                    }
                }
                SessionEvent::PlayerLeft(pid) => {
                    if matches!(self.flight_order, Some(FlightOrder::Follow(target)) if target == pid as u16)
                    {
                        self.flight_order = None;
                    }
                    self.log(format_args!("pid {pid} left"));
                    if let Some(w) = self.world.as_mut()
                        && pid as u16 != self.me
                    {
                        w.remove_ship(pid as u16);
                    }
                }
                SessionEvent::ShipChanged { pid, ship, freq } => {
                    if pid as u16 == self.me {
                        self.log(format_args!("now in ship {} on freq {freq}", ship + 1));
                        self.place_own_ship(ship, freq as u16);
                        self.ask_freq();
                    } else if let Some(w) = self.world.as_mut() {
                        w.remove_ship(pid as u16);
                        if ship != SPECTATOR {
                            w.add_remote_ship(pid as u16, ship, freq as u16);
                        }
                    }
                }
                SessionEvent::FreqChanged { pid, freq } => {
                    if pid as u16 == self.me {
                        self.log(format_args!("now on freq {freq}"));
                        let ship = self.session.me().map(|p| p.ship).unwrap_or(SPECTATOR);
                        self.place_own_ship(ship, freq as u16);
                    } else if let Some(w) = self.world.as_mut()
                        && let Some(i) = w.ship_index(pid as u16)
                    {
                        w.ships[i].freq = freq as u16;
                    }
                }
                SessionEvent::PlayerMoved(pid) if pid as u16 != self.me => {
                    let Some(pos) = self.session.player(pid).and_then(|p| p.position) else {
                        continue;
                    };
                    let Some(w) = self.world.as_mut() else {
                        continue;
                    };
                    let Some(i) = w.ship_index(pid as u16) else {
                        continue;
                    };
                    if pos.x < 0 || pos.y < 0 {
                        // (-1, -1): dead or not yet spawned.
                        if w.ships[i].is_alive() {
                            w.ships[i].state = ShipState::Dead {
                                respawn_tick: w.tick,
                            };
                        }
                        continue;
                    }
                    let age = now_ticks.wrapping_sub(pos.server_ticks);
                    let age = if (age as i32) < 0 { 0 } else { age };
                    w.set_remote_state(i, &remote_state(&pos), age);
                }
                SessionEvent::PlayerFired { pid, at } if pid as u16 != self.me => {
                    let Some(w) = self.world.as_mut() else {
                        continue;
                    };
                    let Some(i) = w.ship_index(pid as u16).filter(|&i| w.ships[i].is_alive())
                    else {
                        continue;
                    };
                    if at.x < 0 || at.y < 0 {
                        continue;
                    }
                    let age = now_ticks.wrapping_sub(at.server_ticks);
                    let age = if (age as i32) < 0 { 0 } else { age };
                    if let Some(weapon) = at.weapon.and_then(remote_weapon) {
                        w.fire_remote(i, &remote_state(&at), weapon, age, &mut sim_events);
                    }
                }
                SessionEvent::Chat {
                    kind,
                    pid,
                    name,
                    text,
                    sound,
                    ..
                } => {
                    let who = name.clone().unwrap_or_else(|| self.name_of(pid));
                    match sound {
                        0 => self.log(format_args!("chat({kind}) {who}> {text}")),
                        _ => self.log(format_args!("chat({kind}, sound {sound}) {who}> {text}")),
                    }
                    if kind == chat_type::PRIVATE && self.handle_order(pid, &text, now) {
                        continue;
                    }
                    let asked = kind == chat_type::PUBLIC || kind == chat_type::PRIVATE;
                    if asked
                        && pid >= 0
                        && pid as u16 != self.me
                        && !self.is_sibling(&who)
                        && let Some(t) = self.talk.as_mut()
                    {
                        t.heard(pid, kind == chat_type::PRIVATE, &text, now);
                    }
                }
                SessionEvent::Kill { killer, killed, .. } => {
                    let (kn, dn) = (self.name_of(killer), self.name_of(killed));
                    self.log(format_args!("{dn} killed by {kn}"));
                    // Your own death is your own simulation's to decide.
                    if killed as u16 != self.me
                        && let Some(w) = self.world.as_mut()
                    {
                        w.kill_ship(killed as u16, killer as u16, &mut sim_events);
                    }
                }
                SessionEvent::ShipReset => {
                    if let Some(w) = self.world.as_mut()
                        && let Some(i) = w.ship_index(self.me)
                        && !w.ships[i].remote
                        && w.ships[i].is_alive()
                    {
                        w.respawn(i);
                    }
                }
                SessionEvent::WarpTo { x, y } => {
                    if let Some(w) = self.world.as_mut()
                        && let Some(i) = w.ship_index(self.me)
                    {
                        let ship = &mut w.ships[i];
                        ship.pos = Pos::from_pixels(x as i32 * 16 + 8, y as i32 * 16 + 8);
                        ship.prev_pos = ship.pos;
                        ship.vel = Default::default();
                    }
                }
                SessionEvent::PrizeReceived { count, prize } => {
                    if let Some(w) = self.world.as_mut()
                        && let Some(i) = w.ship_index(self.me)
                    {
                        for _ in 0..count.clamp(0, 100) {
                            w.apply_prize(i, prize as i32, &mut sim_events);
                        }
                    }
                }
                SessionEvent::Closed(reason) => {
                    log::warn!("[{}] disconnected: {reason:?}", self.label)
                }
                // The session has already hung up. A bot is pointed at one zone
                // on purpose, so it says where it was sent rather than going.
                SessionEvent::Redirected { server, arena, .. } => log::warn!(
                    "[{}] the zone redirects to {server}{}; run again with --server {server}",
                    self.label,
                    arena.map(|a| format!(", arena {a:?}")).unwrap_or_default()
                ),
                _ => {}
            }
        }
    }

    fn greet(&mut self, name: &str, now: u64) {
        if name.is_empty() || self.is_sibling(name) {
            return;
        }
        if self.controller.is_some() && !self.advertised_orders {
            self.session.send_chat(
                chat_type::PUBLIC,
                0,
                "I take flight orders from the arena owner. PM me 'help' for the list.",
            );
            self.advertised_orders = true;
        }
        if let Some(t) = self.talk.as_mut() {
            t.arrived(name, now);
        }
    }

    /// Asks for the bot's own freq once it is in a ship.
    fn ask_freq(&mut self) {
        let Some(want) = self.freq else { return };
        let Some(me) = self.session.me() else { return };
        if self.freq_asked || me.ship == SPECTATOR || me.freq == want {
            return;
        }
        self.freq_asked = true;
        self.session.set_freq(want);
    }

    /// Builds the arena's simulation: its map, settings and the ships in it.
    fn enter_world(&mut self, now: u64) {
        let (Some(map), Some(settings)) = (self.map.as_ref(), self.session.settings()) else {
            return;
        };
        let (mut world, _) = arena_world(map, settings, self.green_seed);
        world.set_tick(self.session.server_ticks(now));
        world.player_count = Some(self.session.players().count() as u32);
        if let Some((seed, at)) = self.doors {
            world.sync_doors(seed, at);
        }
        for p in self.session.players() {
            if p.pid as u16 != self.me && p.ship != SPECTATOR {
                world.add_remote_ship(p.pid as u16, p.ship, p.freq as u16);
            }
        }
        self.world = Some(world);
        self.last_step_ms = now;
        let (ship, freq) = self
            .session
            .me()
            .map(|p| (p.ship, p.freq as u16))
            .unwrap_or((SPECTATOR, 0));
        self.place_own_ship(ship, freq);
    }

    /// Puts the bot's own ship in the simulation (or takes it out, for spectator
    /// mode), at a spawn point of its freq.
    fn place_own_ship(&mut self, ship: u8, freq: u16) {
        let area = self.session.settings().and_then(|cs| spawn_area(cs, freq));
        let Some(w) = self.world.as_mut() else { return };
        w.remove_ship(self.me);
        if ship == SPECTATOR {
            return;
        }
        (w.spawn_center, w.spawn_radius) = area.unwrap_or((
            (512, 512),
            w.settings.warp_radius_limit.clamp(3, 512) as i32,
        ));
        w.add_ship(self.me, ship, freq);
        if self.brain.is_some() && self.nav.is_none() {
            let radius = w.settings.ships[ship as usize % 8].radius_pixels();
            self.nav = Some(Nav::new(&w.map, radius));
        }
        if let Some(b) = self.brain.as_mut() {
            b.respawned();
        }
    }

    /// Advances the simulation to `now`, flying the bot's ship, and reports what
    /// it did (shots, death, prizes) to the zone.
    fn fly(&mut self, now: u64) {
        let Some(world) = self.world.as_mut() else {
            return;
        };
        let (mut enemies, mut shots) = (Vec::new(), Vec::new());
        let mut spectate = false;
        run_ticks(&mut self.last_step_ms, now, |tick_ms| {
            let me = world
                .ship_index(self.me)
                .filter(|&i| !world.ships[i].remote);
            let input = match me {
                Some(i) if world.ships[i].is_alive() => {
                    if let (Some(brain), Some(nav)) = (self.brain.as_mut(), self.nav.as_mut()) {
                        let v = view(world, i, &mut enemies, &mut shots);
                        if let Some(l) = self.leaving {
                            let near = v.enemies.iter().any(|e| {
                                let (dx, dy) = (e.pos.0 - v.pos.0, e.pos.1 - v.pos.1);
                                dx.hypot(dy) < SAFE_TILES * 16.0
                            });
                            if l.ready(tick_ms, near) {
                                spectate = true;
                            }
                            brain.disengage(&v, nav)
                        } else {
                            match self.flight_order {
                                Some(FlightOrder::Position(at)) => brain.obey(&v, nav, at, false),
                                Some(FlightOrder::Follow(id)) => {
                                    let at = world
                                        .ship_index(id)
                                        .map(|j| &world.ships[j])
                                        .filter(|target| target.is_alive())
                                        .map_or(v.pos, |target| px(target.pos));
                                    brain.obey(&v, nav, at, true)
                                }
                                None => brain.think(&v, nav),
                            }
                        }
                    } else if self.fly {
                        random_walk(world.tick, self.rng.next_u32())
                    } else {
                        ShipInput::default()
                    }
                }
                _ => {
                    // Dead (or gone) and told to spectate: nothing to break off.
                    spectate |= self.leaving.is_some();
                    ShipInput::default()
                }
            };
            let mut inputs = vec![ShipInput::default(); world.ships.len()];
            if let Some(i) = me {
                inputs[i] = input;
            }
            let events = world.step(&inputs);
            let own = me.and_then(|_| world.ship_index(self.me));
            // Shots and motion changes go out at once, dated to this tick.
            self.reports.tick(
                &mut self.session,
                now,
                world.tick,
                tick_ms,
                own.map(|i| &world.ships[i]),
                &input,
                &events,
            );
            let Some(i) = own else {
                return;
            };
            let ship = &world.ships[i];
            for ev in events {
                match ev {
                    Event::ShipKilled {
                        ship: id, killer, ..
                    } if id == self.me => {
                        let bounty = ship.bounty.min(u16::MAX as u32) as u16;
                        self.session.send_death(killer as i16, bounty);
                        if self.loud {
                            log::info!("[{}] shot down", self.label);
                        }
                    }
                    Event::Respawned { ship: id } if id == self.me => {
                        if let Some(b) = self.brain.as_mut() {
                            b.respawned();
                        }
                    }
                    Event::GreenTaken {
                        ship: id,
                        x,
                        y,
                        prize,
                    } if id == self.me => {
                        self.session.send_take_green(now, x, y, prize);
                    }
                    _ => {}
                }
            }
        });
        let own = world
            .ship_index(self.me)
            .filter(|&i| !world.ships[i].remote)
            .map(|i| &world.ships[i]);
        self.reports
            .regular(&mut self.session, now, self.last_step_ms, own);
        if spectate {
            self.leaving = None;
            self.set_ship(SPECTATOR);
        }
    }

    fn own_ship_flying(&self) -> bool {
        self.world
            .as_ref()
            .and_then(|w| w.ship_index(self.me).map(|i| !w.ships[i].remote))
            .unwrap_or(false)
    }
}

/// `--fly`: mostly thrust, turn in bursts, shoot now and then.
fn random_walk(t: u32, r: u32) -> ShipInput {
    ShipInput {
        forward: t % 400 < 300,
        left: (t / 150).is_multiple_of(3),
        right: (t / 150) % 3 == 1,
        gun: r.is_multiple_of(40),
        bomb: r % 600 == 1,
        afterburner: t % 1000 < 100,
        ..Default::default()
    }
}

/// What the brain sees this tick: the bot's ship `me`, the ships it may fight
/// (alive, on another freq, out of safe zones: bot or pilot alike, never a
/// teammate) and the shots coming from other freqs.
fn view<'a>(
    world: &World,
    me: usize,
    enemies: &'a mut Vec<Contact>,
    shots: &'a mut Vec<Contact>,
) -> View<'a> {
    let ship = &world.ships[me];
    let s = &world.settings.ships[ship.kind as usize % 8];
    enemies.clear();
    enemies.extend(
        world
            .ships
            .iter()
            .filter(|o| {
                o.id != ship.id
                    && o.is_alive()
                    && o.freq != ship.freq
                    && o.toggles & toggle::SAFETY == 0
            })
            .map(|o| Contact {
                pos: px(o.pos),
                vel: per_second(o.vel),
            }),
    );
    shots.clear();
    shots.extend(
        world
            .projectiles
            .iter()
            .filter(|p| p.freq != ship.freq)
            .map(|p| Contact {
                pos: px(p.pos),
                vel: per_second(p.vel),
            }),
    );
    let bullet_speed = s.bullet_speed as f32 / 10.0;
    View {
        tick: world.tick,
        pos: px(ship.pos),
        vel: per_second(ship.vel),
        rotation: ship.rotation,
        turn_rate: ship.stats.rotation,
        top_speed: ship.stats.speed as f32 / 10.0,
        bullet_speed,
        bullet_range: bullet_speed * world.settings.bullet_alive_time as f32 / 100.0,
        energy: ship.energy as f32 / (ship.stats.max_energy.max(1) as f32 * 1000.0),
        enemies: &enemies[..],
        shots: &shots[..],
    }
}

/// Bot number `n`'s login. It presents itself as the current game client does:
/// a SubSpace 1.34 login (0x09) answering security checks with the keyed exe
/// checksum ([`oss_session::client::vie_keyed`], the browser client's profile),
/// so a zone that verifies that field never reports a bot as a mismatch. A bot
/// is a player to the zone, checked like one.
fn session_config(
    entry: &Entry,
    place: &Place,
    opts: &Options,
    n: usize,
    ship: u8,
) -> SessionConfig {
    SessionConfig {
        name: entry.name.clone(),
        password: entry.password.clone(),
        arena: Some(place.arena.clone()).filter(|a| !a.is_empty()),
        ship,
        machine_id: 0x0B07_0000 + n as u32,
        new_user: opts.new_user,
        client: oss_session::client::vie_keyed(),
        ..Default::default()
    }
}

/// Connects bot number `n` of a lineup: its own socket and session, and, when
/// scripted (`--script` or `--host`), its brain and chat.
fn start_bot(
    entry: &Entry,
    place: &Place,
    opts: &Options,
    n: usize,
    siblings: Siblings,
) -> Result<Bot, String> {
    let driver = UdpDriver::connect(&opts.server).map_err(|e| format!("{}: {e}", opts.server))?;
    let s = &entry.script;
    let seat_ship = s.ship.unwrap_or(0);
    let ship = match entry.seat {
        Seat::In => seat_ship,
        Seat::Spectating => SPECTATOR,
    };
    let cfg = session_config(entry, place, opts, n, ship);
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(1)
        .wrapping_add(n as u32 * 7919);
    let loud = n == 0 || opts.verbose || opts.host;
    // One identity per bot and zone, kept across runs: by number (so a renamed
    // bot keeps its key), or by name for a hosted bot.
    let identity = match opts.identity.as_deref() {
        Some(path) => {
            let who = if opts.host {
                entry.name.to_ascii_lowercase()
            } else {
                format!("bot{}", n + 1)
            };
            let id = Identities::load(path)
                .for_server(&format!("{who}@{}", opts.server))
                .map_err(|e| format!("{}: {e}", path.display()))?;
            if loud {
                println!("[{}] identity {}", entry.name, fingerprint(&id.public()));
            }
            Some(id)
        }
        None => None,
    };
    // Bots remember no server keys, so nothing is pinned.
    let negotiation = client_handshake(
        &MechanismRegistry::builtin(),
        &opts.security,
        &context(seed, None, identity),
    )
    .map_err(|e| format!("--security {}: {e}", opts.security))?;
    let now = driver.now_ms();
    let session = Session::with_negotiation(cfg, now, negotiation);
    let scripted = opts.host || opts.script.is_some();
    let brain = scripted.then(|| Brain::new(s.behaviour, s.points.clone(), s.range.unwrap_or(30)));
    let talk = scripted
        .then(|| Talk::new(s, now))
        .filter(|t| !t.is_quiet());
    Ok(Bot {
        label: entry.name.clone(),
        session,
        driver,
        loud,
        map: None,
        green_seed: 0,
        doors: None,
        ship,
        seat_ship,
        // A hosted bot's freq is the zone's to set.
        freq: if opts.host { None } else { s.freq },
        freq_asked: false,
        fly: opts.fly,
        world: None,
        nav: None,
        me: u16::MAX,
        brain,
        talk,
        siblings,
        controller: place.controller.clone().or(s.controller.clone()),
        flight_order: None,
        leaving: None,
        last_order_reply: None,
        advertised_orders: false,
        rng: Rng::new(seed | 1),
        chatted: 0,
        last_step_ms: 0,
        reports: Reports::default(),
    })
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let mechanisms = MechanismRegistry::builtin();
    let opts = match parse_args(std::env::args().skip(1), &mechanisms) {
        Ok(Some(o)) => o,
        Ok(None) => {
            println!(
                "oss-bot {}\n\n{}",
                env!("CARGO_PKG_VERSION"),
                usage(&mechanisms)
            );
            return;
        }
        Err(e) => {
            eprintln!("error: {e}\n\n{}", usage(&mechanisms));
            std::process::exit(2);
        }
    };
    for line in startup_lines(&mechanisms) {
        log::info!("{line}");
    }
    let cache = MapCache::new(
        opts.map_cache
            .clone()
            .unwrap_or_else(|| default_cache_dir(&opts.server)),
    );
    if opts.host {
        log::info!("hosting bots on {}", opts.server);
        Host::new(Arc::new(opts), Arc::new(cache)).run(std::io::stdin().lock());
        return;
    }
    let script = match opts.script.as_deref().map(read_script) {
        Some(Ok(s)) => Some(s),
        Some(Err(e)) => {
            eprintln!("error: script: {e}");
            std::process::exit(2);
        }
        None => None,
    };
    let stop = Arc::new(AtomicBool::new(false));
    if opts.script.as_deref() == Some("-") {
        watch_stdin(stop.clone());
    }

    let lineup = lineup_from_options(&opts, script.as_ref());
    let place = Place::of(&lineup);
    let mut bots = Vec::new();
    for (i, entry) in lineup.bots.iter().enumerate() {
        match start_bot(entry, &place, &opts, i, Siblings::default()) {
            Ok(b) => bots.push(b),
            Err(e) => {
                eprintln!("error: {e}");
                std::process::exit(1);
            }
        }
    }
    log::info!("{} bot(s) connecting to {}", bots.len(), opts.server);

    let start = Instant::now();
    let mut lost = false;
    loop {
        let mut open = 0;
        for bot in &mut bots {
            if bot.step(&cache, &opts.chat) {
                open += 1;
            }
        }
        if open == 0 {
            log::info!("all bots disconnected");
            lost = true;
            break;
        }
        let quit = stop.load(Ordering::Relaxed);
        if quit || opts.duration.is_some_and(|d| start.elapsed() >= d) {
            for bot in &mut bots {
                bot.leave();
            }
            log::info!(
                "{}; disconnected",
                if quit { "told to stop" } else { "time is up" }
            );
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    // A scripted bot that lost its zone says so, for whatever restarts it.
    if lost && script.is_some() {
        std::process::exit(3);
    }
}

/// `:name:text` → (name, text), Continuum's private-message syntax.
fn private_chat(text: &str) -> Option<(&str, &str)> {
    let (to, msg) = text.strip_prefix(':')?.split_once(':')?;
    (!to.is_empty()).then_some((to, msg))
}

#[cfg(test)]
mod tests {
    use super::*;

    use oss_session::security::{Era, MechanismInUse};
    use oss_session::test_zone::dummy_mechanism;

    fn parse(s: &str) -> Result<Option<Options>, String> {
        parse_args(
            s.split_whitespace().map(String::from),
            &MechanismRegistry::builtin(),
        )
    }

    /// A bot's name is held to the client's 23 characters, numbered names
    /// included: the number goes on the end, so ten bots of a 22-character
    /// name would end with a 24-character one.
    #[test]
    fn a_name_longer_than_23_characters_is_refused() {
        let o = parse("--server x:1 --name Abcdefghijklmnopqrs1234")
            .unwrap()
            .unwrap();
        assert_eq!(o.name, "Abcdefghijklmnopqrs1234");
        assert!(parse("--server x:1 --name Abcdefghijklmnopqrs12345").is_err());
        assert!(
            parse("--server x:1 --name Abcdefghijklmnopqrs123 --count 9")
                .unwrap()
                .is_some()
        );
        assert_eq!(
            parse("--server x:1 --name a:b").err().as_deref(),
            Some("--name: Player name cannot contain a colon.")
        );
        assert_eq!(
            parse("--server x:1 --name Abcdefghijklmnopqrs123 --count 10")
                .err()
                .as_deref(),
            Some("the name \"Abcdefghijklmnopqrs12310\" is longer than 23 characters")
        );
    }

    /// The bot negotiates exactly as the client does: `auto` unless told
    /// otherwise, the order read from its mechanisms by strength, so a
    /// mechanism of middle strength lands between OpenSecurity and Subspace.
    /// Naming one mechanism still works.
    #[test]
    fn the_default_is_the_automatic_order_as_in_the_client() {
        let o = parse("--server x:1").unwrap().unwrap();
        assert_eq!(o.security, SecurityMode::Auto);

        let mut mechanisms = MechanismRegistry::builtin();
        mechanisms
            .register(dummy_mechanism(
                "middle@example.org",
                "Middle",
                Era::Modern,
                2000,
            ))
            .unwrap();
        let o = parse_args(
            ["--server", "x:1"].into_iter().map(String::from),
            &mechanisms,
        )
        .unwrap()
        .unwrap();
        let negotiation =
            client_handshake(&mechanisms, &o.security, &context(1, None, None)).unwrap();
        let MechanismInUse::Automatic(progress) = negotiation.mechanism else {
            panic!("the automatic order")
        };
        let rungs: Vec<&str> = progress.rungs().map(|id| id.as_str()).collect();
        assert_eq!(rungs, ["opensec", "middle@example.org", "vie"]);
        assert_eq!(
            startup_lines(&mechanisms)[1],
            "Automatic order: OpenSecurity, then Middle, then Subspace"
        );

        // By name, the old default among them.
        for word in ["vie", "middle@example.org", "none", "OPENSEC"] {
            let o = parse_args(
                ["--server", "x:1", "--security", word]
                    .into_iter()
                    .map(String::from),
                &mechanisms,
            )
            .unwrap()
            .unwrap();
            let negotiation =
                client_handshake(&mechanisms, &o.security, &context(1, None, None)).unwrap();
            assert_eq!(
                negotiation.mechanism.id().map(|id| id.as_str()),
                Some(word.to_ascii_lowercase().as_str())
            );
        }
        assert_eq!(
            parse("--server x:1 --security middle@example.org").err(),
            Some(
                "--security must be auto, none, vie or opensec, not `middle@example.org`"
                    .to_owned()
            )
        );
    }

    /// The start-up lines, exactly, and the help read from the same mechanisms.
    #[test]
    fn the_start_up_lines_and_help_come_from_the_mechanisms() {
        let builtin = MechanismRegistry::builtin();
        assert_eq!(
            startup_lines(&builtin),
            [
                "Mechanisms supported: None, Subspace, OpenSecurity",
                "Automatic order: OpenSecurity, then Subspace",
            ]
        );
        let help = usage(&builtin);
        let flat = help.split_whitespace().collect::<Vec<_>>().join(" ");
        assert!(
            flat.contains(
                "--security <mode> auto (the default) tries the automatic order, \
                 OpenSecurity, then Subspace,"
            ),
            "{help}"
        );
        assert!(
            flat.contains("Or name one mechanism: none, vie or opensec."),
            "{help}"
        );
        assert!(!help.contains("{security}"));
        // The lines written for `--security` fit, each after the first indented
        // under the option's description.
        let security: Vec<&str> = help
            .lines()
            .skip_while(|l| !l.starts_with("  --security"))
            .take_while(|l| !l.starts_with("  --identity"))
            .collect();
        assert!(security.len() > 1, "{help}");
        for line in &security {
            assert!(line.len() <= 80, "{line:?}");
        }
        for line in &security[1..] {
            assert!(line.starts_with(&" ".repeat(25)), "{line:?}");
        }

        let mut more = MechanismRegistry::builtin();
        more.register(dummy_mechanism(
            "middle@example.org",
            "Middle",
            Era::Modern,
            2000,
        ))
        .unwrap();
        assert_eq!(
            startup_lines(&more)[0],
            "Mechanisms supported: None, Subspace, Middle, OpenSecurity"
        );
        assert!(usage(&more).contains("middle@example.org"));
    }

    #[test]
    fn parses_only_supported_flight_orders() {
        assert_eq!(parse_order(" Cme here! "), Some(OrderRequest::Come));
        assert_eq!(parse_order("follow me"), Some(OrderRequest::Follow));
        assert_eq!(
            parse_order("stay here until I say go"),
            Some(OrderRequest::Hold)
        );
        assert_eq!(parse_order("go"), Some(OrderRequest::Resume));
        assert_eq!(parse_order("go away"), None);
    }

    #[test]
    fn parses_options() {
        let o = parse("--server 127.0.0.1:5000 --ship 3 --count 20 --fly --duration 1.5")
            .unwrap()
            .unwrap();
        assert_eq!((o.ship, o.count, o.fly), (2, 20, true));
        assert_eq!(o.duration, Some(Duration::from_millis(1500)));
        assert_eq!(
            parse("--server x:1 --ship spec").unwrap().unwrap().ship,
            SPECTATOR
        );
        assert!(parse("--ship 2").is_err());
        assert!(parse("--server x:1 --ship 9").is_err());
        assert!(parse("--help").unwrap().is_none());
        let o = parse("--server x:1 --script -").unwrap().unwrap();
        assert_eq!(o.script.as_deref(), Some("-"));
        assert!(parse("--server x:1 --script - --count 2").is_err());
    }

    #[test]
    fn private_chat_syntax() {
        assert_eq!(private_chat(":Bob:?ban -t 5"), Some(("Bob", "?ban -t 5")));
        assert_eq!(private_chat("hello :there:"), None);
        assert_eq!(private_chat("::x"), None);
        let o = parse("--server x:1 --identity ids.txt").unwrap().unwrap();
        assert_eq!(o.identity, Some(PathBuf::from("ids.txt")));
    }

    #[test]
    fn host_args_parse() {
        let o = parse("--host --server a:1").unwrap().unwrap();
        assert!(o.host);
        assert_eq!(o.server, "a:1");
        assert!(o.script.is_none());
        assert!(!parse("--server a:1").unwrap().unwrap().host);
        assert!(parse("--host").is_err(), "--host still needs --server");
        assert!(parse("--host --server a:1 --script -").is_err());
        assert!(parse("--host --server a:1 --count 2").is_err());
    }

    /// Bots answer a security check exactly as the game client (the browser
    /// build's keyed profile) does: the keyed exe checksum a verifying zone
    /// computes, in the 40-byte SubSpace form, logging in as 0x09 v134.
    #[test]
    fn bots_answer_security_checks_as_the_game_client() {
        use oss_session::SecurityChallenge;
        let opts = parse("--server a:1 --name Rex").unwrap().unwrap();
        let lineup = lineup_from_options(&opts, None);
        let cfg = session_config(&lineup.bots[0], &Place::of(&lineup), &opts, 0, 0);
        let game = oss_session::client::vie_keyed();
        for key in [1, 0x1234_5678, 0xDEAD_BEEF] {
            let challenge = SecurityChallenge {
                key,
                map_crc32: Some(7),
            };
            let answer = cfg.client.security(challenge);
            assert_eq!(answer, game.security(challenge));
            assert_eq!(
                answer.exe_checksum,
                oss_proto::checksum::vie_exe_checksum(key)
            );
            assert_ne!(answer.exe_checksum, 0, "not the 1.34 client's zero");
            assert_eq!(answer.continuum, None, "still the 40-byte SubSpace form");
        }
        let login = cfg.client.login();
        assert_eq!((login.kind, login.version), (0x09, 134));
        assert_eq!(
            (login.kind, login.version, login.continuum_id),
            (
                game.login().kind,
                game.login().version,
                game.login().continuum_id
            )
        );
    }

    #[test]
    fn view_targets_bots_on_other_freqs_and_never_teammates() {
        let mut w = World::new(
            oss_sim::TileMap::empty(),
            oss_sim::ArenaSettings::sandbox(),
            1,
        );
        // The bot, on freq 1.
        let me = w.add_ship(1, 0, 1);
        // Another bot (once named "Rex [bot]", a peer) on freq 2: an enemy.
        let other_bot = w.add_ship(2, 0, 2);
        // A pilot on the bot's own freq: a teammate, never a target.
        w.add_ship(3, 0, 1);
        let (mut enemies, mut shots) = (Vec::new(), Vec::new());
        let v = view(&w, me, &mut enemies, &mut shots);
        let want = px(w.ships[other_bot].pos);
        assert_eq!(v.enemies.len(), 1);
        assert_eq!(v.enemies[0].pos, want);
    }
}
