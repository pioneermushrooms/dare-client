//! The settings file: video, audio, accessibility, networking, the player's name,
//! key and gamepad bindings, macro keys and directory servers.
//!
//! A small INI file (`oss-client.ini` in the user's config folder, or `--config`).
//! Missing entries keep their defaults, unknown or bad lines are reported and skipped,
//! and the file is created with every default written out (as a reference) the first
//! time the game runs. Command-line flags override it for that run, and are never
//! written into it.
//!
//! ```ini
//! [video]
//! scale = 0          ; zoom, 1-8 in steps (1.25, 1.5...); 0 = from the display
//! [accessibility]
//! palette = standard ; or deuteranopia, protanopia, tritanopia, high_contrast
//! [keys]
//! gun = NoShift+ControlLeft, NoShift+ControlRight
//! mine = Shift+Tab
//! [macros]
//! Ctrl+F1 = 'on my way to %coord
//! ```

use std::path::{Path, PathBuf};

use oss_session::security::{MechanismPolicy, MechanismRegistry};
use winit::keyboard::KeyCode;

use crate::game::input::{Action, Binding, Bindings, Mods, PadBindings, PadButton};

pub const FILE_NAME: &str = "oss-client.ini";

/// The directory servers a fresh install asks for zones.
///
/// Measured on 2026-11-18, four rounds of queries through this client's own
/// directory code, and checked against the community's live status page and
/// the directories a current open-source zone server tells zones to register
/// with -- the same four in all three:
///
/// - `sscentral.trenchwars.org`: answered every time, with all 22 zones. One of
///   the two directories real Continuum has built in. First, for the full list.
/// - `sscentral.sscuservers.net`: answered every time, the fastest of the
///   reliable ones, and the other one built into Continuum -- but it listed 17
///   zones, missing five, among them the busiest zone in the game that day.
/// - `directory.poid.be`: all 22 and the fastest of all, but on and off: it
///   answered twice, then turned everyone away.
/// - `subspace.gamespec.org`: answered every time with 6 zones the others also
///   list. Kept as a fourth independent operator, for the day the first two
///   are both down.
///
/// **Directories do not all hold the same list**, which is why the client asks
/// every one of them at once and merges what comes back, rather than settling
/// on whichever answered. One that is down costs nothing but a note.
///
/// `ssdir.playsubspace.com` and `zones.sscentral.com`, shipped before, are gone:
/// neither answered a single query. The second's domain does not resolve, and
/// looking it up takes several seconds to fail.
///
/// Port 4990 is the client-facing one; zones register on 4991.
pub const DEFAULT_DIRECTORIES: [&str; 4] = [
    "sscentral.trenchwars.org:4990",
    "sscentral.sscuservers.net:4990",
    "directory.poid.be:4990",
    "subspace.gamespec.org:4990",
];

#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    /// World zoom: screen pixels to each game pixel, one of [`ZOOM_STEPS`];
    /// `None` picks a whole number from the display.
    pub scale: Option<f32>,
    pub vsync: bool,
    pub fullscreen: bool,
    pub mute: bool,
    /// Master volume, 0-100.
    pub volume: u32,
    /// Default player name online.
    pub name: Option<String>,
    pub bindings: Bindings,
    /// Macro keys: binding and the chat line they send.
    pub macros: Vec<(Binding, String)>,
    /// Accessibility: colour palette.
    pub palette: crate::game::palette::PaletteKind,
    /// The HUD, the radar, chat and the rest of what is drawn over the world.
    pub view: GameView,
    /// Gamepad buttons per action (the left stick always steers).
    pub pad: PadBindings,
    /// Directory servers for the zone browser ("host:port").
    /// [`DEFAULT_DIRECTORIES`] unless the settings file says otherwise.
    pub directories: Vec<String>,
    /// Connection encryption, as the file says it: `auto`, or the name of one
    /// mechanism. Kept as the word, and read against this build's mechanisms
    /// only when it is applied, so a word this build does not know is still in
    /// the file after a Save.
    pub security: String,
    /// Offer zones an OpenSec identity key (one per zone) they can use for bans.
    pub identity: bool,
    /// The start screen's skin, by name.
    pub skin: String,
    /// Extra folders to look for skins in.
    pub skin_folders: Vec<PathBuf>,
    /// Enter opens the chat line; with it closed, letters drive the voice menu (V).
    /// Off: Continuum's way, every character types.
    pub enter_to_chat: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            scale: None,
            vsync: false,
            fullscreen: false,
            mute: false,
            volume: 60,
            name: None,
            bindings: Bindings::default(),
            macros: Vec::new(),
            palette: Default::default(),
            view: GameView::default(),
            pad: PadBindings::default(),
            directories: DEFAULT_DIRECTORIES.iter().map(|&s| s.to_owned()).collect(),
            security: oss_session::security::AUTO.to_owned(),
            identity: true,
            skin: crate::game::skin::DEFAULT_SKIN.to_owned(),
            skin_folders: Vec::new(),
            enter_to_chat: true,
        }
    }
}

/// The zooms the settings window and the zoom keys step through: eighths up to
/// 2, quarters up to 4, then whole numbers. Each is a whole number of screen
/// pixels for a 16-pixel tile, so the map's tiles stay the same size as each
/// other at every one of them.
pub const ZOOM_STEPS: [f32; 19] = [
    1.0, 1.125, 1.25, 1.375, 1.5, 1.625, 1.75, 1.875, 2.0, 2.25, 2.5, 2.75, 3.0, 3.25, 3.5, 3.75,
    4.0, 6.0, 8.0,
];

