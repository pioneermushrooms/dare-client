//! Loads the Continuum graphics the game draws into one texture atlas.

use std::collections::HashMap;

use oss_assets::{AssetDirs, Image, decode_image};

use crate::gfx::{AtlasBuilder, Region};

/// Atlas side length. Every stock Continuum sheet fits comfortably.
const ATLAS_SIZE: u32 = 4096;

/// Tileset grid: 19×10 tiles of 16×16 pixels (ids 1..=190).
pub const TILESET_COLUMNS: u32 = 19;

/// Atlas regions for every sheet. Missing optional sheets are empty regions and simply
/// don't draw.
#[derive(Debug, Default, Clone)]
pub struct Art {
    pub white: Region,
    pub tiles: Region,
    pub ships: Region,
    pub bullets: Region,
    pub bombs: Region,
    pub mines: Region,
    pub explode0: Region,
    pub explode1: Region,
    pub explode2: Region,
    pub warp: Region,
    pub exhaust: Region,
    pub over1: Region,
    pub over2: Region,
    pub over3: Region,
    pub over4: Region,
    pub over5: Region,
    pub goal: Region,
    pub flag: Region,
    pub hlthbar: Region,
    pub gradient: Region,
    pub engyfont: Region,
    pub tallfont: Region,
    /// `colors.bm2`: Continuum's UI palette, one colour per row and 128 columns of
    /// animation. Rows 1-3 are the three plies of a panel border and row 16 is the
    /// panel fill, which is what gives every box in the game its look.
    pub colors: Region,
    pub prizes: Region,
    pub shrapnel: Region,
    pub wall: Region,
    pub warppnt: Region,
    pub repel: Region,
    pub shield: Region,
    pub superpower: Region,
    pub icons: Region,
    pub icondoor: Region,
    pub empburst: Region,
    pub rocket: Region,
    /// Powerball: normal, phased and trail frames, a third each.
    pub powerb: Region,
    /// Per-ship replacement sheets from LVZ files (`ship1`..`ship8`): region and
    /// frame side (40 square frames).
    pub ship_sheets: [Option<(Region, u32)>; 8],
    /// The pilot's own weapon sheets for shots from one ship (`bullets@1`..`shrapnel@8`),
    /// by ship then [`SHIP_WEAPONS`] order; `None` uses the sheet for every ship.
    pub ship_weapons: [[Option<Region>; 4]; 8],
    /// Whether any of [`Self::ship_weapons`] is set (so shots needn't look up their ship).
    pub has_ship_weapons: bool,
}

/// Weapon sheets a pilot may set for one ship's shots, as `<name>@<ship>`.
pub const SHIP_WEAPONS: [&str; 4] = ["bullets", "bombs", "mines", "shrapnel"];

/// One of [`SHIP_WEAPONS`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Weapon {
    Bullets = 0,
    Bombs = 1,
    Mines = 2,
    Shrapnel = 3,
}

impl Art {
    /// The `weapon` sheet for a shot from ship `kind` (0..8), or from no known ship.
    pub fn weapon(&self, weapon: Weapon, kind: Option<u8>) -> Region {
        if let Some(own) = kind
            .and_then(|k| self.ship_weapons.get(k as usize))
            .and_then(|w| w[weapon as usize])
        {
            return own;
        }
        match weapon {
            Weapon::Bullets => self.bullets,
            Weapon::Bombs => self.bombs,
            Weapon::Mines => self.mines,
            Weapon::Shrapnel => self.shrapnel,
        }
    }
}

/// Sheets an LVZ file can replace, by file name without extension (lowercase),
/// besides `ship1`..`ship8`. Mirrors the graphics nullspace lets zones override.
pub const OVERRIDABLE: &[&str] = &[
    "ships", "bullets", "bombs", "mines", "explode0", "explode1", "explode2", "warp", "exhaust",
    "goal", "flag", "prizes", "shrapnel", "warppnt", "repel", "shield", "super", "empburst",
    "rocket", "powerb", "over1", "over2", "over3", "over4", "over5", "wall", "gradient",
];

