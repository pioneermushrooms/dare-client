//! Continuum `.skn` skins: the start screen's picture and where its buttons are.
//!
//! A skin file is a Windows BMP with the skin's description text appended after
//! the bitmap's declared size. The text names hot spots (`play`, `ship1`,
//! `zonetext`, ...) as rectangles of the main region, each with optional overlay
//! frames drawn when the spot is not highlighted (NORM) or is (ALT, after a `+`),
//! plus a few colour definitions. The format is the one Continuum 0.40 reads, so
//! skins made for it load here unchanged.
//!
//! Skin files are untrusted: anything this parser does not understand is skipped
//! with a warning, never a panic.

use oss_assets::image::{Image, decode_image};

/// How long one animation tick lasts.
pub const TICK_MS: u64 = 200;
/// The largest skin file read.
pub const MAX_SKIN_BYTES: usize = 32 * 1024 * 1024;

/// A rectangle of skin pixels. The text format gives inclusive corners; this
/// stores the origin and size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SkinRect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl SkinRect {
    /// Whether the pixel at `x`,`y` is inside.
    pub fn holds(&self, x: u32, y: u32) -> bool {
        x >= self.x && y >= self.y && x - self.x < self.w && y - self.y < self.h
    }
}

/// One frame of an overlay animation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frame {
    /// Draw the region's size of bitmap from this top-left point.
    At(u32, u32),
    /// `(*)`: draw nothing, so the main region shows.
    Blank,
}

/// A frame and how many ticks it stays up.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    pub ticks: u32,
    pub frame: Frame,
}

/// One rectangle of a hot spot with its overlays.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkinRegion {
    pub rect: SkinRect,
    /// Drawn while the spot is not highlighted.
    pub norm: Vec<Step>,
    /// Drawn while it is.
    pub alt: Vec<Step>,
}

/// The hot spots the skin format names. Anything else a skin defines is kept as
/// [`HotSpot::Other`]: it animates, but does nothing when clicked.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum HotSpot {
    Main,
    Profile,
    PrevProf,
    NextProf,
    Macros,
    KeyDefs,
    Spectate,
    /// `ship1`..`ship8`.
    Ship(u8),
    Zones,
    Trace,
    ZoneUp,
    ZoneDown,
    Ping,
    Options,
    AdvOpt,
    Play,
    Quit,
    ProfText,
    ProfBox,
    ShipBox,
    InfoBox,
    ZoneText,
    ZoneBox,
    /// A name the format does not define, lower-cased.
    Other(String),
}

impl HotSpot {
    fn from_name(name: &str) -> HotSpot {
        let n = name.to_ascii_lowercase();
        match n.as_str() {
            "main" => HotSpot::Main,
            "profile" => HotSpot::Profile,
            "prevprof" => HotSpot::PrevProf,
            "nextprof" => HotSpot::NextProf,
            "macros" => HotSpot::Macros,
            "keydefs" => HotSpot::KeyDefs,
            "spectate" => HotSpot::Spectate,
            "zones" => HotSpot::Zones,
            "trace" => HotSpot::Trace,
            "zoneup" => HotSpot::ZoneUp,
            "zonedown" => HotSpot::ZoneDown,
            "ping" => HotSpot::Ping,
            "options" => HotSpot::Options,
            "advopt" => HotSpot::AdvOpt,
            "play" => HotSpot::Play,
            "quit" => HotSpot::Quit,
            "proftext" => HotSpot::ProfText,
            "profbox" => HotSpot::ProfBox,
            "shipbox" => HotSpot::ShipBox,
            "infobox" => HotSpot::InfoBox,
            "zonetext" => HotSpot::ZoneText,
            "zonebox" => HotSpot::ZoneBox,
            _ => match n.strip_prefix("ship").and_then(|d| d.parse::<u8>().ok()) {
                Some(k @ 1..=8) if n.len() == 5 => HotSpot::Ship(k),
                _ => HotSpot::Other(n),
            },
        }
    }
}

/// The skin's optional colours; `None` means use the client's own.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SkinColors {
    /// `zonecolor1..3`: low, medium and high ping.
    pub zone: [Option<[u8; 3]>; 3],
    /// `zoneselcolor1..2`: the selection bar.
    pub zone_sel: [Option<[u8; 3]>; 2],
    /// `profilecolor1..4`: the profile text lines.
    pub profile: [Option<[u8; 3]>; 4],
}