/// The zoom step nearest `v`, so a value typed into the file lands on one.
pub fn nearest_zoom(v: f32) -> f32 {
    ZOOM_STEPS
        .iter()
        .copied()
        .min_by(|a, b| (a - v).abs().total_cmp(&(b - v).abs()))
        .unwrap_or(1.0)
}

/// The zoom one step in (`dir` 1) or out (-1) from `v`, stopping at the ends.
pub fn step_zoom(v: f32, dir: i32) -> f32 {
    let i = ZOOM_STEPS
        .iter()
        .position(|z| *z == nearest_zoom(v))
        .unwrap_or(0) as i32;
    ZOOM_STEPS[(i + dir).clamp(0, ZOOM_STEPS.len() as i32 - 1) as usize]
}

/// What a ship's label shows beside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ShipNames {
    /// Name and bounty, as Continuum shows them.
    #[default]
    NameAndBounty,
    /// The name alone.
    Name,
    /// No labels.
    Off,
}

impl ShipNames {
    pub const ALL: [ShipNames; 3] = [ShipNames::NameAndBounty, ShipNames::Name, ShipNames::Off];

    pub fn name(self) -> &'static str {
        match self {
            ShipNames::NameAndBounty => "name_and_bounty",
            ShipNames::Name => "name",
            ShipNames::Off => "off",
        }
    }

    pub fn from_name(s: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|n| n.name().eq_ignore_ascii_case(s.trim()))
    }
}

/// What the frame-rate line shows when the game starts; F12 still cycles it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FrameRate {
    #[default]
    Off,
    /// Frames per second and the ping.
    On,
    /// That, and the renderer's own figures.
    Detail,
}

impl FrameRate {
    pub const ALL: [FrameRate; 3] = [FrameRate::Off, FrameRate::On, FrameRate::Detail];

    pub fn name(self) -> &'static str {
        match self {
            FrameRate::Off => "off",
            FrameRate::On => "on",
            FrameRate::Detail => "detail",
        }
    }

    pub fn from_name(s: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|n| n.name().eq_ignore_ascii_case(s.trim()))
    }
}

/// The game's own display settings: the HUD, the radar, chat and labels.
///
/// Percentages are of the size the game would pick by itself, so 100 is
/// always what it looked like before the setting existed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GameView {
    /// The HUD, chat, menus and every other piece of text, in percent.
    pub hud_size: u32,
    /// The small radar's side, in percent.
    pub radar_size: u32,
    /// How close the radar's map is, in percent: 200 shows half as much.
    pub radar_zoom: u32,
    pub ship_names: ShipNames,
    /// Chat lines under the world; `None` lets the screen height decide.
    pub chat_lines: Option<u32>,
    /// The sender column of the chat log, in characters (`?namelen`).
    pub name_width: u32,
    pub frame_rate: FrameRate,
    /// Voice lines and chat sounds, 0-100, under the master volume.
    pub voice_volume: u32,
}

impl Default for GameView {
    fn default() -> Self {
        Self {
            hud_size: 100,
            radar_size: 100,
            radar_zoom: 100,
            ship_names: ShipNames::default(),
            chat_lines: None,
            name_width: crate::game::chat::DEFAULT_NAMELEN as u32,
            frame_rate: FrameRate::default(),
            voice_volume: 100,
        }
    }
}

/// The ranges of the [`GameView`] numbers: least, most and one step.
pub const HUD_SIZE: (u32, u32, u32) = (50, 300, 25);
pub const RADAR_SIZE: (u32, u32, u32) = (50, 250, 25);
pub const RADAR_ZOOM: (u32, u32, u32) = (25, 400, 25);
/// The most chat lines a setting asks for (the log's own ceiling).
pub const CHAT_LINES_MAX: u32 = 20;
/// The widest sender column (`?namelen` stops there too).
pub const NAME_WIDTH_MAX: u32 = 24;

