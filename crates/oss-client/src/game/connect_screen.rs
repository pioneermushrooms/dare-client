//! The screen between choosing a zone and flying in it: a Continuum panel over
//! drifting stars, with the ship turning beside the steps of the way in
//! (connect, sign in, enter the arena, load the map and graphics), a bar for the
//! step under way and a line saying what it is doing.
//!
//! In the browser the page shows the same panel's steps while the game itself
//! downloads, under the same names ([`Stage::label`]), with one step of its own
//! in front, so the hand-over from the page to this screen reads as one
//! sequence rather than two.

use crate::game::art::Art;
use crate::game::online::{Online, Stage};
use crate::game::view::{Canvas, color, ui_color};
use crate::gfx::{Renderer, StarParams};

/// The step the browser page did before the game could start: downloading it.
/// Shown done, first, so the list is the one the page showed.
#[cfg(target_arch = "wasm32")]
const PAGE_STEP: Option<&str> = Some("Download the game");
#[cfg(not(target_arch = "wasm32"))]
const PAGE_STEP: Option<&str> = None;

/// The inside of the panel, in game pixels.
const WIDTH: f32 = 304.0;
const ROW: f32 = 15.0;

const DONE: [f32; 4] = [0.33, 0.85, 0.42, 1.0];
const NOW: [f32; 4] = [1.0, 0.86, 0.3, 1.0];
const LATER: [f32; 4] = [0.42, 0.45, 0.44, 1.0];
const TRACK: [f32; 4] = [0.02, 0.05, 0.03, 1.0];

/// Stars for behind the panel: drifting slowly, as if the ship were coasting.
pub fn stars(t: f32, scale: u32) -> StarParams {
    StarParams {
        camera: [t * 18.0, t * 7.0],
        scale: scale.max(1) as f32,
    }
}

/// How many times over the panel is drawn on a screen `size` physical pixels
/// at `scale`: twice where the game's view is big enough to leave the panel
/// small in the middle of it. The browser page works this out the same way, so
/// its loader is the panel's size.
pub fn zoom(size: (u32, u32), scale: u32) -> u32 {
    let scale = scale.max(1);
    if size.0 / scale >= 1100 && size.1 / scale >= 640 {
        2
    } else {
        1
    }
}