/// Everything the description text says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkinLayout {
    pub main: SkinRect,
    /// In file order; a spot defined on several lines appears several times.
    pub spots: Vec<(HotSpot, SkinRegion)>,
    pub colors: SkinColors,
    /// Lines or overlays that were skipped, for the log.
    pub warnings: Vec<String>,
}

impl SkinLayout {
    /// Every region of `h`, in file order.
    pub fn regions<'a>(&'a self, h: &'a HotSpot) -> impl Iterator<Item = &'a SkinRegion> {
        self.spots
            .iter()
            .filter(move |(s, _)| s == h)
            .map(|(_, r)| r)
    }

    /// The first rectangle of `h` -- the one text boxes and ship images use.
    pub fn first_rect(&self, h: &HotSpot) -> Option<SkinRect> {
        self.regions(h).next().map(|r| r.rect)
    }
}

/// Why a skin could not be used.
#[derive(Debug)]
pub enum SkinError {
    TooLarge,
    NotBmp,
    Image(String),
    NoMain,
    Empty,
}

impl std::fmt::Display for SkinError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SkinError::TooLarge => write!(f, "skin file is larger than 32 MiB"),
            SkinError::NotBmp => write!(f, "skin file does not start with a BMP"),
            SkinError::Image(e) => write!(f, "skin image: {e}"),
            SkinError::NoMain => write!(f, "skin has no `main` region"),
            SkinError::Empty => write!(f, "skin file is empty"),
        }
    }
}

/// A loaded skin.
pub struct Skin {
    pub name: String,
    pub image: Image,
    pub layout: SkinLayout,
}

impl std::fmt::Debug for Skin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Skin")
            .field("name", &self.name)
            .field("image", &(self.image.width, self.image.height))
            .finish()
    }
}

/// The frame showing at `tick` for a list of steps, cycling.
pub fn step_at(steps: &[Step], tick: u64) -> Option<Frame> {
    let total: u64 = steps.iter().map(|s| u64::from(s.ticks.max(1))).sum();
    if total == 0 {
        return None;
    }
    let mut t = tick % total;
    for s in steps {
        let n = u64::from(s.ticks.max(1));
        if t < n {
            return Some(s.frame);
        }
        t -= n;
    }
    None
}

/// Parses a skin's description text.
pub fn parse_layout(text: &str) -> Result<SkinLayout, SkinError> {
    let mut main = None;
    let mut spots = Vec::new();
    let mut colors = SkinColors::default();
    let mut warnings = Vec::new();
    for (n, raw) in text.lines().enumerate() {
        let line: String = raw
            .split(';')
            .next()
            .unwrap_or("")
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        if line.is_empty() {
            continue;
        }
        let mut warn = |why: &str| warnings.push(format!("line {}: {why}: {}", n + 1, raw.trim()));
        let Some((name, rest)) = line.split_once('=') else {
            warn("no `=`");
            continue;
        };
        let lname = name.to_ascii_lowercase();
        if let Some(slot) = color_slot(&mut colors, &lname) {
            match parse_numbers(rest).filter(|v| v.len() == 3 && rest.starts_with('(')) {
                Some(v)
                    if v.iter().all(|&c| c <= 255)
                        && rest.ends_with(')')
                        && rest.matches('(').count() == 1 =>
                {
                    *slot = Some([v[0] as u8, v[1] as u8, v[2] as u8]);
                }
                _ => warn("bad colour"),
            }
            continue;
        }
        let spot = HotSpot::from_name(name);
        let Some(region) = parse_region(rest) else {
            warn("bad region");
            continue;
        };
        if spot == HotSpot::Main {
            if main.is_some() {
                warn("second `main` ignored");
            } else {
                if !region.norm.is_empty() || !region.alt.is_empty() {
                    warn("`main` overlays ignored");
                }
                main = Some(region.rect);
            }
            continue;
        }
        spots.push((spot, region));
    }
    let main = main.ok_or(SkinError::NoMain)?;
    Ok(SkinLayout {
        main,
        spots,
        colors,
        warnings,
    })
}

