//! How a scripted bot flies: the keys it presses each tick.
//!
//! A ship can only thrust along its nose, so every manoeuvre comes down to
//! turning towards a direction and then pressing forward or backward. Positions
//! here are pixels and speeds pixels per second, as floating point: this is the
//! bot deciding what to press, not the simulation, so it needs no exactness.

use std::f32::consts::TAU;

use oss_sim::ShipInput;

use crate::nav::{Nav, Route, centre, tile_of};
use crate::script::Behaviour;

/// Another ship as the bot sees it.
#[derive(Debug, Clone, Copy)]
pub struct Contact {
    pub pos: (f32, f32),
    pub vel: (f32, f32),
}

/// What the bot knows this tick.
pub struct View<'a> {
    pub tick: u32,
    pub pos: (f32, f32),
    pub vel: (f32, f32),
    /// 0..40000, as the simulation keeps it.
    pub rotation: u32,
    /// Rotation units per tick when turning.
    pub turn_rate: u32,
    /// Pixels per second.
    pub top_speed: f32,
    /// Bullet speed and range in pixels (per second, and how far one lasts).
    pub bullet_speed: f32,
    pub bullet_range: f32,
    /// Energy as a fraction of full.
    pub energy: f32,
    /// Pilots the bot may fight, alive and not in a safe zone.
    pub enemies: &'a [Contact],
    /// Enemy shots in flight.
    pub shots: &'a [Contact],
}

/// Rotation units in a full turn, and per one of the 40 directions.
const FULL: i32 = 40_000;
const STEP: i32 = 1000;
/// Waypoints count as reached this close (pixels).
const REACHED: f32 = 40.0;
/// Roaming: how far off (tiles) to pick the next spot, how fast to go, how many
/// tiles a search for one may look at, and how long (ticks) to try before
/// picking another.
const ROAM_REACH: i32 = 48;
const ROAM_PACE: f32 = 0.55;
/// A pick tries up to four spots, so all four together look at no more tiles
/// than one route search.
const ROAM_SEARCH: usize = SEARCH / 4;
const ROAM_GIVE_UP: u32 = 2_000;
/// Ticks to wait after finding no roaming spot before trying again.
const ROAM_RETRY: u32 = 100;
/// How far along a path to look for a corner to cut to.
const LOOKAHEAD: usize = 14;
/// Tiles one path search may look at per call: a few milliseconds at most in a
/// release build (one that runs its whole budget took 2.3 ms here, 14.7 ms at
/// the old 120 000), so no brain step holds up the bot's thread for long. A
/// search that needs more is not lost: it gives the path to the tile nearest the
/// goal it has seen, which the bot follows, and carries on from where it stopped
/// on the next ticks (see `Nav::path`) until it finds the whole route.
const SEARCH: usize = 20_000;
/// Fire only with at least this much energy left, so a bot doesn't shoot itself
/// to death.
const FIRE_ENERGY: f32 = 0.35;

/// A swarm bot holds this far (pixels) either side of its ring before it moves.
const SWARM_BAND: f32 = 32.0;
/// Ticks between a swarm bot's shots: 1.5 s at the simulation's 100 ticks a second.
const SWARM_FIRE_TICKS: u32 = 150;

/// How far ahead (pixels) a bot breaking off aims, away from the nearest enemy.
const AWAY_PIXELS: f32 = 640.0;
/// A bot leaving for spectator breaks off for at most this long (ms), and
/// goes at once once no enemy is within `SAFE_TILES`.
const BREAK_OFF_MS: u64 = 8000;
pub const SAFE_TILES: f32 = 40.0;

/// A bot that has been told to spectate but is still breaking off its fight.
#[derive(Debug, Clone, Copy)]
pub struct Leaving {
    pub since_ms: u64,
}

impl Leaving {
    /// Time to spectate: 8 s have passed, or no enemy is within 40 tiles.
    pub fn ready(&self, now_ms: u64, enemy_within_40_tiles: bool) -> bool {
        !enemy_within_40_tiles || now_ms >= self.since_ms + BREAK_OFF_MS
    }
}

/// The direction (0..40) from `a` to `b`, 0 being up and counting clockwise.
pub fn direction(dx: f32, dy: f32) -> i32 {
    let a = dx.atan2(-dy).rem_euclid(TAU);
    ((a / TAU * 40.0).round() as i32).rem_euclid(40)
}

fn heading(dir: i32) -> (f32, f32) {
    let a = dir as f32 / 40.0 * TAU;
    (a.sin(), -a.cos())
}

fn len(v: (f32, f32)) -> f32 {
    (v.0 * v.0 + v.1 * v.1).sqrt()
}

fn sub(a: (f32, f32), b: (f32, f32)) -> (f32, f32) {
    (a.0 - b.0, a.1 - b.1)
}

/// Steps (0..=20) between two directions, either way round.
fn apart(a: i32, b: i32) -> i32 {
    let d = (a - b).rem_euclid(40);
    d.min(40 - d)
}

