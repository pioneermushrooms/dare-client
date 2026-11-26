//! A flying bot's position reports, sent the way the game client sends its own
//! (`oss-client`'s `sandbox.rs`): each dated to the moment of the simulation tick it
//! describes, not to when the bot got round to sending it, and an extra one the
//! tick thrust, afterburners or turning start or stop (at most one per
//! [`oss_session::sim::MOTION_SEND_GAP`], the game client's rule, shared with it).
//! Without these other players see a bot drift and then snap: every client carries
//! a ship on at its last velocity until the next update says otherwise.

use oss_session::sim::{Motion, MotionReports, own_position, weapon_for};
use oss_session::{OwnPosition, Session};
use oss_sim::{Event, Ship, ShipInput};

/// Where position reports go: the session, or a recorder in tests.
pub trait Outbox {
    /// Whether a regular, timed update is due.
    fn position_due(&self, now_ms: u64) -> bool;
    /// Sends `pos` as it was at `state_ms`.
    fn send_position_at(&mut self, now_ms: u64, state_ms: u64, pos: &OwnPosition);
}

impl Outbox for Session {
    fn position_due(&self, now_ms: u64) -> bool {
        Session::position_due(self, now_ms)
    }

    fn send_position_at(&mut self, now_ms: u64, state_ms: u64, pos: &OwnPosition) {
        Session::send_position_at(self, now_ms, state_ms, pos);
    }
}

/// The position reports of one bot's ship.
#[derive(Debug, Default)]
pub struct Reports {
    motion: MotionReports,
}

impl Reports {
    /// After simulating `tick`, which stands for session time `tick_ms`: one report
    /// per weapon the ship fired, then one for a change of motion, unless a
    /// regular update is about to go out with the change in it anyway. `ship` is
    /// the bot's own ship (`None` while spectating), steered this tick by `input`.
    #[allow(clippy::too_many_arguments)]
    pub fn tick(
        &mut self,
        out: &mut impl Outbox,
        now: u64,
        tick: u32,
        tick_ms: u64,
        ship: Option<&Ship>,
        input: &ShipInput,
        events: &[Event],
    ) {
        let Some(ship) = ship else {
            self.motion.reset();
            return;
        };
        for ev in events {
            if let Some(w) = weapon_for(ev, ship) {
                // Each shot goes out at once, from where and when it was fired.
                out.send_position_at(now, tick_ms, &own_position(ship, Some(w)));
            }
        }
        if !ship.is_alive() {
            self.motion.reset();
            return;
        }
        let motion = Motion::of(ship, input);
        if !self.motion.due(tick, motion) {
            return;
        }
        if !out.position_due(now) {
            out.send_position_at(now, tick_ms, &own_position(ship, None));
        }
        self.motion.sent(tick, motion);
    }

    /// After the last tick simulated (standing for `tick_ms`): the regular update,
    /// if one is due.
    pub fn regular(&mut self, out: &mut impl Outbox, now: u64, tick_ms: u64, ship: Option<&Ship>) {
        if let Some(ship) = ship
            && out.position_due(now)
        {
            out.send_position_at(now, tick_ms, &own_position(ship, None));
        }
    }
}

