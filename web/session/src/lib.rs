//! Browser bindings for the desktop client's session and fixed-step simulation.
use oss_session::sim::{arena_world, own_position, remote_state, remote_weapon, weapon_for};
use oss_session::{RemotePosition, Session, SessionConfig, SessionEvent};
use oss_sim::{Event, ShipInput, World};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct BrowserSession {
    session: Session,
    map: Option<Vec<u8>>,
    world: Option<World>,
    seeds: Option<(u32, u32, u32)>,
    tick_ms: u32,
    inputs: Vec<ShipInput>,
}

#[wasm_bindgen]
impl BrowserSession {
    #[wasm_bindgen(constructor)]
    pub fn new(name: String, password: String, arena: String, now_ms: u32, seed: u32,
               server_ip: u32, source_port: u16) -> BrowserSession {
        let config = SessionConfig {
            name, password,
            arena: (!arena.is_empty()).then_some(arena),
            ship: 0,
            ..SessionConfig::default()
        };
        let mut session = Session::new(config, u64::from(now_ms), seed);
        session.set_local_endpoint(server_ip, source_port);
        Self { session, map: None, world: None, seeds: None, tick_ms: now_ms, inputs: Vec::new() }
    }

    pub fn receive(&mut self, now_ms: u32, datagram: &[u8]) {
        self.session.handle_datagram(u64::from(now_ms), datagram);
    }

    pub fn transmit(&mut self) -> Option<Vec<u8>> {
        self.session.poll_transmit()
    }

    pub fn state(&self) -> String {
        format!("{:?}", self.session.state())
    }

    pub fn update(&mut self, now_ms: u32, keys: u8) -> Option<String> {
        let now = u64::from(now_ms);
        let mut error = None;
        while let Some(event) = self.session.poll_event() {
            match event {
                SessionEvent::MapNeeded { .. } => self.session.download_map(),
                SessionEvent::MapReady { data, .. } => self.map = Some(data),
                SessionEvent::Playing => self.enter(now_ms),
                SessionEvent::Seeds { green_seed, door_seed, timestamp } => {
                    self.seeds = Some((green_seed, door_seed, timestamp));
                    if let Some(world) = &mut self.world {
                        world.prize_seed = green_seed;
                        world.sync_doors(door_seed, timestamp);
                    }
                }
                SessionEvent::PlayerEntered(pid) | SessionEvent::ShipChanged { pid, .. } => {
                    if let Some(world) = &mut self.world {
                        world.remove_ship(pid as u16);
                        add_player(world, &self.session, pid);
                    }
                }
                SessionEvent::FreqChanged { pid, freq } => {
                    if let Some(world) = &mut self.world
                        && let Some(index) = world.ship_index(pid as u16)
                    {
                        world.ships[index].freq = freq as u16;
                    }
                }
                SessionEvent::PlayerLeft(pid) => {
                    if let Some(world) = &mut self.world {
                        world.remove_ship(pid as u16);
                    }
                }
                SessionEvent::PlayerMoved(pid) => {
                    if let Some(position) = self.session.player(pid).and_then(|p| p.position) {
                        self.remote(pid, position, true, now);
                    }
                }
                SessionEvent::PlayerFired { pid, at } => self.remote(pid, at, false, now),
                SessionEvent::LoginFailed { message, zone_text, .. } => {
                    error = Some(zone_text.unwrap_or_else(|| message.into()));
                }
                SessionEvent::NewUserRequired { .. } => {
                    error = Some("Claim your pilot name on Foundation before logging in.".into());
                }
                _ => {}
            }
        }
        self.session.tick(now);
        self.fly(now_ms, keys);
        self.session.flush(now);
        error
    }

    /// Interleaved x/y world pixels. The first pair is the local ship when present.
    pub fn positions(&self) -> Vec<f32> {
        let Some(world) = &self.world else { return Vec::new() };
        let own = self.session.pid().map(|pid| pid as u16);
        world.ships.iter().filter(|ship| Some(ship.id) == own)
            .chain(world.ships.iter().filter(|ship| Some(ship.id) != own && ship.is_alive()))
            .map(|ship| ship.pos.to_f32())
            .flat_map(|(x,y)| [x,y])
            .collect()
    }