/// Draws the screen at `t` seconds since it opened.
pub fn draw(c: &Canvas, r: &mut Renderer, art: &Art, net: &Online, t: f32) {
    let steps: Vec<(&str, Mark)> = steps(net.stage);
    let width = WIDTH.min(c.view_w - 16.0).floor();
    let body = steps.len() as f32 * ROW;
    // title, rule, steps, bar and its line
    let height = 12.0 + 9.0 + 8.0 + body + 8.0 + 6.0 + 6.0 + 12.0 + 4.0;
    let x = ((c.view_w - width) / 2.0).floor();
    let y = ((c.view_h - height) / 2.0 - 8.0).floor().max(4.0);
    c.panel(r, x, y, width, height);

    // Title: where to.
    let (lead, place) = match (net.stage, net.arena_name()) {
        (Stage::Replay, _) => ("Watching ", "a recording"),
        (_, Some(arena)) => ("Entering ", arena),
        (_, None) => ("Entering ", "a public arena"),
    };
    let tx = x + 6.0;
    c.text(r, tx, y + 3.0, color::WHITE, lead);
    let room = (((width - 12.0) / 8.0) as usize).saturating_sub(lead.len());
    c.text(
        r,
        tx + lead.len() as f32 * 8.0,
        y + 3.0,
        color::YELLOW,
        &fit(place, room),
    );
    c.separator(r, x + 1.0, y + 18.0, width - 2.0);

    // The ship, turning, in the panel's left column.
    let top = y + 26.0;
    let column = 56.0;
    let kind = if net.ship() < 8 { net.ship() } else { 0 };
    let ship = art.ship(kind, ((t * 12.0) as u32 % 40) as u8);
    if ship.w > 0 {
        let (sw, sh) = (ship.w as f32, ship.h as f32);
        let cx = x + 4.0 + column / 2.0;
        let cy = top + body / 2.0 - 2.0;
        c.ui(
            r,
            ship,
            (cx - sw / 2.0).floor(),
            (cy - sh / 2.0).floor(),
            None,
            [1.0; 4],
        );
    }

    // The steps.
    let lx = x + 4.0 + column + 4.0;
    for (i, (label, mark)) in steps.iter().enumerate() {
        let ry = top + i as f32 * ROW;
        draw_mark(c, r, lx, ry + 3.0, *mark, t);
        let text = match mark {
            Mark::Now => format!("{label}{}", &"..."[..((t * 2.5) as usize % 4).min(3)]),
            _ => (*label).to_string(),
        };
        let tint = match mark {
            Mark::Done => DONE,
            Mark::Now => NOW,
            Mark::Later => LATER,
        };
        c.text_tinted(r, lx + 12.0, ry, tint, &text);
    }

    // The bar for this step: how much has arrived, or a sweep while that isn't
    // something that can be counted.
    let by = top + body + 8.0;
    let (bx, bw) = (x + 6.0, width - 12.0);
    c.color_rect(r, ui_color::BORDER3, bx - 1.0, by - 1.0, bw + 2.0, 8.0);
    c.rect(r, bx, by, bw, 6.0, TRACK);
    match net.progress.filter(|_| net.stage == Stage::Download) {
        Some(done) => {
            let fill = (bw * done.clamp(0.0, 1.0)).floor();
            c.rect(r, bx, by, fill, 6.0, DONE);
            c.rect(r, bx, by, fill, 1.0, [0.6, 1.0, 0.65, 1.0]);
        }
        None => {
            let seg = (bw / 4.0).floor();
            let phase = (t / 1.6).fract();
            let head = -seg + phase * (bw + seg);
            let from = head.max(0.0);
            let to = (head + seg).min(bw);
            if to > from {
                c.rect(r, bx + from.floor(), by, (to - from).floor(), 6.0, NOW);
            }
        }
    }

    // What this step is doing, and how far through it is.
    let ly = by + 12.0;
    let percent = net
        .progress
        .filter(|_| net.stage == Stage::Download)
        .map(|p| format!("{}%", (p.clamp(0.0, 1.0) * 100.0).floor() as u32));
    let right = percent.as_deref().unwrap_or("");
    let room = ((bw - right.len() as f32 * 8.0).max(0.0) / 8.0) as usize;
    let room = if right.is_empty() {
        room
    } else {
        room.saturating_sub(1)
    };
    c.text(r, bx, ly, color::WHITE, &fit(&net.detail, room));
    if !right.is_empty() {
        c.text_right(r, bx + bw, ly, color::GREEN, right);
    }

    c.text_tinted(
        r,
        ((c.view_w - 13.0 * 8.0) / 2.0).floor(),
        y + height + 10.0,
        LATER,
        "Esc to cancel",
    );
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mark {
    Done,
    Now,
    Later,
}

/// The list of steps, each marked against the one under way.
fn steps(stage: Stage) -> Vec<(&'static str, Mark)> {
    if stage == Stage::Replay {
        return vec![(stage.label(), Mark::Now)];
    }
    let at = Stage::STEPS.iter().position(|s| *s == stage).unwrap_or(0);
    PAGE_STEP
        .map(|label| (label, Mark::Done))
        .into_iter()
        .chain(Stage::STEPS.iter().enumerate().map(|(i, s)| {
            let mark = match i.cmp(&at) {
                std::cmp::Ordering::Less => Mark::Done,
                std::cmp::Ordering::Equal => Mark::Now,
                std::cmp::Ordering::Greater => Mark::Later,
            };
            (s.label(), mark)
        }))
        .collect()
}

/// A step's marker: a filled square once done, a pulsing one while under way,
/// an empty frame until then.
fn draw_mark(c: &Canvas, r: &mut Renderer, x: f32, y: f32, mark: Mark, t: f32) {
    match mark {
        Mark::Done => c.rect(r, x, y, 6.0, 6.0, DONE),
        Mark::Now => {
            let glow = 0.55 + 0.45 * (t * 5.0).sin().abs();
            c.rect(
                r,
                x - 1.0,
                y - 1.0,
                8.0,
                8.0,
                [NOW[0], NOW[1], NOW[2], 0.35],
            );
            c.rect(
                r,
                x,
                y,
                6.0,
                6.0,
                [NOW[0] * glow, NOW[1] * glow, NOW[2] * glow, 1.0],
            );
        }
        Mark::Later => {
            c.rect(r, x, y, 6.0, 1.0, LATER);
            c.rect(r, x, y + 5.0, 6.0, 1.0, LATER);
            c.rect(r, x, y, 1.0, 6.0, LATER);
            c.rect(r, x + 5.0, y, 1.0, 6.0, LATER);
        }
    }
}

/// `s` cut to `room` characters, with "..." where it was cut.
fn fit(s: &str, room: usize) -> String {
    let n = s.chars().count();
    if n <= room {
        return s.to_string();
    }
    if room <= 3 {
        return s.chars().take(room).collect();
    }
    let mut out: String = s.chars().take(room - 3).collect();
    out.push_str("...");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_before_the_one_under_way_are_done() {
        let marks: Vec<Mark> = steps(Stage::Enter).into_iter().map(|(_, m)| m).collect();
        let own = &marks[marks.len() - 4..];
        assert_eq!(own, [Mark::Done, Mark::Done, Mark::Now, Mark::Later]);
        assert_eq!(steps(Stage::Replay).len(), 1);
    }

    #[test]
    fn the_panel_doubles_only_where_there_is_room() {
        assert_eq!(zoom((2560, 1600), 2), 2);
        assert_eq!(zoom((1280, 800), 1), 2);
        assert_eq!(zoom((1280, 720), 2), 1);
        assert_eq!(zoom((800, 600), 1), 1);
    }

    #[test]
    fn long_lines_are_cut_with_dots() {
        assert_eq!(fit("tactical.lvl", 20), "tactical.lvl");
        assert_eq!(fit("a-very-long-file-name.lvz", 10), "a-very-...");
        assert_eq!(fit("abcdef", 2), "ab");
    }
}
