//! Browser bindings for the same game session the desktop client uses.
use oss_session::{SPECTATOR, Session, SessionConfig, SessionEvent};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct BrowserSession(Session);

#[wasm_bindgen]
impl BrowserSession {
    #[wasm_bindgen(constructor)]
    pub fn new(
        name: String,
        password: String,
        arena: String,
        now_ms: u32,
        seed: u32,
        server_ip: u32,
        source_port: u16,
    ) -> BrowserSession {
        let config = SessionConfig {
            name,
            password,
            arena: (!arena.is_empty()).then_some(arena),
            ship: SPECTATOR,
            ..SessionConfig::default()
        };
        // The first browser integration uses VIE directly, like Session::new.
        let mut session = Session::new(config, u64::from(now_ms), seed);
        session.set_local_endpoint(server_ip, source_port);
        Self(session)
    }

    pub fn receive(&mut self, now_ms: u32, datagram: &[u8]) {
        self.0.handle_datagram(u64::from(now_ms), datagram);
    }

    pub fn transmit(&mut self) -> Option<Vec<u8>> {
        self.0.poll_transmit()
    }

    pub fn state(&self) -> String {
        format!("{:?}", self.0.state())
    }

    pub fn update(&mut self, now_ms: u32) -> Option<String> {
        let mut error = None;
        while let Some(event) = self.0.poll_event() {
            match event {
                SessionEvent::MapNeeded { .. } => self.0.download_map(),
                SessionEvent::LoginFailed { message, zone_text, .. } => {
                    error = Some(zone_text.unwrap_or_else(|| message.into()));
                }
                SessionEvent::NewUserRequired { .. } => {
                    error = Some("Claim your pilot name on Foundation before logging in.".into());
                }
                _ => {}
            }
        }
        self.0.tick(u64::from(now_ms));
        self.0.flush(u64::from(now_ms));
        error
    }

    /// Interleaved x/y pixel coordinates of players with valid positions.
    pub fn positions(&self) -> Vec<f32> {
        self.0
            .players()
            .filter_map(|player| player.position)
            .filter(|position| position.x >= 0 && position.y >= 0)
            .flat_map(|position| [f32::from(position.x), f32::from(position.y)])
            .collect()
    }

    pub fn disconnect(&mut self, now_ms: u32) {
        self.0.disconnect();
        self.0.flush(u64::from(now_ms));
    }
}