    pub fn projectiles(&self) -> Vec<f32> {
        self.world.iter().flat_map(|world| &world.projectiles)
            .flat_map(|shot| { let (x,y) = shot.pos.to_f32(); [x,y] }).collect()
    }

    pub fn tiles(&self) -> Vec<f32> {
        self.map.as_deref().and_then(|data| oss_formats::Level::parse(data).ok())
            .map_or_else(Vec::new, |level| level.tiles.iter()
                .flat_map(|tile| [f32::from(tile.x) * 16.0 + 8.0, f32::from(tile.y) * 16.0 + 8.0])
                .collect())
    }

    pub fn disconnect(&mut self, now_ms: u32) {
        self.session.disconnect();
        self.session.flush(u64::from(now_ms));
    }
}

fn add_player(world: &mut World, session: &Session, pid: i16) {
    if let Some(player) = session.player(pid).filter(|player| player.ship < 8) {
        if session.pid() == Some(pid) {
            world.add_ship(pid as u16, player.ship, player.freq as u16);
        } else {
            world.add_remote_ship(pid as u16, player.ship, player.freq as u16);
        }
    }
}

impl BrowserSession {
    fn enter(&mut self, now_ms: u32) {
        let (Some(map), Some(settings)) = (&self.map, self.session.settings()) else { return };
        let (mut world, _) = arena_world(map, settings, self.seeds.map_or(0, |seeds| seeds.0));
        world.set_tick(self.session.server_ticks(u64::from(now_ms)));
        if let Some((_, door, timestamp)) = self.seeds {
            world.sync_doors(door, timestamp);
        }
        for player in self.session.players() {
            add_player(&mut world, &self.session, player.pid);
        }
        self.world = Some(world);
        self.tick_ms = now_ms;
    }

    fn remote(&mut self, pid: i16, report: RemotePosition, move_ship: bool, now: u64) {
        if self.session.pid() == Some(pid) || report.x < 0 || report.y < 0 {
            return;
        }
        let Some(world) = &mut self.world else { return };
        let Some(index) = world.ship_index(pid as u16) else { return };
        let age = (self.session.server_ticks(now).wrapping_sub(report.server_ticks) as i32).max(0) as u32;
        let state = remote_state(&report);
        if move_ship {
            world.set_remote_state(index, &state, age);
        } else if let Some(weapon) = report.weapon.and_then(remote_weapon) {
            world.fire_remote(index, &state, weapon, age, &mut Vec::new());
        }
    }

    fn fly(&mut self, now_ms: u32, keys: u8) {
        let Some(world) = &mut self.world else { return };
        let Some(id) = self.session.pid().map(|pid| pid as u16) else { return };
        let Some(index) = world.ship_index(id) else { return };
        self.tick_ms = self.tick_ms.max(now_ms.saturating_sub(80));
        self.inputs.resize(world.ships.len(), ShipInput::default());
        self.inputs.fill(ShipInput::default());
        self.inputs[index] = ShipInput {
            forward: keys & 1 != 0, backward: keys & 2 != 0,
            left: keys & 4 != 0, right: keys & 8 != 0,
            gun: keys & 16 != 0, bomb: keys & 32 != 0,
            ..ShipInput::default()
        };
        let now = u64::from(now_ms);
        while now_ms.saturating_sub(self.tick_ms) >= 10 {
            self.tick_ms += 10;
            for event in world.step(&self.inputs) {
                let ship = &world.ships[index];
                if let Some(weapon) = weapon_for(&event, ship) {
                    self.session.send_position_at(now, u64::from(self.tick_ms), &own_position(ship, Some(weapon)));
                }
                match event {
                    Event::ShipKilled { ship: victim, killer, .. } if victim == id =>
                        self.session.send_death(killer as i16, ship.bounty.min(u16::MAX as u32) as u16),
                    Event::GreenTaken { ship: pilot, x, y, prize } if pilot == id =>
                        self.session.send_take_green(now, x, y, prize),
                    _ => {}
                }
            }
        }
        if self.session.position_due(now) {
            self.session.send_position_at(now, u64::from(self.tick_ms), &own_position(&world.ships[index], None));
        }
    }
}