/// Where to aim to hit `t` with a bullet leaving at `speed` from `from` moving at
/// `own_vel` (bullets carry the ship's speed): the target's position when the
/// bullet gets there, or the target itself when it can't be caught.
pub fn lead(from: (f32, f32), own_vel: (f32, f32), t: Contact, speed: f32) -> (f32, f32) {
    let p = sub(t.pos, from);
    let v = sub(t.vel, own_vel);
    // |p + v t| = speed t
    let a = v.0 * v.0 + v.1 * v.1 - speed * speed;
    let b = 2.0 * (p.0 * v.0 + p.1 * v.1);
    let c = p.0 * p.0 + p.1 * p.1;
    let time = if a.abs() < 1e-3 {
        if b.abs() < 1e-3 { 0.0 } else { -c / b }
    } else {
        let disc = b * b - 4.0 * a * c;
        if disc < 0.0 {
            0.0
        } else {
            let r = disc.sqrt();
            let (t1, t2) = ((-b - r) / (2.0 * a), (-b + r) / (2.0 * a));
            [t1, t2]
                .into_iter()
                .filter(|t| *t > 0.0)
                .fold(f32::INFINITY, f32::min)
        }
    };
    let time = if time.is_finite() {
        time.clamp(0.0, 3.0)
    } else {
        0.0
    };
    (t.pos.0 + v.0 * time, t.pos.1 + v.1 * time)
}

pub struct Brain {
    behaviour: Behaviour,
    points: Vec<(i32, i32)>,
    range: f32,
    next_point: usize,
    /// The path being followed and the tile it leads to.
    path: Vec<(i32, i32)>,
    path_goal: Option<(i32, i32)>,
    planned_tick: u32,
    /// `path` ends short of `path_goal` (the search ran out of budget, or the
    /// goal is walled off): re-plan from its end once there.
    partial: bool,
    /// Searching again would get no nearer `path_goal` than now: wait for the
    /// plan to go stale rather than searching every tick.
    no_route: bool,
    /// Where a sentry with no point of its own stands: where it first came in.
    home: Option<(f32, f32)>,
    dodge: Option<(u32, bool)>,
    /// Where the bot is roaming to, and the tick it set off: a fighter with
    /// nobody to fight and no route, or a patrol with no waypoints, flies about
    /// the map rather than sitting where it spawned.
    roam: Option<((i32, i32), u32)>,
    /// When the last pick of a roaming spot found none: the next waits a while.
    roam_failed: Option<u32>,
    /// State of the little generator that picks roaming spots.
    seed: u32,
    /// The tick a swarm bot last fired.
    last_shot_tick: Option<u32>,
}

impl Brain {
    pub fn new(behaviour: Behaviour, points: Vec<(i32, i32)>, range_tiles: i32) -> Self {
        Brain {
            behaviour,
            points,
            range: range_tiles as f32 * 16.0,
            next_point: 0,
            path: Vec::new(),
            path_goal: None,
            planned_tick: 0,
            partial: false,
            no_route: false,
            home: None,
            dodge: None,
            roam: None,
            roam_failed: None,
            seed: 0,
            last_shot_tick: None,
        }
    }

    /// The ship came back to life somewhere new: plans start again.
    pub fn respawned(&mut self) {
        self.path.clear();
        self.path_goal = None;
        self.partial = false;
        self.no_route = false;
        self.roam_failed = None;
        self.dodge = None;
        self.roam = None;
    }

    /// The keys to press this tick.
    pub fn think(&mut self, v: &View, nav: &mut Nav) -> ShipInput {
        match self.behaviour {
            Behaviour::Idle => ShipInput::default(),
            Behaviour::Patrol => self.patrol(v, nav),
            Behaviour::Sentry => self.sentry(v, nav),
            Behaviour::Fighter => self.fighter(v, nav),
            Behaviour::Swarm => self.swarm(v),
        }
    }

    /// Move to the target and brake near it, with no combat input.
    pub fn obey(
        &mut self,
        v: &View,
        nav: &mut Nav,
        target: (f32, f32),
        following: bool,
    ) -> ShipInput {
        let goal = if following && len(sub(v.pos, target)) < 96.0 {
            tile_of(v.pos)
        } else {
            tile_of(target)
        };
        self.travel(v, nav, goal, 0.0)
    }

    /// Breaking off a fight: turn and run from the nearest enemy, afterburner
    /// on, with no weapon pressed. With no enemy about, nothing to do.
    pub fn disengage(&mut self, v: &View, nav: &mut Nav) -> ShipInput {
        let Some(near) = v
            .enemies
            .iter()
            .min_by(|a, b| len(sub(a.pos, v.pos)).total_cmp(&len(sub(b.pos, v.pos))))
        else {
            return ShipInput::default();
        };
        let away = sub(v.pos, near.pos);
        let d = len(away).max(1.0);
        let goal = tile_of((
            v.pos.0 + away.0 / d * AWAY_PIXELS,
            v.pos.1 + away.1 / d * AWAY_PIXELS,
        ));
        let mut input = self.travel(v, nav, goal, 1.0);
        input.gun = false;
        input.bomb = false;
        input.afterburner = input.forward;
        input
    }

