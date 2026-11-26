//! The screen between choosing a zone and flying in it: a warp through the
//! stars, faster as the way in gets on, under the arena's name, a word for what
//! is happening and a progress line that only moves forward.
//!
//! The browser page draws the same warp while the game downloads and keeps it
//! over the canvas until the arena is on screen (the game tells it where it has
//! got to, see [`crate::web::loading_stage`]); the numbers here and in the
//! page's `loading.js` are the same, so the two read as one scene.

use crate::game::online::{Online, Stage};
use crate::game::view::Canvas;
use crate::gfx::Renderer;

/// How many stars the tunnel holds.
const STARS: usize = 320;
/// The most dots one streak is drawn with.
const DOTS: usize = 64;

const BACKGROUND: [f32; 4] = [0.008, 0.016, 0.04, 1.0];
const INK: [f32; 4] = [0.95, 0.98, 1.0, 1.0];
const GLOW: [f32; 4] = [0.31, 0.83, 1.0, 1.0];
const MUTED: [f32; 4] = [0.72, 0.79, 0.88, 1.0];
const DIM: [f32; 4] = [0.45, 0.52, 0.6, 1.0];

/// How long each of the ship's preparations stays on screen, in seconds.
const PHRASE_EVERY: f32 = 2.2;

/// Each step's share of the progress line, and the ship's preparations the
/// line under it picks from while the step lasts (the browser page has the
/// same, in its `loading.js`).
fn share(stage: Stage) -> (f32, f32, &'static [&'static str]) {
    // In the browser the page's own step, downloading the game, has the
    // first 45% of the line.
    let page = if cfg!(target_arch = "wasm32") {
        0.45
    } else {
        0.0
    };
    let at = |f: f32| page + (1.0 - page) * f;
    match stage {
        Stage::Connect => (
            at(0.0),
            at(0.27),
            &[
                "Plotting a course",
                "Hailing the zone",
                "Aligning the antenna",
                "Calibrating the nav computer",
                "Locking onto the beacon",
            ],
        ),
        Stage::SignIn => (
            at(0.27),
            at(0.45),
            &[
                "Checking your pilot licence",
                "Clearing docking control",
                "Scanning your ID badge",
                "Confirming flight clearance",
            ],
        ),
        Stage::Enter => (
            at(0.45),
            at(0.6),
            &[
                "Testing the hyperdrive",
                "Opening the jump gate",
                "Spooling up the warp core",
                "Charging the thrusters",
            ],
        ),
        Stage::Download => (
            at(0.6),
            1.0,
            &[
                "Checking your engines",
                "Charting the arena",
                "Mapping the asteroid fields",
                "Downloading star charts",
                "Polishing the hull",
                "Arming the guns",
            ],
        ),
        Stage::Replay => (
            0.0,
            1.0,
            &["Rewinding the tapes", "Loading the flight recorder"],
        ),
    }
}

/// The progress line `dt` seconds on from `shown`: eased toward `target`, and
/// creeping by itself (faster the further it is from `cap`) where that is
/// further, but never past `cap` and never back.
fn move_line(shown: f32, target: f32, cap: f32, dt: f32) -> f32 {
    let eased = shown + (target - shown) * (dt * 4.0).min(1.0);
    let crept = shown + (0.0015 + 0.06 * (cap - shown).max(0.0)) * dt;
    eased.max(crept).min(cap).max(shown)
}

/// Where the line may creep to while `stage` lasts: halfway into the next
/// step's share, or nearly the end.
fn creep_cap(stage: Stage) -> f32 {
    let next = match stage {
        Stage::Connect => Some(Stage::SignIn),
        Stage::SignIn => Some(Stage::Enter),
        Stage::Enter => Some(Stage::Download),
        Stage::Download | Stage::Replay => None,
    };
    let (_, hi, _) = share(stage);
    next.map_or(0.985, |n| hi + (share(n).1 - hi) * 0.5)
}

#[derive(Debug, Clone, Copy)]
struct Star {
    /// Across the tunnel, -1 to 1.
    x: f32,
    y: f32,
    /// How far ahead, 1 the farthest.
    z: f32,
    color: [f32; 3],
}

/// The warp's state from frame to frame.
#[derive(Debug)]
pub struct Warp {
    stars: Vec<Star>,
    seed: u32,
    /// The progress line as shown, 0 to 1; it never goes back.
    shown: f32,
    stage: Option<Stage>,
    stage_at: f32,
    last: Option<f32>,
    /// The preparation on show, and when it came up.
    phrase: &'static str,
    phrase_at: f32,
}