/// Whether an LVZ file named `stem` (no extension, lowercase) replaces a standard sheet.
pub fn is_overridable(stem: &str) -> bool {
    OVERRIDABLE.contains(&stem)
        || stem
            .strip_prefix("ship")
            .and_then(|n| n.parse::<u8>().ok())
            .is_some_and(|n| (1..=8).contains(&n))
}

impl Art {
    /// Region for tile id `id` (1..=190) in the tileset.
    pub fn tile(&self, id: u8) -> Region {
        self.tiles.frame(16, 16, id.saturating_sub(1) as u32)
    }

    /// Ship sprite for ship type `kind` (0..8) facing `direction` (0..40).
    pub fn ship(&self, kind: u8, direction: u8) -> Region {
        if let Some(Some((sheet, side))) = self.ship_sheets.get(kind as usize) {
            return sheet.frame(*side, *side, direction as u32 % 40);
        }
        self.ships
            .frame(36, 36, kind as u32 * 40 + direction as u32 % 40)
    }
}

/// Picks the frame of a looping animation of `count` frames lasting `duration` seconds.
pub fn anim_frame(
    sheet: Region,
    fw: u32,
    fh: u32,
    first: u32,
    count: u32,
    duration: f32,
    t: f32,
) -> Region {
    if count == 0 || duration <= 0.0 {
        return sheet.frame(fw, fh, first);
    }
    let phase = (t / duration).rem_euclid(1.0);
    let i = ((phase * count as f32) as u32).min(count - 1);
    sheet.frame(fw, fh, first + i)
}

/// Picks the frame of a one-shot animation; `None` once it has finished.
pub fn once_frame(sheet: Region, fw: u32, fh: u32, duration: f32, t: f32) -> Option<Region> {
    let count = sheet.frame_count(fw, fh);
    if t < 0.0 || t >= duration || count == 0 {
        return None;
    }
    let i = ((t / duration * count as f32) as u32).min(count - 1);
    Some(sheet.frame(fw, fh, i))
}

/// Prepares a tileset image: black is transparent only in fly-over/under and later
/// tiles (ids 173+), matching Continuum, so walls stay opaque.
pub fn prepare_tileset(mut img: Image) -> Image {
    let w = img.width;
    for id in 173u32..=190 {
        let i = id - 1;
        let (tx, ty) = ((i % TILESET_COLUMNS) * 16, (i / TILESET_COLUMNS) * 16);
        for y in ty..(ty + 16).min(img.height) {
            for x in tx..(tx + 16).min(w) {
                let p = ((y * w + x) * 4) as usize;
                if img.rgba[p] == 0 && img.rgba[p + 1] == 0 && img.rgba[p + 2] == 0 {
                    img.rgba[p + 3] = 0;
                }
            }
        }
    }
    img
}

/// Builds the atlas. `level_tileset` is the `.lvl`'s embedded BMP, if any; otherwise
/// the install's `tiles.bm2` is used.
pub fn load(dir: &AssetDirs, level_tileset: Option<&[u8]>) -> Result<(Art, u32, Vec<u8>), String> {
    load_with(dir, level_tileset, &HashMap::new())
}