    fn patrol(&mut self, v: &View, nav: &mut Nav) -> ShipInput {
        if self.points.is_empty() {
            return self.wander(v, nav);
        }
        let goal = self.points[self.next_point % self.points.len()];
        if len(sub(centre(goal), v.pos)) < REACHED && self.points.len() > 1 {
            self.next_point = (self.next_point + 1) % self.points.len();
            self.path.clear();
        }
        let goal = self.points[self.next_point % self.points.len()];
        // Target practice: a steady pace, not flat out.
        let pace = if self.points.len() > 1 { 0.6 } else { 0.0 };
        self.travel(v, nav, goal, pace)
    }

    fn sentry(&mut self, v: &View, nav: &mut Nav) -> ShipInput {
        let home = match self.points.first() {
            Some(&p) => centre(p),
            None => *self.home.get_or_insert(v.pos),
        };
        let target = v
            .enemies
            .iter()
            .filter(|e| len(sub(e.pos, home)) <= self.range && len(sub(e.pos, v.pos)) <= self.range)
            .filter(|e| nav.in_sight(v.pos, e.pos))
            .min_by(|a, b| len(sub(a.pos, v.pos)).total_cmp(&len(sub(b.pos, v.pos))));
        let Some(&t) = target else {
            return self.travel(v, nav, tile_of(home), 0.0);
        };
        let mut input = self.aim_and_fire(v, t);
        // Hold the spot while shooting: brake if drifting off it along the nose.
        let off = sub(home, v.pos);
        if len(off) > 48.0 || len(v.vel) > 40.0 {
            let want = sub((off.0 * 0.5, off.1 * 0.5), v.vel);
            let nose = heading(self.facing(v));
            let along = want.0 * nose.0 + want.1 * nose.1;
            input.forward = along > 20.0;
            input.backward = along < -20.0;
        }
        input
    }

    /// Holds a ring `range` out from the nearest enemy, facing it: closes in
    /// when farther, backs off when nearer, brakes within `SWARM_BAND` of the
    /// ring (so a push out of it is always followed by a return). Fires only
    /// when lined up and not within `SWARM_FIRE_TICKS` of the last shot; never
    /// afterburns, bombs or dodges. With no enemy, stops where it is.
    fn swarm(&mut self, v: &View) -> ShipInput {
        let target = v
            .enemies
            .iter()
            .min_by(|a, b| len(sub(a.pos, v.pos)).total_cmp(&len(sub(b.pos, v.pos))))
            .copied();
        let Some(t) = target else {
            return seek(v, v.pos, 0.0);
        };
        let mut input = self.aim_and_fire(v, t);
        let ready = self
            .last_shot_tick
            .is_none_or(|last| v.tick.wrapping_sub(last) >= SWARM_FIRE_TICKS);
        if input.gun && ready {
            self.last_shot_tick = Some(v.tick);
        } else {
            input.gun = false;
        }
        let to = sub(t.pos, v.pos);
        let dist = len(to).max(1.0);
        let err = dist - self.range;
        // Speed along the line to the enemy: towards it when outside the ring.
        let radial = if err.abs() <= SWARM_BAND {
            0.0
        } else {
            (err * 2.0).clamp(-v.top_speed * 0.5, v.top_speed * 0.8)
        };
        let want = (to.0 / dist * radial, to.1 / dist * radial);
        let steer = sub(want, v.vel);
        let nose = heading(self.facing(v));
        let along = steer.0 * nose.0 + steer.1 * nose.1;
        input.forward = along > 20.0;
        input.backward = along < -20.0;
        input
    }

    fn fighter(&mut self, v: &View, nav: &mut Nav) -> ShipInput {
        let target = v
            .enemies
            .iter()
            .min_by(|a, b| len(sub(a.pos, v.pos)).total_cmp(&len(sub(b.pos, v.pos))))
            .copied();
        let Some(t) = target else {
            // Nobody to fight: fly the route if it has one, else roam and look for someone.
            return if self.points.is_empty() {
                self.wander(v, nav)
            } else {
                self.patrol(v, nav)
            };
        };
        let dist = len(sub(t.pos, v.pos));
        if !nav.in_sight(v.pos, t.pos) {
            return self.travel(v, nav, tile_of(t.pos), 1.0);
        }
        self.path.clear();
        let mut input = self.aim_and_fire(v, t);
        let aim = self.aim_dir(v, t);
        let lined_up = apart(self.facing(v), aim) <= 3;
        if dist > 260.0 && lined_up {
            input.forward = true;
            input.afterburner = dist > 700.0 && v.energy > 0.6;
        } else if dist < 140.0 {
            input.backward = true;
        }
        if let Some(side) = self.incoming(v) {
            // Slip sideways out of the shot's way, along whichever end of the
            // ship points away from it.
            input.forward = side;
            input.backward = !side;
            input.afterburner = v.energy > 0.5;
        }
        input
    }