impl Default for Warp {
    fn default() -> Self {
        let seed = web_time::SystemTime::now()
            .duration_since(web_time::UNIX_EPOCH)
            .map_or(1, |d| d.subsec_nanos() | 1);
        let mut warp = Self {
            stars: Vec::with_capacity(STARS),
            seed,
            shown: 0.0,
            stage: None,
            stage_at: 0.0,
            last: None,
            phrase: "",
            phrase_at: f32::NEG_INFINITY,
        };
        for _ in 0..STARS {
            let s = warp.star(false);
            warp.stars.push(s);
        }
        warp
    }
}

impl Warp {
    fn random(&mut self) -> f32 {
        // xorshift: the stars only need to look scattered
        let mut x = self.seed;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.seed = x;
        (x >> 8) as f32 / (1u32 << 24) as f32
    }

    fn star(&mut self, fresh: bool) -> Star {
        let (mut x, mut y);
        loop {
            x = self.random() * 2.0 - 1.0;
            y = self.random() * 2.0 - 1.0;
            if x * x + y * y >= 0.004 {
                break;
            }
        }
        let z = if fresh {
            1.0
        } else {
            0.05 + self.random() * 0.95
        };
        // pale blue to violet-blue, like the page's hsl(195..235, 90%, 80%)
        let hue = self.random();
        let color = [0.62 + 0.18 * hue, 0.82 - 0.12 * hue, 1.0];
        Star { x, y, z, color }
    }

    /// Moves the progress line on. It only moves forward and never stands
    /// still: a step's downloads move it by their bytes, other steps ease it
    /// on toward the end of their share, and underneath both it creeps on by
    /// itself, slower the closer it gets to a little way into the next step.
    fn advance(&mut self, net: &Online, t: f32, dt: f32) {
        if self.stage != Some(net.stage) {
            self.stage = Some(net.stage);
            self.stage_at = t;
            self.say(net.stage, t);
        }
        if t - self.phrase_at > PHRASE_EVERY {
            self.say(net.stage, t);
        }
        let (lo, hi, _) = share(net.stage);
        let cap = creep_cap(net.stage);
        let target = match net.progress.filter(|_| net.stage == Stage::Download) {
            Some(p) => lo + (hi - lo) * p.clamp(0.0, 1.0),
            None => lo + (hi - lo) * 0.9 * (1.0 - (-(t - self.stage_at) / 2.0).exp()),
        };
        self.shown = move_line(self.shown, target, cap, dt);
    }

    /// The next of the ship's preparations for `stage`, never the one just
    /// shown.
    fn say(&mut self, stage: Stage, t: f32) {
        let (_, _, pool) = share(stage);
        let mut next = self.phrase;
        while pool.len() > 1 && next == self.phrase {
            next = pool[(self.random() * pool.len() as f32) as usize % pool.len()];
        }
        if pool.len() == 1 {
            next = pool[0];
        }
        self.phrase = next;
        self.phrase_at = t;
    }

    /// Cruising at first, faster as the way in gets on.
    fn speed(&self) -> f32 {
        0.12 + 0.9 * self.shown * self.shown
    }

    /// Draws the screen at `t` seconds since it opened.
    pub fn draw(&mut self, c: &Canvas, r: &mut Renderer, net: &Online, t: f32) {
        let dt = self.last.map_or(0.0, |l| (t - l).clamp(0.0, 0.05));
        self.last = Some(t);
        self.advance(net, t, dt);
        let (vw, vh) = (c.view_w, c.view_h);
        c.rect(r, 0.0, 0.0, vw, vh, BACKGROUND);
        let (cx, cy) = (vw / 2.0, vh / 2.0);
        let reach = (vw * vw + vh * vh).sqrt() * 0.32;
        let v = self.speed();
        glow(c, r, cx, cy, reach * (0.5 + v * 0.4), v.min(1.0));
        self.streaks(c, r, (cx, cy), reach, v, dt);
        self.words(c, r, net, cx, cy, t);
    }