/// Runs the 10 ms ticks due between `*last_ms` and `now`, handing `step` each
/// tick's own moment, and leaves `*last_ms` at the last one. Far behind (the
/// machine was asleep, say), it skips ahead rather than racing.
pub fn run_ticks(last_ms: &mut u64, now: u64, mut step: impl FnMut(u64)) {
    if now.saturating_sub(*last_ms) > 2000 {
        *last_ms = now - 10;
    }
    while *last_ms + 10 <= now {
        *last_ms += 10;
        step(*last_ms);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oss_sim::{ArenaSettings, TileMap, World};

    /// Records every report (with its stamp) and says a regular update is due
    /// only when told to.
    #[derive(Default)]
    struct Recorder {
        due: bool,
        sent: Vec<(u64, u64)>,
    }

    impl Outbox for Recorder {
        fn position_due(&self, _now: u64) -> bool {
            self.due
        }
        fn send_position_at(&mut self, now: u64, state_ms: u64, _pos: &OwnPosition) {
            self.sent.push((now, state_ms));
        }
    }

    fn world() -> (World, usize) {
        let mut w = World::new(TileMap::empty(), ArenaSettings::sandbox(), 1);
        let me = w.add_ship(1, 0, 1);
        (w, me)
    }

    fn thrust() -> ShipInput {
        ShipInput {
            forward: true,
            ..Default::default()
        }
    }

    /// Steps the world one tick with `input` and lets `reports` see it.
    fn step(
        w: &mut World,
        me: usize,
        reports: &mut Reports,
        out: &mut Recorder,
        now: u64,
        tick_ms: u64,
        input: ShipInput,
    ) {
        let mut inputs = vec![ShipInput::default(); w.ships.len()];
        inputs[me] = input;
        let events = w.step(&inputs);
        reports.tick(
            out,
            now,
            w.tick,
            tick_ms,
            Some(&w.ships[me]),
            &input,
            &events,
        );
    }

    #[test]
    fn motion_change_sends_one_extra_report() {
        let (mut w, me) = world();
        let mut reports = Reports::default();
        let mut out = Recorder::default();
        // First tick alive: nothing extra (the regular update covers it).
        step(
            &mut w,
            me,
            &mut reports,
            &mut out,
            1000,
            1000,
            ShipInput::default(),
        );
        assert!(out.sent.is_empty());
        // Thrust comes on at tick t: one extra report.
        step(&mut w, me, &mut reports, &mut out, 1010, 1010, thrust());
        assert_eq!(out.sent.len(), 1);
        // A turn starts 10 ms later, inside the 20 ms gap: no second report yet.
        let turn = ShipInput {
            left: true,
            ..thrust()
        };
        step(&mut w, me, &mut reports, &mut out, 1020, 1020, turn);
        assert_eq!(out.sent.len(), 1, "two changes within 20 ms send one extra");
        // Once the gap is over the change, still standing, goes out.
        step(&mut w, me, &mut reports, &mut out, 1030, 1030, turn);
        assert_eq!(out.sent.len(), 2);
        // No change, no report.
        step(&mut w, me, &mut reports, &mut out, 1040, 1040, turn);
        assert_eq!(out.sent.len(), 2);
        // A change while a regular update is due anyway rides in that one.
        out.due = true;
        step(
            &mut w,
            me,
            &mut reports,
            &mut out,
            1050,
            1050,
            ShipInput::default(),
        );
        assert_eq!(out.sent.len(), 2);
    }

    #[test]
    fn reports_are_dated_to_their_tick() {
        let (mut w, me) = world();
        let mut reports = Reports::default();
        let mut out = Recorder::default();
        // The bot's loop wakes at 1035 with ticks for 1010, 1020 and 1030 to run;
        // thrust starts on the 1010 tick.
        let (mut last, now) = (1000, 1035);
        step(
            &mut w,
            me,
            &mut reports,
            &mut out,
            1000,
            1000,
            ShipInput::default(),
        );
        let mut stamps = Vec::new();
        run_ticks(&mut last, now, |tick_ms| {
            stamps.push(tick_ms);
            step(&mut w, me, &mut reports, &mut out, now, tick_ms, thrust());
        });
        assert_eq!(stamps, [1010, 1020, 1030]);
        assert_eq!(last, 1030);
        assert_eq!(out.sent, [(1035, 1010)], "dated to its tick, not to now");
        // The regular update carries the last tick's moment.
        out.due = true;
        reports.regular(&mut out, now, last, Some(&w.ships[me]));
        assert_eq!(out.sent[1], (1035, 1030));
    }
}
