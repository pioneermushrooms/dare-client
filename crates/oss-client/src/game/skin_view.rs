//! Where a skin goes in the window, what the pointer is over, and drawing it.
//!
//! A skin is drawn at a whole-number scale, centred, so its pixel art stays crisp
//! at any window size. Everything here works in *skin pixels* -- the coordinates
//! the skin file uses -- and converts to physical window pixels only to draw.

use crate::game::art::Art;
use crate::game::skin::{Frame, HotSpot, Skin, SkinLayout, SkinRect, TICK_MS, step_at};
use crate::gfx::{Region, Renderer, TextureId};
use oss_session::directory::ZoneInfo;

/// How much of the window a skin may cover.
const FILL: f32 = 0.92;
/// Line pitch of the zone list and the profile text, as the skin format fixes them.
pub const ZONE_PITCH: u32 = 14;
pub const PROFILE_PITCH: u32 = 16;
/// Profile text lines are at most this many characters.
pub const PROFILE_CHARS: usize = 23;

/// The largest whole-number scale at which `main` fits in the window, at least 1.
pub fn skin_scale(window: (u32, u32), main: (u32, u32)) -> u32 {
    let fit_w = window.0 as f32 * FILL / main.0.max(1) as f32;
    let fit_h = window.1 as f32 * FILL / main.1.max(1) as f32;
    (fit_w.min(fit_h).floor() as u32).max(1)
}

/// Where the skin's top-left corner goes, in physical pixels, to centre it.
pub fn skin_origin(window: (u32, u32), main: (u32, u32), scale: u32) -> (f32, f32) {
    let w = (main.0 * scale) as f32;
    let h = (main.1 * scale) as f32;
    (
        ((window.0 as f32 - w) / 2.0).floor(),
        ((window.1 as f32 - h) / 2.0).floor(),
    )
}

/// The skin pixel under a physical window position.
pub fn to_skin(physical: (f32, f32), origin: (f32, f32), scale: u32) -> (i32, i32) {
    let s = scale.max(1) as f32;
    (
        ((physical.0 - origin.0) / s).floor() as i32,
        ((physical.1 - origin.1) / s).floor() as i32,
    )
}

fn holds(r: SkinRect, x: i32, y: i32) -> bool {
    x >= 0 && y >= 0 && r.holds(x as u32, y as u32)
}

/// What the pointer is over: a button or ship before a text area, a text area
/// before the box around it. Never `main` or a skin's own decorative regions.
pub fn hotspot_at(layout: &SkinLayout, x: i32, y: i32) -> Option<HotSpot> {
    let tier = |h: &HotSpot| match h {
        HotSpot::Main | HotSpot::Other(_) | HotSpot::InfoBox => None,
        HotSpot::ProfBox | HotSpot::ShipBox | HotSpot::ZoneBox => Some(2),
        HotSpot::ProfText | HotSpot::ZoneText => Some(1),
        _ => Some(0),
    };
    layout
        .spots
        .iter()
        .filter(|(_, r)| holds(r.rect, x, y))
        .filter_map(|(h, _)| tier(h).map(|t| (t, h)))
        .min_by_key(|(t, _)| *t)
        .map(|(_, h)| h.clone())
}

/// How many zone lines fit in `zonetext`.
pub fn zone_rows(layout: &SkinLayout) -> usize {
    layout
        .first_rect(&HotSpot::ZoneText)
        .map_or(0, |r| (r.h / ZONE_PITCH) as usize)
}

/// The zone under the pointer, as an index into the whole list.
pub fn zone_row_at(
    layout: &SkinLayout,
    x: i32,
    y: i32,
    scroll: usize,
    count: usize,
) -> Option<usize> {
    let r = layout.first_rect(&HotSpot::ZoneText)?;
    if !holds(r, x, y) {
        return None;
    }
    let row = ((y as u32 - r.y) / ZONE_PITCH) as usize;
    (row < zone_rows(layout))
        .then_some(scroll + row)
        .filter(|&i| i < count)
}

/// One zone-list line `chars` wide: ping (3), the name, players (3).
pub fn zone_line(name: &str, players: u32, ping_ms: Option<u32>, chars: usize) -> String {
    let field = chars.saturating_sub(8);
    let ping = ping_ms.map_or("---".to_string(), |p| format!("{:>3}", p.min(999)));
    let name: String = name.chars().take(field).collect();
    format!("{ping} {name:<field$} {:>3}", players.min(999))
}