    fn streaks(
        &mut self,
        c: &Canvas,
        r: &mut Renderer,
        (cx, cy): (f32, f32),
        reach: f32,
        v: f32,
        dt: f32,
    ) {
        let trail = 0.012 + v * 0.07;
        let (vw, vh) = (c.view_w, c.view_h);
        for i in 0..self.stars.len() {
            let mut s = self.stars[i];
            s.z -= v * dt;
            let hx = cx + s.x / s.z.max(0.001) * reach;
            let hy = cy + s.y / s.z.max(0.001) * reach;
            if s.z <= 0.02 || hx < -40.0 || hy < -40.0 || hx > vw + 40.0 || hy > vh + 40.0 {
                self.stars[i] = self.star(true);
                continue;
            }
            self.stars[i] = s;
            let tz = (s.z + trail).min(1.2);
            let tx = cx + s.x / tz * reach;
            let ty = cy + s.y / tz * reach;
            let near = 1.0 - s.z;
            let size = (0.6 + near * 1.8).round().max(1.0);
            let alpha = (0.15 + near * 2.0).min(1.0);
            let bright = 0.75 + 0.25 * near;
            let (dx, dy) = (hx - tx, hy - ty);
            let n = ((dx.abs().max(dy.abs()) / size).ceil() as usize).clamp(1, DOTS);
            for d in 0..=n {
                let f = d as f32 / n as f32;
                // brightest at the head, fading along the tail
                let a = alpha * (0.15 + 0.85 * f);
                c.rect(
                    r,
                    (tx + dx * f - size / 2.0).floor(),
                    (ty + dy * f - size / 2.0).floor(),
                    size,
                    size,
                    [
                        s.color[0] * bright,
                        s.color[1] * bright,
                        s.color[2] * bright,
                        a,
                    ],
                );
            }
        }
    }

    /// The arena's name, the word for this step, the progress line, and how to
    /// give up.
    #[allow(clippy::too_many_arguments)]
    fn words(&self, c: &Canvas, r: &mut Renderer, net: &Online, cx: f32, cy: f32, t: f32) {
        let name = match (net.stage, net.arena_name()) {
            (Stage::Replay, _) => "replay".to_string(),
            (_, Some(arena)) => arena.to_string(),
            (_, None) => "public".to_string(),
        }
        .to_uppercase();
        // as big as fits: three times the font, two, or one
        let room = c.view_w - 24.0;
        let big = [3.0, 2.0, 1.0]
            .into_iter()
            .find(|k| name.len() as f32 * 9.0 * k <= room)
            .unwrap_or(1.0);
        let top = (cy - 12.0 * big / 2.0 - 10.0).floor();

        spaced(
            c,
            r,
            (cx, top - 18.0),
            1.0,
            3.0,
            [0.62, 0.86, 1.0, 0.85],
            "ENTERING",
        );
        // a halo behind the name: the same letters in blue, a pixel out each way
        let halo = [GLOW[0], GLOW[1], GLOW[2], 0.35];
        for (ox, oy) in [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0)] {
            spaced(c, r, (cx + ox, top + oy), big, big, halo, &name);
        }
        spaced(c, r, (cx, top), big, big, INK, &name);

        // the progress line: blue into cyan into white at the head
        let lw = (c.view_w * 0.5).clamp(80.0, 240.0).floor();
        let lx = (cx - lw / 2.0).floor();
        let ly = top + 12.0 * big + 16.0;
        c.rect(r, lx, ly, lw, 1.0, [1.0, 1.0, 1.0, 0.12]);
        let fill = (lw * self.shown).floor();
        if fill > 0.0 {
            let parts = [
                (0.0, [0.16, 0.48, 1.0]),
                (0.6, [0.44, 0.89, 0.98]),
                (0.92, [1.0, 1.0, 1.0]),
            ];
            for (i, (from, col)) in parts.iter().enumerate() {
                let to = parts.get(i + 1).map_or(1.0, |n| n.0);
                let a = (fill * from).floor();
                let b = (fill * to).floor();
                if b > a {
                    c.rect(r, lx + a, ly, b - a, 1.0, [col[0], col[1], col[2], 1.0]);
                }
            }
            c.rect(
                r,
                lx + fill - 2.0,
                ly - 1.0,
                3.0,
                3.0,
                [0.7, 0.95, 1.0, 0.5],
            );
        }

        // what the ship is getting ready, faded in as it changes
        let fade = ((t - self.phrase_at) / 0.25).clamp(0.0, 1.0);
        spaced(
            c,
            r,
            (cx, ly + 12.0),
            1.0,
            2.0,
            [MUTED[0], MUTED[1], MUTED[2], fade],
            &self.phrase.to_uppercase(),
        );
        let esc = "Esc to cancel";
        c.text_tinted(
            r,
            (cx - esc.len() as f32 * 4.0).floor(),
            (c.view_h - 24.0).floor(),
            DIM,
            esc,
        );
    }
}