    /// Whether an enemy shot is about to hit: `Some(true)` to dodge forward,
    /// `Some(false)` backward. A dodge lasts a quarter of a second.
    fn incoming(&mut self, v: &View) -> Option<bool> {
        if let Some((until, side)) = self.dodge {
            if v.tick.wrapping_sub(until) as i32 <= 0 {
                return Some(side);
            }
            self.dodge = None;
        }
        let nose = heading(self.facing(v));
        for s in v.shots {
            let p = sub(s.pos, v.pos);
            let rel = sub(s.vel, v.vel);
            let speed2 = rel.0 * rel.0 + rel.1 * rel.1;
            if speed2 < 1.0 {
                continue;
            }
            let t = -(p.0 * rel.0 + p.1 * rel.1) / speed2;
            if !(0.0..0.7).contains(&t) {
                continue;
            }
            let miss = len((p.0 + rel.0 * t, p.1 + rel.1 * t));
            if miss > 26.0 {
                continue;
            }
            // Move across the shot's line: the side its closest pass misses on,
            // so the miss grows.
            let across = (-rel.1, rel.0);
            let closest = (p.0 + rel.0 * t, p.1 + rel.1 * t);
            let away = if closest.0 * across.0 + closest.1 * across.1 > 0.0 {
                (-across.0, -across.1)
            } else {
                across
            };
            let side = away.0 * nose.0 + away.1 * nose.1 >= 0.0;
            self.dodge = Some((v.tick.wrapping_add(25), side));
            return Some(side);
        }
        None
    }

    fn facing(&self, v: &View) -> i32 {
        (v.rotation / STEP as u32 % 40) as i32
    }

    fn aim_dir(&self, v: &View, t: Contact) -> i32 {
        let at = lead(v.pos, v.vel, t, v.bullet_speed);
        direction(at.0 - v.pos.0, at.1 - v.pos.1)
    }

    /// Turns towards where `t` will be and fires when the nose is on it.
    fn aim_and_fire(&self, v: &View, t: Contact) -> ShipInput {
        let aim = self.aim_dir(v, t);
        let mut input = ShipInput::default();
        turn_to(&mut input, v, aim);
        let dist = len(sub(t.pos, v.pos));
        input.gun = apart(self.facing(v), aim) == 0
            && dist < v.bullet_range * 0.8
            && v.energy > FIRE_ENERGY;
        input
    }

    /// Roams: flies at an easy pace to a reachable open spot some way off, and
    /// picks another on arrival, or after a while if it cannot get there.
    fn wander(&mut self, v: &View, nav: &mut Nav) -> ShipInput {
        let here = tile_of(v.pos);
        let due = match self.roam {
            None => self
                .roam_failed
                .is_none_or(|t| v.tick.wrapping_sub(t) > ROAM_RETRY),
            Some((goal, since)) => {
                len(sub(centre(goal), v.pos)) < REACHED * 2.0
                    || v.tick.wrapping_sub(since) > ROAM_GIVE_UP
            }
        };
        if due {
            self.roam = None;
            if self.seed == 0 {
                self.seed =
                    (v.pos.0 as u32).wrapping_mul(2_654_435_761) ^ (v.pos.1 as u32) ^ v.tick | 1;
            }
            for _ in 0..4 {
                let dx = self.random(ROAM_REACH * 2 + 1) - ROAM_REACH;
                let dy = self.random(ROAM_REACH * 2 + 1) - ROAM_REACH;
                let want = ((here.0 + dx).clamp(1, 1022), (here.1 + dy).clamp(1, 1022));
                if (dx.abs().max(dy.abs())) < ROAM_REACH / 3 {
                    continue;
                }
                // A partial path still ends somewhere reachable: go there.
                if let Some(path) = nav.path(here, want, ROAM_SEARCH).tiles()
                    && let Some(&end) = path.last()
                    && end != here
                {
                    self.roam = Some((end, v.tick));
                    self.path = path;
                    self.path_goal = Some(end);
                    self.partial = false;
                    self.no_route = false;
                    self.planned_tick = v.tick;
                    break;
                }
            }
            self.roam_failed = self.roam.is_none().then_some(v.tick);
        }
        match self.roam {
            Some((goal, _)) => self.travel(v, nav, goal, ROAM_PACE),
            None => ShipInput::default(),
        }
    }

    /// 0..n from a xorshift generator.
    fn random(&mut self, n: i32) -> i32 {
        let mut x = self.seed;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.seed = x;
        (x % n.max(1) as u32) as i32
    }

    /// Flies to tile `goal`, round walls where needed, at `pace` of top speed;
    /// a pace of 0 stops on the tile and stays there.
    fn travel(&mut self, v: &View, nav: &mut Nav, goal: (i32, i32), pace: f32) -> ShipInput {
        let target = self.waypoint(v, nav, goal);
        let last_leg = target == centre(goal);
        let pace = if last_leg { pace } else { pace.max(0.6) };
        seek(v, target, pace)
    }