/// The settings folder: `%APPDATA%\OpenSubSpace` on Windows, else
/// `$XDG_CONFIG_HOME/open-subspace` or `~/.config/open-subspace`.
pub fn default_dir() -> Option<PathBuf> {
    if cfg!(windows) {
        std::env::var_os("APPDATA").map(|d| PathBuf::from(d).join("OpenSubSpace"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .map(|d| d.join("open-subspace"))
    }
}

impl Config {
    /// Loads `path`. A missing file at the default location is created with the
    /// defaults; a missing file given explicitly is an error, because a mistyped
    /// `--config` should not quietly mint a second profile somewhere else. Returns
    /// the settings and a warning per line that couldn't be used.
    ///
    /// `mechanisms` are this build's, for the comment a new file is written with.
    pub fn load(
        path: &Path,
        explicit: bool,
        mechanisms: &MechanismRegistry,
    ) -> Result<(Config, Vec<String>), String> {
        match std::fs::read(path) {
            Ok(bytes) => Ok(Config::parse(&String::from_utf8_lossy(&bytes))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && !explicit => {
                let config = Config::default();
                let written = path
                    .parent()
                    .map_or(Ok(()), std::fs::create_dir_all)
                    .and_then(|_| std::fs::write(path, config.to_text(mechanisms)));
                match written {
                    Ok(()) => log::info!("wrote default settings to {}", path.display()),
                    Err(e) => log::warn!("couldn't write {}: {e}", path.display()),
                }
                Ok((config, Vec::new()))
            }
            // Only an explicit path reaches this, since the arm above takes the
            // default one. Saying the rule here is the point: whoever meets it has
            // just typed a path and been handed `os error 2` -- or `os error 3`,
            // which is the folder missing rather than the file.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(format!(
                "{}: {e}. --config names a settings file to read; only the \
                 default one is written when it is missing. Create it first -- \
                 an empty file is enough.",
                path.display()
            )),
            Err(e) => Err(format!("{}: {e}", path.display())),
        }
    }

    /// Browser play's settings: `local` is the whole file as this browser
    /// last saved it for the pilot, and `account` the keys and macros the
    /// pilot's account holds, which win over the browser's where it has any.
    /// Either may be empty. The warnings say which of the two each came from.
    pub fn for_browser(local: &str, account: &str) -> (Config, Vec<String>) {
        let (mut c, warnings) = Config::parse(local);
        let mut warnings: Vec<String> = warnings
            .into_iter()
            .map(|w| format!("browser settings: {w}"))
            .collect();
        if !account.trim().is_empty() {
            let (pilot, more) = Config::parse(account);
            warnings.extend(more.into_iter().map(|w| format!("pilot settings: {w}")));
            c.bindings = pilot.bindings;
            c.macros = pilot.macros;
        }
        (c, warnings)
    }

    pub fn parse(text: &str) -> (Config, Vec<String>) {
        let mut c = Config::default();
        let mut warnings = Vec::new();
        let mut section = String::new();
        // Whether the file binds `spectate` itself; see [`settle_spectate`].
        let mut spectate_listed = false;
        for (n, raw) in text.lines().enumerate() {
            let whole = raw.trim();
            if whole.is_empty() || whole.starts_with(';') || whole.starts_with('#') {
                continue;
            }
            // Macro lines are chat text, where ';' and '#' mean channels and squads.
            let line = if section == "macros" && !whole.starts_with('[') {
                whole
            } else {
                strip_comment(raw).trim()
            };
            if line.is_empty() {
                continue;
            }
            let mut warn = |msg: String| warnings.push(format!("line {}: {msg}", n + 1));
            if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                section = name.trim().to_ascii_lowercase();
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                warn(format!("expected `name = value`, got `{line}`"));
                continue;
            };
            let (key, value) = (key.trim().to_ascii_lowercase(), value.trim());
            let result = match (section.as_str(), key.as_str()) {
                ("video", "scale") => zoom(value).map(|z| c.scale = z),
                ("video", "vsync") => flag(value).map(|b| c.vsync = b),
                ("video", "fullscreen") => flag(value).map(|b| c.fullscreen = b),
                ("video", "skin") => {
                    c.skin = if value.is_empty() {
                        crate::game::skin::DEFAULT_SKIN.to_owned()
                    } else {
                        value.to_owned()
                    };
                    Ok(())
                }
                ("skins", "folders") => {
                    c.skin_folders = value
                        .split(',')
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(PathBuf::from)
                        .collect();
                    Ok(())
                }
                ("audio", "mute") => flag(value).map(|b| c.mute = b),
                ("chat", "enter_to_chat") => flag(value).map(|b| c.enter_to_chat = b),
                ("audio", "volume") => number(value, 0, 100).map(|n| c.volume = n),
                ("accessibility", "palette") => crate::game::palette::PaletteKind::from_name(value)
                    .map(|p| c.palette = p)
                    .ok_or_else(|| format!("unknown palette `{value}`")),
                // Written before the HUD had a size of its own: each step was
                // one more whole pixel scale, which is about half as large again.
                ("accessibility", "ui_scale") => {
                    number(value, 0, 4).map(|n| c.view.hud_size = (100 + 50 * n).min(HUD_SIZE.1))
                }
                ("hud", "size") => stepped(value, HUD_SIZE).map(|n| c.view.hud_size = n),
                ("hud", "radar_size") => stepped(value, RADAR_SIZE).map(|n| c.view.radar_size = n),
                ("hud", "radar_zoom") => stepped(value, RADAR_ZOOM).map(|n| c.view.radar_zoom = n),
                ("hud", "ship_names") => ShipNames::from_name(value)
                    .map(|n| c.view.ship_names = n)
                    .ok_or_else(|| format!("unknown ship_names `{value}`")),
                ("hud", "frame_rate") => FrameRate::from_name(value)
                    .map(|f| c.view.frame_rate = f)
                    .ok_or_else(|| format!("unknown frame_rate `{value}`")),
                ("chat", "lines") => number(value, 0, CHAT_LINES_MAX)
                    .map(|n| c.view.chat_lines = (n > 0).then_some(n)),
                ("chat", "name_width") => {
                    number(value, 1, NAME_WIDTH_MAX).map(|n| c.view.name_width = n)
                }
                ("audio", "voice_volume") => number(value, 0, 100).map(|n| c.view.voice_volume = n),
                ("network", "identity") => flag(value).map(|b| c.identity = b),
                // Kept as written: which words are good depends on the mechanisms
                // the build has, and a word this build does not know is not this
                // build's to throw away.
                ("network", "security") => {
                    c.security = value.trim().to_owned();
                    Ok(())
                }
                ("directory", "servers") => {
                    // Every settings file written before this key had defaults
                    // carries `servers =` with nothing after it, because
                    // nothing was the default. Reading that as "no directory
                    // servers" would leave precisely the players who have run
                    // the client before -- the only ones who have a settings
                    // file at all -- still staring at the empty zone browser
                    // these defaults exist to fill. So an empty value means
                    // the built-in list, and a player who genuinely wants no
                    // zone browser says so in a word.
                    let listed: Vec<String> = value
                        .split(',')
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_string)
                        .collect();
                    c.directories = match listed.as_slice() {
                        [] => DEFAULT_DIRECTORIES.iter().map(|&s| s.to_owned()).collect(),
                        [one] if one.eq_ignore_ascii_case("none") => Vec::new(),
                        _ => listed,
                    };
                    Ok(())
                }
                ("player", "name") => {
                    c.name = (!value.is_empty()).then(|| value.to_string());
                    // The name is the start-up screen's, remembered in
                    // `login.ini`. This key is only the seed for a run that has
                    // never been through that screen, so a reader who sets it
                    // and sees nothing change is told why rather than left to
                    // work it out. It used to have a box in the options as well,
                    // which made two places setting one thing and only one of
                    // them winning.
                    warn(
                        "player name is set on the start-up screen and remembered \
                         there; this key only seeds the first run"
                            .to_string(),
                    );
                    Ok(())
                }
                ("macros", _) => {
                    // The key keeps its case-insensitive name; the text keeps its case.
                    let key_text = line.split_once('=').map_or("", |(k, _)| k.trim());
                    parse_binding(key_text).map(|b| {
                        if !value.is_empty() {
                            c.macros.push((b, value.to_string()));
                        }
                    })
                }
                ("gamepad", action) => match Action::from_name(action) {
                    Some(a) => value
                        .split(',')
                        .map(str::trim)
                        .filter(|b| !b.is_empty())
                        .map(|b| {
                            PadButton::from_name(b).ok_or_else(|| format!("unknown button `{b}`"))
                        })
                        .collect::<Result<Vec<_>, _>>()
                        .map(|buttons| c.pad.set(a, buttons)),
                    None => Err(format!("unknown action `{action}`")),
                },
                ("keys", action) => match Action::from_name(action) {
                    Some(a) => {
                        spectate_listed |= a == Action::Spectate;
                        parse_bindings(value).map(|b| c.bindings.set(a, b))
                    }
                    None => Err(format!("unknown action `{action}`")),
                },
                _ => Err(format!("unknown setting `{key}` in [{section}]")),
            };
            if let Err(e) = result {
                warn(e);
            }
        }
        if !spectate_listed {
            settle_spectate(&mut c.bindings);
        }
        (c, warnings)
    }

    /// The whole file, with comments; parsing it gives back `self`. The comment
    /// on `security` lists the words `mechanisms` accept.
    pub fn to_text(&self, mechanisms: &MechanismRegistry) -> String {
        let mut s = String::from(
            "; oss-client settings. Delete a line to get its default back.\n\
             ; Command-line flags override these for one run, and are never saved here.\n\n[video]\n",
        );
        s += &format!(
            "scale = {}          ; zoom: screen pixels per game pixel, 1-8 (1.25, 1.5...), 0 = from the display\n",
            self.scale.unwrap_or(0.0)
        );
        s += &format!("vsync = {}\nfullscreen = {}\n", self.vsync, self.fullscreen);
        s += &format!(
            "skin = {}           ; start screen skin (Cmd-K / F7 cycles them)\n\n",
            self.skin
        );
        s += &format!(
            "[audio]\nmute = {}\nvolume = {}         ; 0-100\n\
             voice_volume = {}  ; voice lines and chat sounds, 0-100 of the volume\n\n",
            self.mute, self.volume, self.view.voice_volume
        );
        let v = &self.view;
        s += &format!(
            "[hud]\n\
             ; Percent of the size the game picks by itself.\n\
             size = {}         ; text, gauges, chat and menus, {}-{}\n\
             radar_size = {}   ; {}-{}\n\
             radar_zoom = {}   ; {}-{}: 200 shows half as much of the map\n\
             ship_names = {}   ; name_and_bounty, name or off\n\
             frame_rate = {}   ; at the start: off, on or detail (F12 cycles it)\n\n",
            v.hud_size,
            HUD_SIZE.0,
            HUD_SIZE.1,
            v.radar_size,
            RADAR_SIZE.0,
            RADAR_SIZE.1,
            v.radar_zoom,
            RADAR_ZOOM.0,
            RADAR_ZOOM.1,
            v.ship_names.name(),
            v.frame_rate.name(),
        );
        s += &format!(
            "[chat]\n\
             ; yes: Enter opens the chat line, and V opens the voice menu. no: every\n\
             ; key types straight into chat, as in Continuum.\n\
             enter_to_chat = {}\n\
             lines = {}          ; chat lines shown, 0 = from the screen's height\n\
             name_width = {}     ; the sender column, 1-24 characters\n\n",
            self.enter_to_chat,
            self.view.chat_lines.unwrap_or(0),
            self.view.name_width,
        );
        s += &format!(
            "[accessibility]\n\
             palette = {}  ; standard, deuteranopia, protanopia, tritanopia, high_contrast\n\n",
            self.palette.name(),
        );
        let order = MechanismPolicy::automatic(mechanisms).automatic_line(mechanisms);
        let names: Vec<&str> = mechanisms.ids().map(|id| id.as_str()).collect();
        s += &format!(
            "[network]\n\
             ; auto tries each mechanism in turn, strongest first, stepping down for a\n\
             ; server that does not answer (never once a server's key is on record).\n\
             ; {order}.\n\
             ; Or name one to use it alone: {}.\n\
             security = {}\n\
             ; Offer each zone an OpenSec identity key of its own (kept in\n\
             ; identities.txt), which zones may use for bans.\n\
             identity = {}\n\n",
            names.join(", "),
            self.security,
            self.identity
        );
        s += &format!(
            "[directory]\n\
             ; Directory servers for the zone browser (F3 on the login screen), as\n\
             ; host:port, comma separated. These are the game's own, and the zones\n\
             ; list themselves on them. One that is unreachable is skipped and the\n\
             ; rest are merged, so a directory having a bad day costs you nothing.\n\
             ; Empty brings this list back; `none` turns the zone browser off.\n\
             servers = {}\n\n",
            if self.directories.is_empty() {
                "none".to_owned()
            } else {
                self.directories.join(", ")
            }
        );
        s += &format!(
            "[skins]\n\
             ; Extra folders holding .skn skins, comma separated. The bundled skins,\n\
             ; the `skins` folder beside this file and a Continuum install's are\n\
             ; always searched.\n\
             folders = {}\n\n",
            self.skin_folders
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        );
        // No `[player] name`. The name is the start-up screen's and is
        // remembered in `login.ini`; a file that still carries the key is read
        // (and warned about) but it is never written again, because writing a
        // key while telling the reader not to set it is the two-sources fault
        // in another coat.
        s += "[keys]\n\
                ; action = key, key... Keys use their US-layout names (ArrowUp, ControlLeft,\n\
                ; KeyA, Digit1, F3, Tab, Backquote...). Prefixes: Shift+ and Ctrl+ need that\n\
                ; modifier, NoShift+ works only without Shift, and no prefix works either way.\n\
                ; Leave the value empty to unbind an action.\n";
        for a in Action::ALL {
            let keys: Vec<String> = self.bindings.get(a).iter().map(binding_name).collect();
            s += &format!("{} = {}\n", a.name(), keys.join(", "));
        }
        s += "\n[gamepad]\n\
              ; action = buttons: south east west north l1 r1 l2 r2 l3 r3 select start\n\
              ; dpad_up dpad_down dpad_left dpad_right. The left stick always steers.\n";
        for a in Action::ALL {
            let buttons: Vec<&str> = self.pad.get(a).iter().map(|b| b.name()).collect();
            if !buttons.is_empty() {
                s += &format!("{} = {}\n", a.name(), buttons.join(", "));
            }
        }
        s += "\n[macros]\n\
              ; key = chat line, sent when the key is pressed (not while typing). Same keys\n\
              ; and prefixes as [keys], e.g. `Ctrl+F1 = 'need help at base` or\n\
              ; `F9 = ;2;hello channel 2`. Comments here must be on their own line.\n";
        for (b, text) in &self.macros {
            s += &format!("{} = {text}\n", binding_name(b));
        }
        s
    }
}

