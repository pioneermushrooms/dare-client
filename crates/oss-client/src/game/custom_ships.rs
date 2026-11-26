//! The pilot's own ship graphics: sheets that replace `ship1`..`ship8` for this
//! player only, on top of whatever the arena's LVZ files and the install supply.
//!
//! A sheet is the layout the game already reads ship replacements in (see
//! [`super::art::Art::ship`]): 40 square frames in 10 columns and 4 rows, so
//! 360x144 for the stock 36-pixel ships. Frame 0, top left, is the ship nose-up,
//! and each next frame in row order is the ship turned another 9 degrees
//! clockwise (SubSpace's 40 headings). Sheets are stored as drawn; turning one
//! that faces backward is done before it gets here.
//!
//! The page sets these (browser play) at any time, before or during a visit, so
//! they live outside any one game: the game rebuilds its atlas when
//! [`revision`] moves. What each slot shows after a rebuild is kept in
//! [`shown`], so the page can display the ships the player is actually flying.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use oss_assets::{AssetDirs, Image};

use super::art::{self, Art};

/// Ship types the game has.
pub const SLOTS: usize = 8;
/// Headings in a sheet, and its grid.
pub const FRAMES: u32 = 40;
pub const COLUMNS: u32 = 10;
pub const ROWS: u32 = 4;

static CUSTOM: Mutex<[Option<Image>; SLOTS]> = Mutex::new([const { None }; SLOTS]);
static SHOWN: Mutex<[Option<Image>; SLOTS]> = Mutex::new([const { None }; SLOTS]);
static REVISION: AtomicU64 = AtomicU64::new(0);

/// Moves every time a sheet is set or cleared.
pub fn revision() -> u64 {
    REVISION.load(Ordering::Relaxed)
}

/// Whether `img` is a ship sheet: 40 square frames in 10 columns and 4 rows.
// Set and read by the page in browser play.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub fn is_sheet(img: &Image) -> bool {
    let side = img.width / COLUMNS;
    side >= 8 && img.width == side * COLUMNS && img.height == side * ROWS
}

/// Uses `img` for ship `slot` (0..8), or the arena's or game's own ship with `None`.
/// A sheet with no transparency at all has its black keyed out, as LVZ sheets do.
// Set and read by the page in browser play.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub fn set(slot: usize, img: Option<Image>) -> Result<(), String> {
    if slot >= SLOTS {
        return Err(format!("ship {} doesn't exist", slot + 1));
    }
    let img = match img {
        Some(mut img) => {
            if !is_sheet(&img) {
                return Err(format!(
                    "a ship sheet is 40 square frames in 10 columns and 4 rows, not {}x{}",
                    img.width, img.height
                ));
            }
            if img.rgba.as_chunks::<4>().0.iter().all(|p| p[3] == 255) {
                img.key_black_transparent();
            }
            Some(img)
        }
        None => None,
    };
    let mut custom = CUSTOM.lock().unwrap_or_else(|e| e.into_inner());
    custom[slot] = img;
    REVISION.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// The sheet ship `slot` (0..8) was drawn with at the last atlas build.
// Set and read by the page in browser play.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub fn shown(slot: usize) -> Option<Image> {
    SHOWN
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(slot)
        .cloned()
        .flatten()
}

/// `overrides` (an arena's LVZ replacements) with the pilot's own sheets on top.
pub fn merged(overrides: &HashMap<String, Image>) -> HashMap<String, Image> {
    let mut all = overrides.clone();
    let custom = CUSTOM.lock().unwrap_or_else(|e| e.into_inner());
    for (k, img) in custom.iter().enumerate() {
        if let Some(img) = img {
            all.insert(format!("ship{}", k + 1), img.clone());
        }
    }
    all
}

/// [`art::load_with`] with the pilot's own sheets on top of `overrides`, recording
/// what each ship now looks like for [`shown`].
pub fn load_art(
    dir: &AssetDirs,
    level_tileset: Option<&[u8]>,
    overrides: &HashMap<String, Image>,
) -> Result<(Art, u32, Vec<u8>), String> {
    let all = merged(overrides);
    let built = art::load_with(dir, level_tileset, &all)?;
    *SHOWN.lock().unwrap_or_else(|e| e.into_inner()) = shown_sheets(dir, &all);
    Ok(built)
}