    /// The point (pixels) to head for on the way to tile `goal`: the goal itself
    /// in a clear line, else the furthest tile of the planned path in sight.
    fn waypoint(&mut self, v: &View, nav: &mut Nav, goal: (i32, i32)) -> (f32, f32) {
        let goal_px = centre(goal);
        if nav.clear_path(v.pos, goal_px) {
            self.path.clear();
            goal_px
        } else {
            let stale = v.tick.wrapping_sub(self.planned_tick) > 150;
            let new_goal = self.path_goal != Some(goal);
            // A partial path followed to its end: carry on from there.
            let at_partial_end = self.partial && self.path.len() <= 1;
            let wanted = new_goal
                || stale
                || nav.searching(goal)
                || ((self.path.is_empty() || at_partial_end) && !self.no_route);
            if wanted {
                let was_end = self.path.last().copied().filter(|_| !new_goal);
                match nav.path(tile_of(v.pos), goal, SEARCH) {
                    Route::Found(p) => {
                        self.path = p;
                        self.partial = false;
                        self.no_route = false;
                    }
                    Route::Partial(p) => {
                        // Still searching, or got nearer than last time: progress.
                        // Otherwise this is as near as it gets for now.
                        let end = p.last().copied();
                        self.no_route = !nav.searching(goal)
                            && (p.len() <= 1 || (self.partial && end == was_end));
                        self.path = p;
                        self.partial = true;
                    }
                    Route::Unreachable => {
                        // Head straight for it meanwhile.
                        self.path.clear();
                        self.partial = false;
                        self.no_route = true;
                    }
                }
                self.path_goal = Some(goal);
                self.planned_tick = v.tick;
            }
            // Drop the tiles already passed (all before the one nearest the bot,
            // then that one too once close), then head for the furthest one still
            // in a straight, open line.
            anchor(&mut self.path, v.pos, nav);
            while self.path.len() > 1 && len(sub(centre(self.path[0]), v.pos)) < 24.0 {
                self.path.remove(0);
            }
            let ahead = self
                .path
                .iter()
                .take(LOOKAHEAD)
                .rposition(|&t| nav.clear_path(v.pos, centre(t)));
            match ahead {
                Some(i) => centre(self.path[i]),
                None => self.path.first().map(|&t| centre(t)).unwrap_or(goal_px),
            }
        }
    }
}

/// How far into a new path (tiles) to look for the bot's own place on it.
const ANCHOR_TILES: usize = 2 * crate::nav::RESUME_TILES as usize + 8;

/// Keeps `path` starting where the bot is. A resumed search traces back to where
/// it first started, up to `RESUME_TILES` behind the bot, and a bot that cuts
/// a corner never passes within reach of some tiles; a head left behind like
/// that would never be dropped, and the bot would aim back for it. So this drops
/// the tiles before the one nearest the bot (in a clear line from it) among the
/// first [`ANCHOR_TILES`], not further, so a path that doubles back past the bot
/// on the far side of a wall is never cut short.
fn anchor(path: &mut Vec<(i32, i32)>, pos: (f32, f32), nav: &Nav) {
    let nearest = path
        .iter()
        .take(ANCHOR_TILES)
        .enumerate()
        .filter(|&(_, &t)| nav.clear_path(pos, centre(t)))
        .min_by(|a, b| {
            let d = |t: (i32, i32)| len(sub(centre(t), pos));
            d(*a.1).total_cmp(&d(*b.1))
        })
        .map(|(i, _)| i);
    if let Some(i) = nearest {
        path.drain(..i);
    }
}

fn turn_to(input: &mut ShipInput, v: &View, dir: i32) {
    // Aim for the middle of the direction so a turn of a few units either way
    // doesn't change which way the nose reports.
    let want = dir * STEP + STEP / 2;
    let delta = (want - v.rotation as i32).rem_euclid(FULL);
    let delta = if delta > FULL / 2 {
        delta - FULL
    } else {
        delta
    };
    let slack = (v.turn_rate as i32 / 2).max(STEP / 3);
    if delta > slack {
        input.right = true;
    } else if delta < -slack {
        input.left = true;
    }
}