fn strip_comment(line: &str) -> &str {
    match line.find([';', '#']) {
        Some(i) => &line[..i],
        None => line,
    }
}

fn flag(v: &str) -> Result<bool, String> {
    match v.to_ascii_lowercase().as_str() {
        "true" | "yes" | "on" | "1" => Ok(true),
        "false" | "no" | "off" | "0" => Ok(false),
        _ => Err(format!("expected true or false, got `{v}`")),
    }
}

/// `[video] scale`: 0 for the display's own, else a zoom from 1 to 8, put on
/// the nearest of [`ZOOM_STEPS`]. Whole numbers are what older files hold.
fn zoom(v: &str) -> Result<Option<f32>, String> {
    match v.parse::<f32>() {
        Ok(0.0) => Ok(None),
        Ok(z) if (1.0..=8.0).contains(&z) => Ok(Some(nearest_zoom(z))),
        _ => Err(format!("expected a zoom 1-8, or 0, got `{v}`")),
    }
}

/// A number in `range` (least, most, step), put on the nearest step.
fn stepped(v: &str, (min, max, step): (u32, u32, u32)) -> Result<u32, String> {
    number(v, min, max).map(|n| (n + step / 2) / step * step)
}

fn number(v: &str, min: u32, max: u32) -> Result<u32, String> {
    v.parse::<u32>()
        .ok()
        .filter(|n| (min..=max).contains(n))
        .ok_or_else(|| format!("expected a number {min}-{max}, got `{v}`"))
}