/// [`load`] with sheets replaced by `overrides` (keyed by lowercase name without
/// extension, as in [`OVERRIDABLE`] and `ship1`..`ship8`; already colour-keyed).
pub fn load_with(
    dir: &AssetDirs,
    level_tileset: Option<&[u8]>,
    overrides: &HashMap<String, Image>,
) -> Result<(Art, u32, Vec<u8>), String> {
    let mut atlas = AtlasBuilder::new(ATLAS_SIZE);
    let mut add = |img: &Image| {
        atlas
            .add(img)
            .ok_or_else(|| "texture atlas full".to_string())
    };

    let mut art = Art {
        white: add(&Image::filled(1, 1, [255; 4]).map_err(|e| e.to_string())?)?,
        ..Art::default()
    };

    let tileset = match level_tileset.map(decode_image) {
        Some(Ok(img)) => img,
        Some(Err(e)) => {
            log::warn!("map tileset unreadable ({e}); using default tiles");
            dir.load_graphic("tiles").map_err(|e| e.to_string())?
        }
        None => dir.load_graphic("tiles").map_err(|e| e.to_string())?,
    };
    art.tiles = add(&prepare_tileset(tileset))?;
    // Loaded before the sheet helper below, because black is a colour here (row 0
    // is "blank") and must not be keyed out.
    art.colors = match dir.load_graphic("colors") {
        Ok(img) => add(&img)?,
        Err(e) => {
            log::warn!("{e}");
            Region::default()
        }
    };

    let mut sheet = |name: &str, required: bool| -> Result<Region, String> {
        if let Some(img) = overrides.get(&name.to_ascii_lowercase()) {
            return add(img);
        }
        match dir.load_graphic(name) {
            Ok(mut img) => {
                img.key_black_transparent();
                add(&img)
            }
            Err(e) if required => Err(e.to_string()),
            Err(e) => {
                log::warn!("{e}");
                Ok(Region::default())
            }
        }
    };
    art.ships = sheet("ships", true)?;
    art.bullets = sheet("bullets", false)?;
    art.bombs = sheet("bombs", false)?;
    art.mines = sheet("mines", false)?;
    art.explode0 = sheet("explode0", false)?;
    art.explode1 = sheet("explode1", false)?;
    art.explode2 = sheet("explode2", false)?;
    art.warp = sheet("warp", false)?;
    art.exhaust = sheet("exhaust", false)?;
    art.over1 = sheet("over1", false)?;
    art.over2 = sheet("over2", false)?;
    art.over3 = sheet("over3", false)?;
    art.over4 = sheet("over4", false)?;
    art.over5 = sheet("over5", false)?;
    art.goal = sheet("goal", false)?;
    art.flag = sheet("flag", false)?;
    art.hlthbar = sheet("hlthbar", false)?;
    art.gradient = sheet("gradient", false)?;
    art.engyfont = sheet("engyfont", false)?;
    art.tallfont = sheet("tallfont", false)?;
    art.prizes = sheet("prizes", false)?;
    art.shrapnel = sheet("shrapnel", false)?;
    art.wall = sheet("wall", false)?;
    art.warppnt = sheet("warppnt", false)?;
    art.repel = sheet("repel", false)?;
    art.shield = sheet("shield", false)?;
    art.superpower = sheet("super", false)?;
    art.icons = sheet("Icons", false)?;
    art.icondoor = sheet("icondoor", false)?;
    art.empburst = sheet("empburst", false)?;
    art.rocket = sheet("rocket", false)?;
    art.powerb = sheet("powerb", false)?;
    drop(sheet);
    for (k, slot) in art.ship_sheets.iter_mut().enumerate() {
        if let Some(img) = overrides.get(&format!("ship{}", k + 1)) {
            // 40 square frames (10 by 4 in the stock layout).
            let side = ((img.width as f64 * img.height as f64 / 40.0).sqrt()) as u32;
            if side > 0 {
                *slot = Some((add(img)?, side));
            }
        }
    }

    for (k, sheets) in art.ship_weapons.iter_mut().enumerate() {
        for (w, name) in SHIP_WEAPONS.iter().enumerate() {
            if let Some(img) = overrides.get(&format!("{name}@{}", k + 1)) {
                sheets[w] = Some(add(img)?);
                art.has_ship_weapons = true;
            }
        }
    }

    let size = atlas.size();
    Ok((art, size, atlas.into_pixels()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ships_own_weapon_sheet_wins_for_its_shots_only() {
        let region = |x| Region {
            x,
            y: 0,
            w: 16,
            h: 16,
        };
        let mut art = Art {
            bombs: region(1),
            ..Art::default()
        };
        art.ship_weapons[2][Weapon::Bombs as usize] = Some(region(7));
        assert_eq!(art.weapon(Weapon::Bombs, Some(2)).x, 7);
        assert_eq!(art.weapon(Weapon::Bombs, Some(3)).x, 1);
        assert_eq!(art.weapon(Weapon::Bombs, None).x, 1);
        assert_eq!(art.weapon(Weapon::Bombs, Some(200)).x, 1);
        assert_eq!(art.weapon(Weapon::Mines, Some(2)).x, 0);
    }
}