/// Steers towards `target` (pixels), arriving at `pace` of top speed (0 to stop
/// there).
fn seek(v: &View, target: (f32, f32), pace: f32) -> ShipInput {
    let mut input = ShipInput::default();
    let to = sub(target, v.pos);
    let dist = len(to);
    let cruise = v.top_speed * pace.max(0.0);
    // Slow down near a stop, at a rate a ship can manage.
    let stopping = (dist * 2.0).min(v.top_speed);
    let speed = if pace == 0.0 { stopping } else { cruise };
    let want = if dist < 1.0 {
        (0.0, 0.0)
    } else {
        (to.0 / dist * speed, to.1 / dist * speed)
    };
    let steer = sub(want, v.vel);
    if len(steer) < 12.0 {
        if dist > 4.0 {
            turn_to(&mut input, v, direction(to.0, to.1));
        }
        return input;
    }
    let facing = (v.rotation / STEP as u32 % 40) as i32;
    let steer_dir = direction(steer.0, steer.1);
    let off = apart(facing, steer_dir);
    if off >= 16 && len(v.vel) > 30.0 {
        // The push needed is behind the ship: brake with reverse thrust.
        input.backward = true;
        turn_to(&mut input, v, steer_dir);
    } else {
        turn_to(&mut input, v, steer_dir);
        input.forward = off <= 3;
    }
    input
}

#[cfg(test)]
mod tests {
    use super::*;
    use oss_sim::TileMap;

    #[test]
    fn directions_and_leading() {
        assert_eq!(direction(0.0, -1.0), 0);
        assert_eq!(direction(1.0, 0.0), 10);
        assert_eq!(direction(0.0, 1.0), 20);
        assert_eq!(direction(-1.0, 0.0), 30);
        assert_eq!(apart(39, 1), 2);
        // A target crossing at speed is led ahead of where it is.
        let t = Contact {
            pos: (100.0, -400.0),
            vel: (100.0, 0.0),
        };
        let at = lead((0.0, 0.0), (0.0, 0.0), t, 400.0);
        assert!(at.0 > 180.0 && at.0 < 240.0, "{at:?}");
        // One that can't be caught is aimed at directly.
        let fast = Contact {
            pos: (0.0, -100.0),
            vel: (0.0, -1000.0),
        };
        assert_eq!(lead((0.0, 0.0), (0.0, 0.0), fast, 200.0), fast.pos);
    }

    fn view<'a>(enemies: &'a [Contact], shots: &'a [Contact]) -> View<'a> {
        View {
            tick: 0,
            pos: (8000.0, 8000.0),
            vel: (0.0, 0.0),
            rotation: 500,
            turn_rate: 200,
            top_speed: 300.0,
            bullet_speed: 400.0,
            bullet_range: 2000.0,
            energy: 1.0,
            enemies,
            shots,
        }
    }

    #[test]
    fn disengaging_flies_away_from_the_nearest_enemy() {
        let mut nav = Nav::new(&TileMap::empty(), 14);
        let mut brain = Brain::new(Behaviour::Fighter, Vec::new(), 30);
        let far = Contact {
            pos: (8000.0, 6000.0),
            vel: (0.0, 0.0),
        };
        let east = Contact {
            pos: (8300.0, 8000.0),
            vel: (0.0, 0.0),
        };
        let enemies = [far, east];
        let mut v = view(&enemies, &[]);
        // Facing up with the enemy to the east: turn left, towards the west.
        let i = brain.disengage(&v, &mut nav);
        assert!(i.left && !i.right, "{i:?}");
        assert!(!i.gun && !i.bomb);
        // Facing west: thrust, afterburner, still no gun.
        v.rotation = 30_500;
        let i = brain.disengage(&v, &mut nav);
        assert!(i.forward && i.afterburner && !i.gun && !i.bomb, "{i:?}");
        let none = brain.disengage(&view(&[], &[]), &mut nav);
        assert!(!none.forward && !none.left && !none.right && !none.gun);
    }

    #[test]
    fn leaving_is_ready_after_eight_seconds_or_with_no_enemy_near() {
        let l = Leaving { since_ms: 1000 };
        assert!(!l.ready(1000, true));
        assert!(!l.ready(8999, true));
        assert!(l.ready(9000, true));
        assert!(l.ready(1000, false));
    }