/// F11 moved from a second screenshot key to spectator mode, which is what it
/// is in Continuum. Every settings file written before that lists every action
/// with its keys, so it says `screenshot = PrintScreen, F11` and has no
/// `spectate` line: read as it stands, F11 would both take a screenshot and
/// spectate.
///
/// So for a file with no `spectate` line: a screenshot binding that is exactly
/// the old default loses F11, and if F11 is still bound to anything after that
/// -- the player put it there on purpose -- spectate starts unbound rather than
/// taking the key away from them.
fn settle_spectate(bindings: &mut Bindings) {
    let old_default = [
        Binding {
            key: KeyCode::PrintScreen,
            mods: Mods::Any,
        },
        Binding {
            key: KeyCode::F11,
            mods: Mods::Any,
        },
    ];
    if bindings.get(Action::Screenshot) == old_default {
        bindings.set(Action::Screenshot, old_default[..1].to_vec());
    }
    let f11_taken = Action::ALL
        .into_iter()
        .filter(|&a| a != Action::Spectate)
        .any(|a| bindings.get(a).iter().any(|b| b.key == KeyCode::F11));
    if f11_taken {
        bindings.set(Action::Spectate, Vec::new());
    }
}

fn parse_bindings(v: &str) -> Result<Vec<Binding>, String> {
    v.split(',')
        .map(str::trim)
        .filter(|k| !k.is_empty())
        .map(parse_binding)
        .collect()
}