/// The light ahead: discs, faint and fainter outward, stacked so together they
/// read as a soft round glow, brighter the faster the ship goes.
fn glow(c: &Canvas, r: &mut Renderer, cx: f32, cy: f32, radius: f32, v: f32) {
    // Many faint discs, each a little smaller and whiter: their edges are too
    // faint to see one by one, so what shows is a falloff rather than rings.
    const LAYERS: usize = 18;
    for i in 0..LAYERS {
        let f = i as f32 / (LAYERS - 1) as f32;
        let k = 1.0 - 0.92 * f;
        let alpha = (0.006 + 0.012 * f) * (0.5 + v);
        let col = [0.16 + 0.6 * f * f, 0.35 + 0.57 * f, 1.0];
        disc(c, r, cx, cy, radius * k, [col[0], col[1], col[2], alpha]);
    }
}

/// A filled disc of thin bands.
fn disc(c: &Canvas, r: &mut Renderer, cx: f32, cy: f32, radius: f32, color: [f32; 4]) {
    let band = (radius / 40.0).max(2.0).floor();
    let mut y = -radius;
    while y < radius {
        let mid = y + band / 2.0;
        let half = (radius * radius - mid * mid).max(0.0).sqrt();
        c.rect(
            r,
            (cx - half).floor(),
            (cy + y).floor(),
            (half * 2.0).floor(),
            band,
            color,
        );
        y += band;
    }
}

/// Text centred on `at.0` with its top at `at.1`, its letters `scale` times
/// the font's size with `gap` pixels between them.
fn spaced(
    c: &Canvas,
    r: &mut Renderer,
    at: (f32, f32),
    scale: f32,
    gap: f32,
    rgba: [f32; 4],
    s: &str,
) {
    let n = s.chars().count() as f32;
    let width = n * 8.0 * scale + (n - 1.0).max(0.0) * gap;
    let x = (at.0 - width / 2.0).floor();
    for (i, ch) in s.chars().enumerate() {
        c.glyph(
            r,
            x + i as f32 * (8.0 * scale + gap),
            at.1.floor(),
            scale,
            rgba,
            ch,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_steps_share_the_line_in_order_and_fill_it() {
        let steps = [Stage::Connect, Stage::SignIn, Stage::Enter, Stage::Download];
        let mut end = share(steps[0]).0;
        for s in steps {
            let (lo, hi, _) = share(s);
            assert_eq!(lo, end, "{s:?} starts where the last ended");
            assert!(hi > lo);
            end = hi;
        }
        assert_eq!(end, 1.0);
    }

    #[test]
    fn the_line_creeps_on_but_not_past_its_cap_and_phrases_change() {
        // Nothing reports for a long while: the line still moves every frame,
        // into the next step's share, and stops short of its cap.
        let (lo, hi, _) = share(Stage::Connect);
        let cap = creep_cap(Stage::Connect);
        let mut shown = lo;
        for _ in 0..4000 {
            let next = move_line(shown, lo, cap, 0.05);
            assert!(next > shown || next == cap, "stood still at {shown}");
            assert!(next <= cap);
            shown = next;
        }
        assert!(shown > hi, "it crept into the next share");
        // A real figure ahead of the creep pulls it on faster, but not past the cap.
        assert!(move_line(lo, hi, cap, 0.05) > move_line(lo, lo, cap, 0.05));
        assert_eq!(move_line(cap, 1.0, cap, 0.05), cap);

        let mut warp = Warp::default();
        let (_, _, pool) = share(Stage::Download);
        warp.say(Stage::Download, 0.0);
        let first = warp.phrase;
        assert!(pool.contains(&first));
        warp.say(Stage::Download, 1.0);
        assert_ne!(warp.phrase, first);
    }

    #[test]
    fn stars_start_inside_the_tunnel_and_off_its_axis() {
        let warp = Warp::default();
        assert_eq!(warp.stars.len(), STARS);
        for s in &warp.stars {
            assert!((-1.0..=1.0).contains(&s.x) && (-1.0..=1.0).contains(&s.y));
            assert!(s.x * s.x + s.y * s.y >= 0.004);
            assert!((0.05..=1.0).contains(&s.z));
        }
    }
}