fn color_slot<'a>(c: &'a mut SkinColors, name: &str) -> Option<&'a mut Option<[u8; 3]>> {
    let idx = |p: &str| name.strip_prefix(p).and_then(|d| d.parse::<usize>().ok());
    if let Some(i) = idx("zoneselcolor") {
        return c.zone_sel.get_mut(i.checked_sub(1)?);
    }
    if let Some(i) = idx("zonecolor") {
        return c.zone.get_mut(i.checked_sub(1)?);
    }
    if let Some(i) = idx("profilecolor") {
        return c.profile.get_mut(i.checked_sub(1)?);
    }
    None
}

/// The numbers in one `(a,b,...)` group, or `None` if anything else is in it.
fn parse_numbers(group: &str) -> Option<Vec<u32>> {
    let inner = group.strip_prefix('(')?.split(')').next()?;
    inner.split(',').map(|s| s.parse::<u32>().ok()).collect()
}

/// `(x1,y1,x2,y2)` followed by NORM steps, then optionally `+` and ALT steps.
fn parse_region(s: &str) -> Option<SkinRegion> {
    let close = s.find(')')?;
    let r = parse_numbers(&s[..=close])?;
    let [x1, y1, x2, y2] = r[..] else {
        return None;
    };
    if x2 < x1 || y2 < y1 {
        return None;
    }
    let rect = SkinRect {
        x: x1,
        y: y1,
        w: (x2 - x1).checked_add(1)?,
        h: (y2 - y1).checked_add(1)?,
    };
    let mut norm = Vec::new();
    let mut alt = Vec::new();
    let mut in_alt = false;
    let mut rest = &s[close + 1..];
    while !rest.is_empty() {
        if let Some(r) = rest.strip_prefix('+') {
            if in_alt {
                return None;
            }
            in_alt = true;
            rest = r;
            continue;
        }
        let digits = rest.chars().take_while(char::is_ascii_digit).count();
        let ticks = if digits == 0 {
            1
        } else {
            rest[..digits].parse::<u32>().ok()?.max(1)
        };
        rest = &rest[digits..];
        let end = rest.find(')')?;
        let group = &rest[..=end];
        let frame = if group == "(*)" {
            Frame::Blank
        } else {
            match parse_numbers(group)?[..] {
                [x, y] => Frame::At(x, y),
                _ => return None,
            }
        };
        rest = &rest[end + 1..];
        let step = Step { ticks, frame };
        if in_alt {
            alt.push(step)
        } else {
            norm.push(step)
        }
    }
    Some(SkinRegion { rect, norm, alt })
}

/// The skin used when none is chosen, or the chosen one is gone.
pub const DEFAULT_SKIN: &str = "Dare";

/// A skin file found on disk; named by its file stem.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkinEntry {
    pub name: String,
    pub path: std::path::PathBuf,
}

/// The folders skins are looked for in, in order: the bundled one, the
/// settings folder's, a Continuum install's, then any the settings add. A
/// later folder's skin replaces an earlier one of the same name.
pub fn skin_dirs(
    bundled_assets: Option<&std::path::Path>,
    settings_dir: &std::path::Path,
    continuum: Option<&std::path::Path>,
    extra: &[std::path::PathBuf],
) -> Vec<std::path::PathBuf> {
    let mut v = Vec::new();
    v.extend(bundled_assets.map(|a| a.join("skins")));
    v.push(settings_dir.join("skins"));
    v.extend(continuum.map(|c| c.join("skins")));
    v.extend(extra.iter().cloned());
    v
}

/// Every `.skn` in `dirs`; `Dare` first, then by name ignoring case.
pub fn find_skins(dirs: &[std::path::PathBuf]) -> Vec<SkinEntry> {
    let mut found: Vec<SkinEntry> = Vec::new();
    for dir in dirs {
        let Ok(rd) = std::fs::read_dir(dir) else {
            continue;
        };
        let mut here: Vec<_> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_file() && p.extension().is_some_and(|x| x.eq_ignore_ascii_case("skn")))
            .collect();
        here.sort();
        for path in here {
            let Some(name) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            found.retain(|e| !e.name.eq_ignore_ascii_case(name));
            found.push(SkinEntry {
                name: name.to_string(),
                path,
            });
        }
    }
    found.sort_by_key(|e| {
        (
            !e.name.eq_ignore_ascii_case(DEFAULT_SKIN),
            e.name.to_lowercase(),
        )
    });
    found
}