fn parse_binding(s: &str) -> Result<Binding, String> {
    let (mods, key) = match s.split_once('+') {
        Some((m, k)) => {
            let mods = match m.trim().to_ascii_lowercase().as_str() {
                "shift" => Mods::Shift,
                "ctrl" => Mods::Ctrl,
                "noshift" => Mods::None,
                _ => return Err(format!("unknown modifier `{m}` in `{s}`")),
            };
            (mods, k.trim())
        }
        None => (Mods::Any, s),
    };
    let key = key_from_name(key).ok_or_else(|| format!("unknown key `{key}`"))?;
    Ok(Binding { key, mods })
}

fn binding_name(b: &Binding) -> String {
    let prefix = match b.mods {
        Mods::Any => "",
        Mods::None => "NoShift+",
        Mods::Shift => "Shift+",
        Mods::Ctrl => "Ctrl+",
    };
    format!("{prefix}{:?}", b.key)
}

pub(crate) fn key_from_name(name: &str) -> Option<KeyCode> {
    KEYS.iter()
        .copied()
        .find(|k| format!("{k:?}").eq_ignore_ascii_case(name))
}

/// Keys that can be bound, by their winit names.
const KEYS: &[KeyCode] = {
    use KeyCode::*;
    &[
        KeyA,
        KeyB,
        KeyC,
        KeyD,
        KeyE,
        KeyF,
        KeyG,
        KeyH,
        KeyI,
        KeyJ,
        KeyK,
        KeyL,
        KeyM,
        KeyN,
        KeyO,
        KeyP,
        KeyQ,
        KeyR,
        KeyS,
        KeyT,
        KeyU,
        KeyV,
        KeyW,
        KeyX,
        KeyY,
        KeyZ,
        Digit0,
        Digit1,
        Digit2,
        Digit3,
        Digit4,
        Digit5,
        Digit6,
        Digit7,
        Digit8,
        Digit9,
        F1,
        F2,
        F3,
        F4,
        F5,
        F6,
        F7,
        F8,
        F9,
        F10,
        F11,
        F12,
        ArrowUp,
        ArrowDown,
        ArrowLeft,
        ArrowRight,
        ShiftLeft,
        ShiftRight,
        ControlLeft,
        ControlRight,
        AltLeft,
        AltRight,
        SuperLeft,
        SuperRight,
        Space,
        Enter,
        Tab,
        Escape,
        Backspace,
        Insert,
        Delete,
        Home,
        End,
        PageUp,
        PageDown,
        CapsLock,
        Backquote,
        Minus,
        Equal,
        BracketLeft,
        BracketRight,
        Backslash,
        Semicolon,
        Quote,
        Comma,
        Period,
        Slash,
        Numpad0,
        Numpad1,
        Numpad2,
        Numpad3,
        Numpad4,
        Numpad5,
        Numpad6,
        Numpad7,
        Numpad8,
        Numpad9,
        NumpadAdd,
        NumpadSubtract,
        NumpadMultiply,
        NumpadDivide,
        NumpadDecimal,
        NumpadEnter,
        NumLock,
        ScrollLock,
        Pause,
        PrintScreen,
        ContextMenu,
    ]
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_file_round_trips() {
        let c = Config::default();
        let (back, warnings) = Config::parse(&c.to_text(&MechanismRegistry::builtin()));
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(back, c);
        let custom = Config {
            scale: Some(1.375),
            vsync: true,
            mute: true,
            volume: 25,
            palette: crate::game::palette::PaletteKind::Tritanopia,
            view: GameView {
                hud_size: 150,
                radar_size: 75,
                radar_zoom: 200,
                ship_names: ShipNames::Name,
                chat_lines: Some(7),
                name_width: 14,
                frame_rate: FrameRate::Detail,
                voice_volume: 40,
            },
            pad: {
                let mut p = PadBindings::default();
                p.set(Action::Gun, vec![PadButton::R2, PadButton::South]);
                p
            },
            directories: vec!["127.0.0.1:4990".into(), "localhost:4991".into()],
            security: "opensec".into(),
            identity: false,
            ..Config::default()
        };
        assert_eq!(
            Config::parse(&custom.to_text(&MechanismRegistry::builtin())).0,
            custom
        );
    }

    /// Zooms from files written when the scale was a whole number still read,
    /// and anything between the steps lands on the nearest one.
    #[test]
    fn zoom_reads_old_whole_numbers_and_lands_on_a_step() {
        let read = |t: &str| Config::parse(&format!("[video]\nscale = {t}\n"));
        assert_eq!(read("0").0.scale, None);
        assert_eq!(read("3").0.scale, Some(3.0));
        assert_eq!(read("1.5").0.scale, Some(1.5));
        assert_eq!(read("1.3").0.scale, Some(1.25));
        assert!(!read("9").1.is_empty(), "past the top is reported");
        assert_eq!(step_zoom(1.0, -1), 1.0, "the bottom stops");
        assert_eq!(step_zoom(1.0, 1), 1.125);
        assert_eq!(step_zoom(4.0, 1), 6.0);
        assert_eq!(step_zoom(8.0, 1), 8.0, "and so does the top");
        // Every step draws a 16-pixel tile a whole number of pixels wide.
        for z in ZOOM_STEPS {
            assert_eq!((16.0 * z).fract(), 0.0, "{z}");
        }
    }

    /// The old interface size becomes a HUD size, and a bad value is reported
    /// rather than read.
    #[test]
    fn hud_settings_read_and_old_interface_size_carries_over() {
        let (c, w) = Config::parse("[accessibility]\nui_scale = 2\n");
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(c.view.hud_size, 200);
        let (c, w) = Config::parse("[hud]\nsize = 130\nship_names = sideways\n");
        assert_eq!(c.view.hud_size, 125, "on the nearest step");
        assert_eq!(c.view.ship_names, ShipNames::NameAndBounty);
        assert_eq!(w.len(), 1, "{w:?}");
    }

    /// In the browser the zoom and the HUD come from the browser's copy, and
    /// keys and macros from the account when it has some.
    #[test]
    fn browser_settings_take_keys_from_the_account() {
        let local = Config {
            scale: Some(1.5),
            view: GameView {
                hud_size: 125,
                ..GameView::default()
            },
            macros: vec![(parse_binding("F5").unwrap(), "local".into())],
            ..Config::default()
        }
        .to_text(&MechanismRegistry::builtin());
        let (c, w) = Config::for_browser(&local, "");
        assert!(w.is_empty(), "{w:?}");
        assert_eq!((c.scale, c.view.hud_size), (Some(1.5), 125));
        assert_eq!(c.macros[0].1, "local", "no account keys: the browser's");
        let (c, _) = Config::for_browser(&local, "[macros]\nF6 = from account\n");
        assert_eq!(c.scale, Some(1.5));
        assert_eq!(c.macros.len(), 1);
        assert_eq!(c.macros[0].1, "from account");
        let (fresh, _) = Config::for_browser("", "");
        assert_eq!(fresh, Config::default());
    }

    #[test]
    fn enter_to_chat_defaults_on_and_round_trips() {
        assert!(Config::default().enter_to_chat);
        let (c, warnings) = Config::parse("[chat]\nenter_to_chat = no\n");
        assert!(warnings.is_empty(), "{warnings:?}");
        assert!(!c.enter_to_chat);
        assert_eq!(
            Config::parse(&c.to_text(&MechanismRegistry::builtin())).0,
            c
        );
    }

    /// A player who has just installed the client must have somewhere to play
    /// without editing a file first. The browser had nothing to ask, so it
    /// told them to go and add a directory server by hand before they could
    /// see a single zone.
    #[test]
    fn a_fresh_install_has_directory_servers() {
        let c = Config::default();
        assert_eq!(c.directories, DEFAULT_DIRECTORIES, "out of the box");
        for dir in DEFAULT_DIRECTORIES {
            assert!(
                c.to_text(&MechanismRegistry::builtin()).contains(dir),
                "and the file written on the first run names {dir}"
            );
        }
        // The two that answered nothing on 2026-11-18 are not shipped.
        for gone in ["ssdir.playsubspace.com", "zones.sscentral.com"] {
            assert!(
                !c.to_text(&MechanismRegistry::builtin()).contains(gone),
                "{gone} is still listed"
            );
        }
    }

    /// Every settings file written before this key had defaults carries
    /// `servers =` with nothing after it. Those players -- the only ones with
    /// a settings file at all -- are exactly the ones stuck with an empty zone
    /// browser, so an empty value has to mean the built-in list rather than
    /// "the player chose none".
    #[test]
    fn an_old_empty_list_picks_the_defaults_up_and_none_opts_out() {
        let (old, warnings) = Config::parse("[directory]\nservers =\n");
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(
            old.directories, DEFAULT_DIRECTORIES,
            "an old file catches up"
        );

        let (off, warnings) = Config::parse("[directory]\nservers = none\n");
        assert!(warnings.is_empty(), "{warnings:?}");
        assert!(off.directories.is_empty(), "and `none` is how you opt out");

        // ... which has to survive being written back out, or the opt-out
        // would last exactly until the next save.
        assert!(
            off.to_text(&MechanismRegistry::builtin())
                .contains("servers = none")
        );
        assert!(
            Config::parse(&off.to_text(&MechanismRegistry::builtin()))
                .0
                .directories
                .is_empty()
        );

        let (mine, _) = Config::parse("[directory]\nservers = dir.example:4990\n");
        assert_eq!(mine.directories, ["dir.example:4990"], "a choice is kept");
    }

    /// The player name is read for an old file and never written back: its home
    /// is the start-up screen, which remembers it in `login.ini`. Two places
    /// setting one thing is the fault this removed, and a file that is still
    /// written with the key would be that fault in another coat.
    #[test]
    fn the_player_name_is_read_once_and_never_written() {
        let (c, warnings) = Config::parse("[player]\nname = Pilot One\n");
        assert_eq!(
            c.name.as_deref(),
            Some("Pilot One"),
            "an old file still loads"
        );
        assert_eq!(warnings.len(), 1, "and is told where the name really lives");
        assert!(
            !c.to_text(&MechanismRegistry::builtin())
                .contains("[player]"),
            "but the key is not written again"
        );
        let (back, warnings) = Config::parse(&c.to_text(&MechanismRegistry::builtin()));
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(back.name, None, "so the seed is spent, not kept in sync");
    }

    #[test]
    fn rebinds_unbinds_and_reports_bad_lines() {
        let text = "\
[video]
scale = 2
fullscreen = maybe
[keys]
gun = Space, ctrl+KeyF   ; two keys
mine =
thor = Hyper+F6
warpdrive = F9
[sound]
loud = yes
";
        let (c, warnings) = Config::parse(text);
        assert_eq!(c.scale, Some(2.0));
        assert!(!c.fullscreen);
        assert_eq!(
            c.bindings.get(Action::Gun),
            &[
                Binding {
                    key: KeyCode::Space,
                    mods: Mods::Any
                },
                Binding {
                    key: KeyCode::KeyF,
                    mods: Mods::Ctrl
                }
            ]
        );
        assert!(c.bindings.get(Action::Mine).is_empty());
        // The bad thor line leaves its default alone.
        assert_eq!(
            c.bindings.get(Action::Thor),
            Bindings::default().get(Action::Thor)
        );
        assert_eq!(warnings.len(), 4, "{warnings:?}");
        assert!(warnings[0].starts_with("line 3:"));
    }

    /// A settings file from before F11 became spectator mode keeps working:
    /// F11 spectates and no longer also takes a screenshot. And a player who
    /// bound F11 to something themselves keeps it.
    #[test]
    fn an_old_settings_file_hands_f11_to_spectator_mode() {
        let f11 = |mods| Binding {
            key: KeyCode::F11,
            mods,
        };
        let print = Binding {
            key: KeyCode::PrintScreen,
            mods: Mods::Any,
        };
        let (c, _) = Config::parse(
            "[keys]
screenshot = PrintScreen, F11
",
        );
        assert_eq!(c.bindings.get(Action::Screenshot), &[print]);
        assert_eq!(c.bindings.get(Action::Spectate), &[f11(Mods::Any)]);

        // Their own use of F11 wins, and spectate waits to be given a key.
        let (c, _) = Config::parse(
            "[keys]
thor = F11
",
        );
        assert_eq!(c.bindings.get(Action::Thor), &[f11(Mods::Any)]);
        assert!(c.bindings.get(Action::Spectate).is_empty());

        // A file that names spectate is taken at its word.
        let (c, _) = Config::parse(
            "[keys]
screenshot = PrintScreen, F11
spectate = F10
",
        );
        assert_eq!(c.bindings.get(Action::Screenshot), &[print, f11(Mods::Any)]);

        // And a fresh file round-trips with spectate on F11 alone.
        let (back, w) = Config::parse(&Config::default().to_text(&MechanismRegistry::builtin()));
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(back.bindings.get(Action::Spectate), &[f11(Mods::Any)]);
        assert_eq!(back.bindings.get(Action::Screenshot), &[print]);
    }

    #[test]
    fn macros_keep_semicolons_and_round_trip() {
        let text = "[macros]\n; a comment line\nCtrl+F1 = 'need help ; now\nF9 = ;2;hi #squad\nbadkey = x\n";
        let (c, warnings) = Config::parse(text);
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert_eq!(
            c.macros,
            vec![
                (
                    Binding {
                        key: KeyCode::F1,
                        mods: Mods::Ctrl
                    },
                    "'need help ; now".to_string()
                ),
                (
                    Binding {
                        key: KeyCode::F9,
                        mods: Mods::Any
                    },
                    ";2;hi #squad".to_string()
                ),
            ]
        );
        assert_eq!(
            Config::parse(&c.to_text(&MechanismRegistry::builtin()))
                .0
                .macros,
            c.macros
        );
    }

    #[test]
    fn creates_the_default_file_once() {
        let dir = std::env::temp_dir().join(format!("oss-config-{}", std::process::id()));
        let path = dir.join(FILE_NAME);
        let _ = std::fs::remove_dir_all(&dir);
        assert!(Config::load(&path, true, &MechanismRegistry::builtin()).is_err());
        let (c, _) = Config::load(&path, false, &MechanismRegistry::builtin()).unwrap();
        assert_eq!(c, Config::default());
        std::fs::write(&path, "[audio]\nmute = yes\n").unwrap();
        assert!(
            Config::load(&path, false, &MechanismRegistry::builtin())
                .unwrap()
                .0
                .mute
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The rule itself is pinned by the test above. This pins the *explanation*:
    /// the OS reports a missing explicit file as `os error 2` (or `os error 3`
    /// when the folder is what is missing), which states no rule at all, and
    /// having to rediscover the rule from that is why this came back as an item.
    #[test]
    fn a_missing_explicit_file_says_what_to_do_about_it() {
        let dir = std::env::temp_dir().join(format!("oss-config-say-{}", std::process::id()));
        let path = dir.join(FILE_NAME);
        let _ = std::fs::remove_dir_all(&dir);
        let e = Config::load(&path, true, &MechanismRegistry::builtin()).unwrap_err();
        assert!(e.contains("--config"), "{e}");
        assert!(e.contains("empty file"), "{e}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn skin_and_folders_round_trip() {
        let (c, warnings) =
            Config::parse("[video]\nskin = Dare Night\n[skins]\nfolders = /x, /y\n");
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(c.skin, "Dare Night");
        assert_eq!(
            c.skin_folders,
            vec![
                std::path::PathBuf::from("/x"),
                std::path::PathBuf::from("/y")
            ]
        );
        let (again, _) = Config::parse(&c.to_text(&MechanismRegistry::builtin()));
        assert_eq!(again.skin, c.skin);
        assert_eq!(again.skin_folders, c.skin_folders);
    }

    #[test]
    fn skin_defaults_to_dare() {
        let (c, _) = Config::parse("");
        assert_eq!(c.skin, "Dare");
        assert!(c.skin_folders.is_empty());
        let (blank, _) = Config::parse("[video]\nskin =\n[skins]\nfolders =\n");
        assert_eq!(blank.skin, "Dare");
        assert!(blank.skin_folders.is_empty());
    }
}