    fn swarm_view<'a>(enemies: &'a [Contact], tick: u32) -> View<'a> {
        View {
            tick,
            ..view(enemies, &[])
        }
    }

    fn ahead_of_view(dist: f32) -> [Contact; 1] {
        // The view faces up (rotation 500, direction 0) from (8000, 8000).
        [Contact {
            pos: (8000.0, 8000.0 - dist),
            vel: (0.0, 0.0),
        }]
    }

    #[test]
    fn swarm_closes_to_its_ring_and_stops() {
        let mut nav = Nav::new(&TileMap::empty(), 14);
        // Range 12 tiles = 192 px. Enemy 600 px ahead: close in.
        let mut brain = Brain::new(Behaviour::Swarm, vec![], 12);
        let far = ahead_of_view(600.0);
        let i = brain.think(&swarm_view(&far, 0), &mut nav);
        assert!(
            i.forward && !i.backward && !i.afterburner && !i.bomb,
            "{i:?}"
        );
        // On the ring, and within 32 px either side of it: no drift.
        for d in [160.0, 192.0, 224.0] {
            let e = ahead_of_view(d);
            let i = brain.think(&swarm_view(&e, 1), &mut nav);
            assert!(!i.forward && !i.backward, "{d}: {i:?}");
        }
        // Much nearer than the ring: back off.
        let near = ahead_of_view(60.0);
        let i = brain.think(&swarm_view(&near, 2), &mut nav);
        assert!(i.backward && !i.forward, "{i:?}");
        // Drifting in on the ring: brake.
        let e = ahead_of_view(192.0);
        let mut v = swarm_view(&e, 3);
        v.vel = (0.0, -150.0);
        assert!(brain.think(&v, &mut nav).backward);
        // Pushed back out (facing the enemy): it returns.
        let out = ahead_of_view(400.0);
        assert!(brain.think(&swarm_view(&out, 4), &mut nav).forward);
    }

    #[test]
    fn swarm_turns_to_its_enemy_and_idles_without_one() {
        let mut nav = Nav::new(&TileMap::empty(), 14);
        let mut brain = Brain::new(Behaviour::Swarm, vec![], 12);
        let right = [Contact {
            pos: (8300.0, 8000.0),
            vel: (0.0, 0.0),
        }];
        let i = brain.think(&swarm_view(&right, 0), &mut nav);
        assert!(i.right && !i.left && !i.gun, "{i:?}");
        assert_eq!(
            brain.think(&swarm_view(&[], 1), &mut nav),
            ShipInput::default()
        );
    }

    #[test]
    fn swarm_fires_no_more_than_every_1_5_s() {
        let mut nav = Nav::new(&TileMap::empty(), 14);
        let mut brain = Brain::new(Behaviour::Swarm, vec![], 12);
        let e = ahead_of_view(192.0);
        let shots: Vec<u32> = (0..300)
            .filter(|&tick| brain.think(&swarm_view(&e, tick), &mut nav).gun)
            .collect();
        assert_eq!(shots, vec![0, 150], "{shots:?}");
    }

    #[test]
    fn swarm_never_afterburns() {
        let mut nav = Nav::new(&TileMap::empty(), 14);
        let mut brain = Brain::new(Behaviour::Swarm, vec![], 12);
        for d in [20.0, 192.0, 700.0, 3000.0] {
            let e = ahead_of_view(d);
            let shot = [Contact {
                pos: (8000.0, 7990.0),
                vel: (0.0, 400.0),
            }];
            let mut v = swarm_view(&e, 0);
            v.shots = &shot;
            v.energy = 1.0;
            let i = brain.think(&v, &mut nav);
            assert!(!i.afterburner && !i.bomb, "{d}: {i:?}");
        }
    }

    #[test]
    fn behaviours_press_sensible_keys() {
        let mut nav = Nav::new(&TileMap::empty(), 14);
        // Facing up with an enemy straight ahead: a sentry fires.
        let ahead = [Contact {
            pos: (8008.0, 7700.0),
            vel: (0.0, 0.0),
        }];
        let mut sentry = Brain::new(Behaviour::Sentry, vec![], 30);
        let i = sentry.think(&view(&ahead, &[]), &mut nav);
        assert!(i.gun && !i.left && !i.right);
        // Out of range: it holds fire.
        let far = [Contact {
            pos: (8008.0, 7000.0),
            vel: (0.0, 0.0),
        }];
        assert!(!sentry.think(&view(&far, &[]), &mut nav).gun);
        // An enemy to the right: turn right.
        let right = [Contact {
            pos: (8300.0, 8000.0),
            vel: (0.0, 0.0),
        }];
        let i = sentry.think(&view(&right, &[]), &mut nav);
        assert!(i.right && !i.gun);
        // A fighter closes in on a distant enemy it is facing.
        let mut fighter = Brain::new(Behaviour::Fighter, vec![], 30);
        let i = fighter.think(&view(&far, &[]), &mut nav);
        assert!(i.forward);
        // Idle presses nothing.
        let mut idle = Brain::new(Behaviour::Idle, vec![], 30);
        assert_eq!(
            idle.think(&view(&ahead, &[]), &mut nav),
            ShipInput::default()
        );
        // A patrol heading for a point straight above thrusts.
        let mut patrol = Brain::new(Behaviour::Patrol, vec![(500, 480), (520, 480)], 30);
        assert!(patrol.think(&view(&[], &[]), &mut nav).forward);
    }

    /// A fighter with nobody to fight and no route, and a patrol with no
    /// waypoints, fly about rather than sit where they spawned.
    #[test]
    fn with_nothing_to_do_fighters_and_patrols_roam() {
        for behaviour in [Behaviour::Fighter, Behaviour::Patrol] {
            let mut nav = Nav::new(&TileMap::empty(), 14);
            let mut brain = Brain::new(behaviour, vec![], 30);
            let mut pressed = false;
            for tick in 0..200 {
                let mut v = view(&[], &[]);
                v.tick = tick;
                let i = brain.think(&v, &mut nav);
                pressed |= i.forward || i.left || i.right;
            }
            assert!(pressed, "{behaviour:?} sat still");
            assert!(brain.roam.is_some(), "{behaviour:?} picked nowhere to go");
        }
        // Idle still sits still.
        let mut nav = Nav::new(&TileMap::empty(), 14);
        let mut idle = Brain::new(Behaviour::Idle, vec![], 30);
        assert_eq!(idle.think(&view(&[], &[]), &mut nav), ShipInput::default());
    }

    /// A goal no route reaches is searched for once per plan (a few budgets, over a
    /// few ticks), not every tick: each failed search runs its whole budget.
    #[test]
    fn a_route_that_cannot_be_found_is_not_searched_for_every_tick() {
        // The patrol point (500, 470) sits inside a closed ring of wall.
        let mut cells = vec![0u8; 1024 * 1024];
        for d in -12i32..=12 {
            for (x, y) in [
                (500 + d, 458),
                (500 + d, 482),
                (488, 470 + d),
                (512, 470 + d),
            ] {
                cells[(y * 1024 + x) as usize] = 1;
            }
        }
        let mut nav = Nav::new(&TileMap::from_cells(&cells), 14);
        let mut patrol = Brain::new(Behaviour::Patrol, vec![(500, 470)], 30);
        for tick in 0..100 {
            let mut v = view(&[], &[]);
            v.tick = tick;
            patrol.think(&v, &mut nav);
        }
        assert!(
            nav.searches <= crate::nav::MAX_RESUMES as usize,
            "{}",
            nav.searches
        );
    }

    /// A bot that moves on while its search resumes gets a path starting where
    /// it is, not where the search began, and keeps heading for the goal: never
    /// back for the tiles behind it.
    #[test]
    fn a_resumed_path_starts_where_the_bot_now_is() {
        let mut cells = vec![0u8; 1024 * 1024];
        for y in 0..=700 {
            cells[y * 1024 + 520] = 1;
        }
        let mut nav = Nav::new(&TileMap::from_cells(&cells), 14);
        let goal = (540, 500);
        let mut patrol = Brain::new(Behaviour::Patrol, vec![goal], 30);
        let mut v = view(&[], &[]);
        // Each tick the bot gets to where it was heading, but no more than a
        // tile down and one across (it moves on while the search resumes).
        for tick in 0..2_000 {
            v.tick = tick;
            let target = patrol.waypoint(&v, &mut nav, goal);
            // The path starts at (or just past) its tile nearest the bot, of those
            // the bot can fly to straight.
            let d = |t: &(i32, i32)| len(sub(centre(*t), v.pos));
            if let Some(head) = patrol.path.first() {
                let nearest = patrol
                    .path
                    .iter()
                    .filter(|t| nav.clear_path(v.pos, centre(**t)))
                    .map(d)
                    .fold(f32::MAX, f32::min);
                assert!(
                    d(head) <= nearest + 24.0,
                    "tick {tick}: path starts at {head:?}, bot at {:?}",
                    tile_of(v.pos)
                );
            }
            if tile_of(v.pos) == goal {
                return;
            }
            let (dx, dy) = sub(target, v.pos);
            v.pos.0 += dx.clamp(-16.0, 16.0);
            v.pos.1 += dy.clamp(-16.0, 16.0);
        }
        panic!("never got there: stuck at {:?}", tile_of(v.pos));
    }

    /// A detour too long for one 20 000-tile search is still found: the bot gets
    /// a partial path toward the goal at once and the whole route a few ticks on.
    #[test]
    fn a_long_detour_is_found_over_a_few_ticks() {
        // A wall across x = 500 from the top down to y = 700; the bot at
        // (500, 500)'s left, the patrol point at its right.
        let mut cells = vec![0u8; 1024 * 1024];
        for y in 0..=700 {
            cells[y * 1024 + 520] = 1;
        }
        let mut nav = Nav::new(&TileMap::from_cells(&cells), 14);
        let goal = (540, 500);
        let mut patrol = Brain::new(Behaviour::Patrol, vec![goal], 30);
        let mut v = view(&[], &[]);
        patrol.think(&v, &mut nav);
        assert!(
            patrol.partial && patrol.path.len() > 1,
            "a partial path at once"
        );
        for tick in 1..20 {
            v.tick = tick;
            patrol.think(&v, &mut nav);
        }
        assert!(!patrol.partial);
        assert_eq!(patrol.path.last(), Some(&goal));
        assert!(patrol.path.iter().any(|&(x, y)| x == 520 && y > 700));
    }

    #[test]
    fn fighter_dodges_a_shot() {
        let mut nav = Nav::new(&TileMap::empty(), 14);
        let shot = [Contact {
            pos: (7800.0, 8000.0),
            vel: (600.0, 0.0),
        }];
        let far = [Contact {
            pos: (8008.0, 6000.0),
            vel: (0.0, 0.0),
        }];
        let mut fighter = Brain::new(Behaviour::Fighter, vec![], 30);
        let i = fighter.think(&view(&far, &shot), &mut nav);
        assert!(i.forward != i.backward && i.afterburner);
    }
}