/// Each ship's sheet: its own replacement, else its four rows of the `ships` sheet
/// (the arena's replacement for it, else the install's).
fn shown_sheets(dir: &AssetDirs, all: &HashMap<String, Image>) -> [Option<Image>; SLOTS] {
    let ships = all.get("ships").cloned().or_else(|| {
        dir.load_graphic("ships").ok().map(|mut img| {
            img.key_black_transparent();
            img
        })
    });
    std::array::from_fn(|k| {
        all.get(&format!("ship{}", k + 1)).cloned().or_else(|| {
            let ships = ships.as_ref()?;
            ship_rows(ships, k)
        })
    })
}

/// Ship `kind`'s 40 frames out of a whole `ships` sheet (36-pixel frames, the
/// eight ships one after another), laid out 10 by 4 like a single ship's sheet.
fn ship_rows(ships: &Image, kind: usize) -> Option<Image> {
    let side = 36;
    let cols = ships.width / side;
    if cols == 0 {
        return None;
    }
    let mut out = Image::filled(side * COLUMNS, side * ROWS, [0; 4]).ok()?;
    for f in 0..FRAMES {
        let i = kind as u32 * FRAMES + f;
        let (sx, sy) = ((i % cols) * side, (i / cols) * side);
        if sy + side > ships.height {
            return None;
        }
        let (dx, dy) = ((f % COLUMNS) * side, (f / COLUMNS) * side);
        for y in 0..side {
            let src = (((sy + y) * ships.width + sx) * 4) as usize;
            let dst = (((dy + y) * out.width + dx) * 4) as usize;
            let n = (side * 4) as usize;
            out.rgba[dst..dst + n].copy_from_slice(&ships.rgba[src..src + n]);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sheet(w: u32, h: u32, alpha: u8) -> Image {
        Image::filled(w, h, [0, 0, 0, alpha]).unwrap()
    }

    #[test]
    fn a_sheet_is_ten_by_four_square_frames() {
        assert!(is_sheet(&sheet(360, 144, 255)));
        assert!(is_sheet(&sheet(480, 192, 255)));
        assert!(!is_sheet(&sheet(144, 360, 255)));
        assert!(!is_sheet(&sheet(360, 1152, 255)));
        assert!(!is_sheet(&sheet(361, 144, 255)));
    }

    #[test]
    fn own_sheets_go_on_top_of_the_arenas_and_can_be_taken_off() {
        let mut lvz = HashMap::new();
        lvz.insert("ship2".to_string(), sheet(360, 144, 255));
        lvz.insert("ship3".to_string(), sheet(360, 144, 255));
        assert!(set(8, Some(sheet(360, 144, 255))).is_err());
        assert!(set(2, Some(sheet(100, 100, 255))).is_err());
        let before = revision();
        set(2, Some(sheet(480, 192, 255))).unwrap();
        assert!(revision() > before);
        let all = merged(&lvz);
        assert_eq!(all["ship3"].width, 480, "the pilot's own wins");
        assert_eq!(
            all["ship2"].width, 360,
            "the arena's stays where the pilot has none"
        );
        // An opaque sheet has its black keyed out, like an LVZ sheet.
        assert_eq!(all["ship3"].pixel(0, 0).unwrap()[3], 0);
        set(2, None).unwrap();
        assert_eq!(merged(&lvz)["ship3"].width, 360);
    }

    #[test]
    fn a_ships_rows_come_out_as_its_own_sheet() {
        // A whole ships sheet: 10 frames across, 32 rows, eight ships of four rows each.
        let mut ships = Image::filled(360, 36 * 32, [0; 4]).unwrap();
        // Mark ship 5's frame 13 (overall frame 213: column 3, row 21).
        let (x, y) = (3 * 36 + 1, 21 * 36 + 2);
        let p = ((y * 360 + x) * 4) as usize;
        ships.rgba[p..p + 4].copy_from_slice(&[9, 9, 9, 255]);
        let one = ship_rows(&ships, 5).unwrap();
        assert_eq!((one.width, one.height), (360, 144));
        // Frame 13 of a single sheet: column 3, row 1.
        assert_eq!(one.pixel(3 * 36 + 1, 36 + 2), Some([9, 9, 9, 255]));
    }
}