/// The skin called `wanted` (ignoring case), else `Dare`, else the first.
pub fn pick<'a>(entries: &'a [SkinEntry], wanted: &str) -> Option<&'a SkinEntry> {
    let named = |n: &str| entries.iter().find(|e| e.name.eq_ignore_ascii_case(n));
    named(wanted)
        .or_else(|| named(DEFAULT_SKIN))
        .or_else(|| entries.first())
}

/// The skin after `current` in the list, wrapping; the first if `current` is gone.
pub fn next_skin<'a>(entries: &'a [SkinEntry], current: &str) -> Option<&'a SkinEntry> {
    let at = entries
        .iter()
        .position(|e| e.name.eq_ignore_ascii_case(current));
    match at {
        Some(i) => entries.get((i + 1) % entries.len()),
        None => entries.first(),
    }
}

/// Loads a skin from the bytes of a `.skn` file.
pub fn load_skin(name: &str, bytes: &[u8]) -> Result<Skin, SkinError> {
    if bytes.is_empty() {
        return Err(SkinError::Empty);
    }
    if bytes.len() > MAX_SKIN_BYTES {
        return Err(SkinError::TooLarge);
    }
    if bytes.len() < 6 || !bytes.starts_with(b"BM") {
        return Err(SkinError::NotBmp);
    }
    let declared = u32::from_le_bytes([bytes[2], bytes[3], bytes[4], bytes[5]]) as usize;
    let split = declared.clamp(6, bytes.len());
    let image = decode_image(&bytes[..split]).map_err(|e| SkinError::Image(format!("{e:?}")))?;
    let text: String = bytes[split..].iter().map(|&b| b as char).collect();
    let mut layout = parse_layout(&text)?;
    let (iw, ih) = (image.width, image.height);
    let fits = |x: u32, y: u32, w: u32, h: u32| {
        u64::from(x) + u64::from(w) <= u64::from(iw) && u64::from(y) + u64::from(h) <= u64::from(ih)
    };
    let m = layout.main;
    if !fits(m.x, m.y, m.w, m.h) {
        layout.main = SkinRect {
            x: m.x.min(iw.saturating_sub(1)),
            y: m.y.min(ih.saturating_sub(1)),
            w: m.w.min(iw.saturating_sub(m.x.min(iw))),
            h: m.h.min(ih.saturating_sub(m.y.min(ih))),
        };
        layout
            .warnings
            .push("`main` is larger than the image; clipped".into());
    }
    let mut dropped = Vec::new();
    // A hot spot must be on the image, which also keeps every later sum of a
    // position and a size far from overflowing.
    layout.spots.retain(|(spot, region)| {
        let r = region.rect;
        let ok = fits(r.x, r.y, r.w, r.h);
        if !ok {
            dropped.push(format!("{spot:?}: ({},{}) is off the image", r.x, r.y));
        }
        ok
    });
    for (spot, region) in &mut layout.spots {
        let (w, h) = (region.rect.w, region.rect.h);
        for list in [&mut region.norm, &mut region.alt] {
            list.retain(|s| match s.frame {
                Frame::At(x, y) if !fits(x, y, w, h) => {
                    dropped.push(format!("{spot:?}: overlay at ({x},{y}) runs off the image"));
                    false
                }
                _ => true,
            });
        }
    }
    layout.warnings.extend(dropped);
    Ok(Skin {
        name: name.to_string(),
        image,
        layout,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 24-bit bottom-up BMP of `w`×`h`, all one grey.
    fn bmp(w: u32, h: u32) -> Vec<u8> {
        let row = (w * 3).div_ceil(4) * 4;
        let size = 54 + row * h;
        let mut d = Vec::new();
        d.extend_from_slice(b"BM");
        d.extend_from_slice(&size.to_le_bytes());
        d.extend_from_slice(&[0; 4]);
        d.extend_from_slice(&54u32.to_le_bytes());
        d.extend_from_slice(&40u32.to_le_bytes());
        d.extend_from_slice(&(w as i32).to_le_bytes());
        d.extend_from_slice(&(h as i32).to_le_bytes());
        d.extend_from_slice(&1u16.to_le_bytes());
        d.extend_from_slice(&24u16.to_le_bytes());
        d.extend_from_slice(&[0; 24]);
        d.resize(size as usize, 0x80);
        d
    }

    #[test]
    fn later_folder_wins_and_dare_first() {
        let root = std::env::temp_dir().join(format!("oss-skins-{}", std::process::id()));
        let (a, b) = (root.join("a"), root.join("b"));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        for f in ["b.skn", "Dare.skn", "a.SKN", "notes.txt"] {
            std::fs::write(a.join(f), b"x").unwrap();
        }
        std::fs::write(b.join("A.skn"), b"x").unwrap();
        let found = find_skins(&[a.clone(), root.join("missing"), b.clone()]);
        let names: Vec<_> = found.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["Dare", "A", "b"]);
        assert_eq!(found[1].path, b.join("A.skn"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn next_skin_wraps() {
        let e = |n: &str| SkinEntry {
            name: n.into(),
            path: n.into(),
        };
        let list = vec![e("Dare"), e("Dare Night"), e("Pipe Dreams")];
        assert_eq!(next_skin(&list, "dare").unwrap().name, "Dare Night");
        assert_eq!(next_skin(&list, "Pipe Dreams").unwrap().name, "Dare");
        assert_eq!(next_skin(&list, "Gone").unwrap().name, "Dare");
        assert!(next_skin(&[], "Dare").is_none());
    }

    #[test]
    fn missing_skin_falls_back_to_dare() {
        let e = |n: &str| SkinEntry {
            name: n.into(),
            path: n.into(),
        };
        assert_eq!(
            pick(&[e("Dare Night"), e("Dare")], "Gone").unwrap().name,
            "Dare"
        );
    }

    #[test]
    fn pick_is_case_insensitive_then_dare() {
        let e = |n: &str| SkinEntry {
            name: n.into(),
            path: n.into(),
        };
        let list = vec![e("Dare"), e("Pipe Dreams")];
        assert_eq!(pick(&list, "pipe dreams").unwrap().name, "Pipe Dreams");
        assert_eq!(pick(&list, "Gone").unwrap().name, "Dare");
        assert_eq!(pick(&[e("Solo")], "Gone").unwrap().name, "Solo");
        assert!(pick(&[], "Dare").is_none());
    }

    #[test]
    fn skin_dirs_in_order() {
        let d = skin_dirs(
            Some(std::path::Path::new("/app/assets")),
            std::path::Path::new("/cfg"),
            Some(std::path::Path::new("/cont")),
            &["/extra".into()],
        );
        let want: Vec<std::path::PathBuf> = vec![
            "/app/assets/skins".into(),
            "/cfg/skins".into(),
            "/cont/skins".into(),
            "/extra".into(),
        ];
        assert_eq!(d, want);
    }

    #[test]
    fn parses_main_and_simple_alt() {
        let l = parse_layout("MAIN=(0,0,639,479)\nprofile = (0,0,19,19) + (640,0)").unwrap();
        assert_eq!(
            l.main,
            SkinRect {
                x: 0,
                y: 0,
                w: 640,
                h: 480
            }
        );
        let r = l.regions(&HotSpot::Profile).next().unwrap();
        assert_eq!(
            r.rect,
            SkinRect {
                x: 0,
                y: 0,
                w: 20,
                h: 20
            }
        );
        assert!(r.norm.is_empty());
        assert_eq!(
            r.alt,
            vec![Step {
                ticks: 1,
                frame: Frame::At(640, 0)
            }]
        );
    }

    #[test]
    fn counts_blanks_and_norm_alt_split() {
        let l = parse_layout(
            "main=(0,0,9,9)\nzones=(1,1,5,5)(0,800)2(0,805)(*)2(0,810)+(20,800)3(20,810)",
        )
        .unwrap();
        let r = l.regions(&HotSpot::Zones).next().unwrap();
        assert_eq!(
            r.norm.iter().map(|s| s.ticks).collect::<Vec<_>>(),
            vec![1, 2, 1, 2]
        );
        assert_eq!(r.norm[2].frame, Frame::Blank);
        assert_eq!(r.alt.len(), 2);
        assert_eq!(
            r.alt[1],
            Step {
                ticks: 3,
                frame: Frame::At(20, 810)
            }
        );
    }

    #[test]
    fn whitespace_case_and_comments_ignored() {
        let a =
            parse_layout("main=(0,0,9,9)\nprofile=(0,0,20,20)(800,0)(810,0)+(900,0)2(910,0) ; x")
                .unwrap();
        let b = parse_layout(
            "MAIN = ( 0 , 0 , 9 , 9 )\r\nProfile        =(0,0,     20,20)(800    ,0)(810,0)+(    900,0)2     (910,0)",
        )
        .unwrap();
        assert_eq!(a.spots, b.spots);
        assert_eq!(a.spots.len(), 1);
    }

    #[test]
    fn repeated_lines_add_regions() {
        let l = parse_layout(
            "main=(0,0,999,999)\nplay = (400,400,435,435) + (800,0)\nplay = (400,435,420,470) + (800,35)",
        )
        .unwrap();
        assert_eq!(l.regions(&HotSpot::Play).count(), 2);
    }

    #[test]
    fn ships_and_unknown_names() {
        let l = parse_layout("main=(0,0,99,99)\nship3=(1,1,2,2)\nUser7=(3,3,4,4)(5,5)").unwrap();
        assert!(l.first_rect(&HotSpot::Ship(3)).is_some());
        assert!(l.first_rect(&HotSpot::Other("user7".into())).is_some());
        assert!(
            parse_layout("main=(0,0,9,9)\nship9=(1,1,2,2)")
                .unwrap()
                .first_rect(&HotSpot::Ship(9))
                .is_none()
        );
    }

    #[test]
    fn colours_parse() {
        let l = parse_layout(
            "main=(0,0,9,9)\nzoneselcolor1=(255,0,255)\nprofilecolor4 = (1,2,3)\nzonecolor2=(9,8,7)",
        )
        .unwrap();
        assert_eq!(l.colors.zone_sel[0], Some([255, 0, 255]));
        assert_eq!(l.colors.profile[3], Some([1, 2, 3]));
        assert_eq!(l.colors.zone[1], Some([9, 8, 7]));
        assert_eq!(l.colors.zone[0], None);
    }

    #[test]
    fn missing_main_is_error() {
        assert!(matches!(
            parse_layout("play=(0,0,1,1)"),
            Err(SkinError::NoMain)
        ));
        assert!(matches!(parse_layout(""), Err(SkinError::NoMain)));
    }

    #[test]
    fn skips_bad_lines_keeps_rest() {
        let l = parse_layout(
            "main=(0,0,99,99)\nplay=(10,10,5,5)\ngarbage\nquit=(1,1,2,2)+(oops)\nzones=(0,0,9,9)",
        )
        .unwrap();
        assert!(l.first_rect(&HotSpot::Zones).is_some());
        assert!(l.first_rect(&HotSpot::Play).is_none());
        assert!(l.first_rect(&HotSpot::Quit).is_none());
        assert!(l.warnings.len() >= 3, "{:?}", l.warnings);
    }

    #[test]
    fn step_sequence_cycles_with_counts() {
        let s = vec![
            Step {
                ticks: 1,
                frame: Frame::At(0, 0),
            },
            Step {
                ticks: 3,
                frame: Frame::At(1, 1),
            },
        ];
        let got: Vec<_> = (0..8).map(|t| step_at(&s, t).unwrap()).collect();
        let (a, b) = (Frame::At(0, 0), Frame::At(1, 1));
        assert_eq!(got, vec![a, b, b, b, a, b, b, b]);
        assert_eq!(step_at(&[], 5), None);
    }

    #[test]
    fn drops_out_of_bounds_overlays() {
        let mut d = bmp(50, 50);
        d.extend_from_slice(b"main=(0,0,39,39)\nplay=(0,0,9,9)+(45,45)(0,40)");
        let s = load_skin("t", &d).unwrap();
        let r = s.layout.regions(&HotSpot::Play).next().unwrap();
        assert_eq!(
            r.rect,
            SkinRect {
                x: 0,
                y: 0,
                w: 10,
                h: 10
            }
        );
        assert_eq!(
            r.alt,
            vec![Step {
                ticks: 1,
                frame: Frame::At(0, 40)
            }]
        );
        assert!(!s.layout.warnings.is_empty());
        assert_eq!((s.image.width, s.image.height), (50, 50));
    }

    #[test]
    fn main_larger_than_image_is_clipped() {
        let mut d = bmp(20, 20);
        d.extend_from_slice(b"main=(0,0,639,479)");
        let s = load_skin("t", &d).unwrap();
        assert_eq!(
            s.layout.main,
            SkinRect {
                x: 0,
                y: 0,
                w: 20,
                h: 20
            }
        );
    }

    /// Loads every `.skn` in `$OSS_SKIN_DIR` and prints what it found. Run by
    /// hand against real skins: `OSS_SKIN_DIR=... cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn real_skins_from_env() {
        let Ok(dir) = std::env::var("OSS_SKIN_DIR") else {
            return;
        };
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("skn")) {
                let s = load_skin("x", &std::fs::read(&p).unwrap()).unwrap();
                println!(
                    "{:?}: main {:?}, {} spots, {} warnings {:?}",
                    p.file_name().unwrap(),
                    s.layout.main,
                    s.layout.spots.len(),
                    s.layout.warnings.len(),
                    s.layout.warnings.iter().take(4).collect::<Vec<_>>()
                );
            }
        }
    }

    #[test]
    fn bundled_skins_load() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/skins");
        for name in ["Dare", "Dare Night"] {
            let bytes = std::fs::read(dir.join(format!("{name}.skn"))).expect(name);
            let s = load_skin(name, &bytes).expect(name);
            assert!(
                s.layout.warnings.is_empty(),
                "{name}: {:?}",
                s.layout.warnings
            );
            assert_eq!((s.layout.main.w, s.layout.main.h), (640, 420), "{name}");
            let mut need = vec![
                HotSpot::Profile,
                HotSpot::PrevProf,
                HotSpot::NextProf,
                HotSpot::ProfText,
                HotSpot::ProfBox,
                HotSpot::Spectate,
                HotSpot::ShipBox,
                HotSpot::Zones,
                HotSpot::Trace,
                HotSpot::ZoneText,
                HotSpot::ZoneBox,
                HotSpot::ZoneUp,
                HotSpot::ZoneDown,
                HotSpot::Ping,
                HotSpot::InfoBox,
                HotSpot::Options,
                HotSpot::KeyDefs,
                HotSpot::Macros,
                HotSpot::Play,
                HotSpot::Quit,
            ];
            need.extend((1..=8).map(HotSpot::Ship));
            for h in &need {
                assert!(s.layout.first_rect(h).is_some(), "{name} lacks {h:?}");
            }
            let pt = s.layout.first_rect(&HotSpot::ProfText).unwrap();
            assert!(pt.w >= 184 && pt.h >= 64, "{name} proftext {pt:?}");
            let c = &s.layout.colors;
            assert!(c.zone.iter().all(Option::is_some), "{name}");
            assert!(
                c.zone_sel[0].is_some() && c.profile.iter().all(Option::is_some),
                "{name}"
            );
        }
    }

    #[test]
    fn huge_numbers_never_panic() {
        let l = parse_layout(
            "main=(0,0,9,9)\nplay=(0,0,4294967295,10)\nquit=(4294967290,4294967290,4294967295,4294967295)+(1,1)",
        )
        .unwrap();
        assert!(
            l.first_rect(&HotSpot::Play).is_none(),
            "width overflows u32"
        );
        let mut d = bmp(50, 50);
        d.extend_from_slice(
            b"main=(0,0,39,39)\nproftext=(0,4294967290,10,4294967294)\ninfobox=(10,10,20,20)",
        );
        let s = load_skin("t", &d).unwrap();
        assert!(
            s.layout.first_rect(&HotSpot::ProfText).is_none(),
            "outside the image"
        );
        assert!(s.layout.first_rect(&HotSpot::InfoBox).is_some());
        assert!(!s.layout.warnings.is_empty());
    }

    #[test]
    fn load_skin_rejects_non_bmp_and_huge() {
        assert!(matches!(
            load_skin("t", b"PK\x03\x04"),
            Err(SkinError::NotBmp)
        ));
        assert!(matches!(
            load_skin("t", &vec![0u8; 33 * 1024 * 1024]),
            Err(SkinError::TooLarge)
        ));
        assert!(matches!(load_skin("t", b""), Err(SkinError::Empty)));
    }
}