/// Splits `text` into lines of at most `width` characters, at spaces where it can.
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let mut word: String = word.to_string();
        while word.chars().count() > width {
            if !line.is_empty() {
                lines.push(std::mem::take(&mut line));
            }
            let rest = word.split_off(
                word.char_indices()
                    .nth(width)
                    .map_or(word.len(), |(i, _)| i),
            );
            lines.push(word);
            word = rest;
        }
        let need = line.chars().count() + usize::from(!line.is_empty()) + word.chars().count();
        if need > width && !line.is_empty() {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(&word);
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

/// Whether a ship or spectate slot is somewhere the client should draw its own
/// sprite: a real area inside the main region. Skins that draw their own ship
/// art (in ALT overlays) give a dummy first rectangle to turn the sprite off.
pub fn sprite_slot(main: SkinRect, slot: SkinRect) -> bool {
    slot.w >= 16
        && slot.h >= 16
        && slot.x >= main.x
        && slot.y >= main.y
        && slot.x + slot.w <= main.x + main.w
        && slot.y + slot.h <= main.y + main.h
}

/// The skin's bitmap on the GPU.
pub struct SkinTexture {
    pub id: TextureId,
}

impl SkinTexture {
    pub fn upload(r: &mut Renderer, skin: &Skin) -> SkinTexture {
        let (w, h) = (skin.image.width, skin.image.height);
        SkinTexture {
            id: r.create_texture(w, h, &skin.image.rgba),
        }
    }

    pub fn free(self, r: &mut Renderer) {
        r.free_texture(self.id);
    }
}

/// What the screen shows this frame.
pub struct SkinState<'a> {
    pub hovered: Option<HotSpot>,
    pub focused: Option<HotSpot>,
    /// The selected ship, 0-7, or the spectator value.
    pub ship: u8,
    /// Animation ticks since the screen opened.
    pub tick: u64,
    pub profile_lines: [&'a str; 4],
    pub zones: &'a [ZoneInfo],
    pub selected: Option<usize>,
    pub scroll: usize,
    pub ping_graph: bool,
    pub info: &'a str,
}

/// Ticks elapsed after `ms` milliseconds.
pub fn tick_at(ms: u128) -> u64 {
    (ms / u128::from(TICK_MS)) as u64
}

/// Draws the skin, its overlays, ships and text.
#[allow(clippy::too_many_arguments)]
pub fn draw_skin(
    r: &mut Renderer,
    tex: &SkinTexture,
    art: &Art,
    atlas: TextureId,
    skin: &Skin,
    st: &SkinState,
    window: (u32, u32),
) {
    let l = &skin.layout;
    let main = (l.main.w, l.main.h);
    let s = skin_scale(window, main);
    let o = skin_origin(window, main, s);
    let sf = s as f32;
    let at = |x: u32, y: u32| (o.0 + x as f32 * sf, o.1 + y as f32 * sf);
    let white = [1.0; 4];

    let (mx, my) = at(0, 0);
    r.quad(
        tex.id,
        Region {
            x: l.main.x,
            y: l.main.y,
            w: l.main.w,
            h: l.main.h,
        },
        mx,
        my,
        main.0 as f32 * sf,
        main.1 as f32 * sf,
        white,
    );

    let rows = zone_rows(l);
    let can_up = st.scroll > 0;
    let can_down = st.scroll + rows < st.zones.len();
    for (spot, region) in &l.spots {
        let lit = st.hovered.as_ref() == Some(spot)
            || st.focused.as_ref() == Some(spot)
            || match spot {
                HotSpot::Ship(n) => st.ship == n - 1,
                HotSpot::Spectate => st.ship == oss_session::SPECTATOR,
                _ => false,
            };
        let steps = match spot {
            // Drawn only when there is somewhere to scroll; ALT stands in for a
            // missing NORM so older skins still show the arrow.
            HotSpot::ZoneUp | HotSpot::ZoneDown => {
                let able = if *spot == HotSpot::ZoneUp {
                    can_up
                } else {
                    can_down
                };
                if !able {
                    continue;
                }
                if (lit && !region.alt.is_empty()) || region.norm.is_empty() {
                    &region.alt
                } else {
                    &region.norm
                }
            }
            // There is only ever one profile until profiles arrive.
            HotSpot::PrevProf | HotSpot::NextProf => continue,
            _ if lit && !region.alt.is_empty() => &region.alt,
            _ => &region.norm,
        };
        if let Some(Frame::At(fx, fy)) = step_at(steps, st.tick) {
            let (x, y) = at(region.rect.x, region.rect.y);
            r.quad(
                tex.id,
                Region {
                    x: fx,
                    y: fy,
                    w: region.rect.w,
                    h: region.rect.h,
                },
                x,
                y,
                region.rect.w as f32 * sf,
                region.rect.h as f32 * sf,
                white,
            );
        }
    }

    // Ships, centred in each spot's first rectangle.
    for k in 1..=8u8 {
        if let Some(b) = l
            .first_rect(&HotSpot::Ship(k))
            .filter(|b| sprite_slot(l.main, *b))
        {
            let sprite = art.ship(k - 1, 0);
            let (x, y) = at(b.x, b.y);
            let (w, h) = (sprite.w as f32 * sf, sprite.h as f32 * sf);
            r.quad(
                atlas,
                sprite,
                x + ((b.w as f32 * sf - w) / 2.0).floor(),
                y + ((b.h as f32 * sf - h) / 2.0).floor(),
                w,
                h,
                white,
            );
        }
    }
    let text = |r: &mut Renderer, x: u32, y: u32, rgb: [u8; 3], s_: &str| {
        let (px, py) = at(x, y);
        draw_text(r, art, atlas, px, py, sf, rgb, s_);
    };
    if let Some(b) = l
        .first_rect(&HotSpot::Spectate)
        .filter(|b| sprite_slot(l.main, *b))
    {
        let label = "SPEC";
        let w = label.len() as u32 * 8;
        text(
            r,
            b.x + b.w.saturating_sub(w) / 2,
            b.y + b.h.saturating_sub(12) / 2,
            [220, 220, 220],
            label,
        );
    }

    // Profile text: four lines, 16 pixels apart.
    if let Some(b) = l.first_rect(&HotSpot::ProfText) {
        let fallback = [
            [34, 211, 238],
            [240, 240, 240],
            [250, 204, 21],
            [200, 200, 200],
        ];
        for (i, line) in st.profile_lines.iter().enumerate() {
            let y = b.y + 2 + i as u32 * PROFILE_PITCH;
            if y + 12 > b.y + b.h {
                break;
            }
            let chars = ((b.w / 8) as usize).min(PROFILE_CHARS);
            let shown: String = line.chars().take(chars).collect();
            text(
                r,
                b.x + 2,
                y,
                l.colors.profile[i].unwrap_or(fallback[i]),
                &shown,
            );
        }
    }

    // The zone list.
    if let Some(b) = l.first_rect(&HotSpot::ZoneText) {
        let chars = (b.w / 8) as usize;
        let sel = l.colors.zone_sel[0].unwrap_or([40, 70, 110]);
        for (row, i) in (st.scroll..st.zones.len()).take(rows).enumerate() {
            let z = &st.zones[i];
            let y = b.y + row as u32 * ZONE_PITCH;
            if st.selected == Some(i) {
                fill(r, art, atlas, at(b.x, y), (b.w as f32 * sf, 13.0 * sf), sel);
            }
            let band = match z.ping_ms {
                Some(p) if p <= 100 => 0,
                Some(p) if p <= 250 => 1,
                Some(_) => 2,
                None => 2,
            };
            let fallback = [[74, 222, 128], [250, 204, 21], [248, 113, 113]];
            let col = l.colors.zone[band].unwrap_or(fallback[band]);
            let line = zone_line(&z.name, z.players, z.ping_ms, chars);
            if st.ping_graph {
                let len = z.ping_ms.map_or(0, |p| (p.min(500) * 22 / 500).max(1));
                fill(
                    r,
                    art,
                    atlas,
                    at(b.x + 1, y + 3),
                    (len as f32 * sf, 6.0 * sf),
                    col,
                );
                text(r, b.x + 32, y + 1, [235, 235, 235], &line[4..]);
            } else {
                text(r, b.x, y + 1, col, &line[..3]);
                text(
                    r,
                    b.x + 32,
                    y + 1,
                    [235, 235, 235],
                    &line[4..line.len() - 4],
                );
                text(
                    r,
                    b.x + (chars as u32 - 3) * 8,
                    y + 1,
                    col,
                    &line[line.len() - 3..],
                );
            }
        }
    }

    // The info box: whatever the pointer is over, else the screen's status.
    if let Some(b) = l.first_rect(&HotSpot::InfoBox) {
        let width = (b.w.saturating_sub(4) / 8) as usize;
        for (i, line) in wrap(st.info, width).iter().enumerate() {
            let y = b.y + 2 + i as u32 * 12;
            if y + 12 > b.y + b.h {
                break;
            }
            text(r, b.x + 2, y, [220, 226, 235], line);
        }
    }
}

/// A solid rectangle at physical position `p` and size `size`.
fn fill(
    r: &mut Renderer,
    art: &Art,
    atlas: TextureId,
    p: (f32, f32),
    size: (f32, f32),
    rgb: [u8; 3],
) {
    r.quad(atlas, art.white, p.0, p.1, size.0, size.1, rgba(rgb));
}

fn rgba(c: [u8; 3]) -> [f32; 4] {
    [
        f32::from(c[0]) / 255.0,
        f32::from(c[1]) / 255.0,
        f32::from(c[2]) / 255.0,
        1.0,
    ]
}

/// Text in the game's 8×12 font at physical `x`,`y`, scaled by `scale`, tinted.
#[allow(clippy::too_many_arguments)]
pub fn draw_text(
    r: &mut Renderer,
    art: &Art,
    atlas: TextureId,
    x: f32,
    y: f32,
    scale: f32,
    rgb: [u8; 3],
    s: &str,
) {
    if art.tallfont.w == 0 {
        return;
    }
    let tint = rgba(rgb);
    for (i, ch) in s.chars().enumerate() {
        let c = ch as u32;
        if !(32..128).contains(&c) {
            continue;
        }
        let glyph = art.tallfont.frame(8, 12, c - 32);
        r.quad(
            atlas,
            glyph,
            x + i as f32 * 8.0 * scale,
            y,
            8.0 * scale,
            12.0 * scale,
            tint,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::skin::parse_layout;

    #[test]
    fn scale_fits_3x_on_2560x1600() {
        assert_eq!(skin_scale((2560, 1600), (640, 420)), 3);
        assert_eq!(skin_origin((2560, 1600), (640, 420), 3), (320.0, 170.0));
    }

    #[test]
    fn scale_never_below_one_and_maps_when_clipped() {
        assert_eq!(skin_scale((500, 300), (640, 420)), 1);
        let o = skin_origin((500, 300), (640, 420), 1);
        assert_eq!(o, (-70.0, -60.0));
        assert_eq!(to_skin((0.0, 0.0), o, 1), (70, 60));
        assert_eq!(to_skin((10.0, 7.0), (4.0, 1.0), 3), (2, 2));
        assert_eq!(to_skin((0.0, 0.0), (4.0, 1.0), 3), (-2, -1));
    }

    #[test]
    fn hotspot_priority() {
        let l = parse_layout(
            "main=(0,0,639,419)\nzonebox=(300,60,628,330)\nplay=(310,70,400,84)\nzonetext=(314,106,601,321)\nship2=(20,20,40,40)",
        )
        .unwrap();
        assert_eq!(hotspot_at(&l, 320, 75), Some(HotSpot::Play));
        assert_eq!(hotspot_at(&l, 320, 110), Some(HotSpot::ZoneText));
        assert_eq!(hotspot_at(&l, 305, 65), Some(HotSpot::ZoneBox));
        assert_eq!(hotspot_at(&l, 30, 30), Some(HotSpot::Ship(2)));
        assert_eq!(hotspot_at(&l, 5, 5), None);
        assert_eq!(hotspot_at(&l, -5, 5), None);
    }

    #[test]
    fn zone_row_at_maps_rows_and_scroll() {
        let l = parse_layout("main=(0,0,639,419)\nzonetext=(10,100,209,239)").unwrap();
        assert_eq!(zone_rows(&l), 10);
        assert_eq!(zone_row_at(&l, 20, 100 + 14 * 2 + 3, 5, 20), Some(7));
        assert_eq!(zone_row_at(&l, 20, 100, 0, 20), Some(0));
        assert_eq!(zone_row_at(&l, 20, 100 + 14 * 4, 18, 20), None);
        assert_eq!(zone_row_at(&l, 5, 120, 0, 20), None);
        assert_eq!(zone_rows(&parse_layout("main=(0,0,9,9)").unwrap()), 0);
    }

    #[test]
    fn sprite_only_in_real_slots() {
        let main = SkinRect {
            x: 0,
            y: 0,
            w: 588,
            h: 380,
        };
        let r = |x, y, w, h| SkinRect { x, y, w, h };
        assert!(sprite_slot(main, r(56, 184, 200, 152)));
        assert!(
            !sprite_slot(main, r(0, 400, 1, 1)),
            "dummy slot outside main"
        );
        assert!(!sprite_slot(main, r(10, 10, 4, 4)), "too small for a ship");
        assert!(!sprite_slot(main, r(580, 370, 30, 30)), "runs off main");
    }

    #[test]
    fn zone_line_format() {
        assert_eq!(
            zone_line("Dare", 4, Some(48), 20),
            format!(" 48 Dare{}  4", " ".repeat(9))
        );
        assert_eq!(
            zone_line("A very long zone name", 123, None, 16),
            "--- A very l 123"
        );
        assert_eq!(zone_line("Z", 5000, Some(1500), 12), "999 Z    999");
    }

    #[test]
    fn wraps_info_text() {
        assert_eq!(
            wrap("play the selected zone now", 10),
            vec!["play the", "selected", "zone now"]
        );
        assert_eq!(wrap("abcdefghijkl", 5), vec!["abcde", "fghij", "kl"]);
        assert!(wrap("", 5).is_empty());
    }
}
