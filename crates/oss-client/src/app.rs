//! Window/event loop glue: owns the window, renderer, input devices and the current
//! screen (connecting to a zone, or in the game).

use std::path::PathBuf;
use std::sync::Arc;

use oss_assets::{AssetDirs, AssetSearch};
use winit::application::ApplicationHandler;
#[cfg(not(target_arch = "wasm32"))]
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

use crate::audio::Audio;
use crate::game::account::{Answer, Prompt};
use crate::game::art::{self, Art};
use crate::game::input::{InputState, Stroke, char_for, shifted_char};
use crate::game::login::{Choice, Login};
use crate::game::online::{Online, ServerOptions, Supplied};
use crate::game::sandbox::Sandbox;
use crate::game::settings_ui::{SettingsUi, UiAction};
use crate::game::skin::{HotSpot, Skin, SkinEntry};
use crate::game::skin_screen::{self, SkinAction};
use crate::game::skin_view::{self, SkinState, SkinTexture};
use crate::game::start_spots::{Rect, Spot, Spots};
use crate::game::view::{Canvas, color, ui_color};
use crate::gfx::{FrameError, Renderer, TextureId};

/// An in-game screen `--screen` can put up at start, so that a screenshot can
/// catch one without a key being pressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartScreen {
    Menu,
    /// A help page, counted from zero.
    Help(usize),
    /// The start screen with its profile dialog open.
    Profile,
}

/// What to play.
pub enum Mode {
    /// Offline, on a local map (or the built-in one).
    Sandbox { map: Option<PathBuf>, ship: u8 },
    /// Online, in a zone.
    Zone(Box<ServerOptions>),
    /// The start-up screen, carrying everything the command line asked for so that
    /// going in through the form reaches the zone with the same options as going in
    /// straight from the command line.
    Login(Box<ServerOptions>),
    /// Playing a replay file back.
    Replay(PathBuf),
}

/// One scheduled keyboard event in a scripted run.
///
/// `--press` is a tap: the key goes down and comes up in the same frame, which
/// is all an edge-triggered screen looks at. `--hold` keeps it down, and repeats
/// it the way a real keyboard does, because a harness that holds a key without
/// repeating it is not holding a key.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScriptedKey {
    /// Seconds after the window opened.
    pub at: f32,
    pub key: winit::keyboard::KeyCode,
    pub what: ScriptedEdge,
}

/// What a [`ScriptedKey`] does to the key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptedEdge {
    Down,
    /// The OS repeating a key that is being held.
    Repeat,
    Up,
}

/// A held key does nothing for half a second, then about thirty a second.
///
/// Windows' own defaults, and the rate itself matters to nothing: what reads
/// repeats only asks whether one arrived. Expanded at parse time so the frame
/// loop stays a walk down one sorted list.
pub const REPEAT_DELAY: f32 = 0.5;
/// Repeats a second once they start. See [`REPEAT_DELAY`].
pub const REPEAT_RATE: u32 = 30;

/// One scripted pointer event, in UI pixels.
///
/// `--click` is a press and a release at the same instant, which is all a
/// click-sensed thing looks at. `--drag` puts a move between them and spreads
/// the three over separate frames, so a press in one place leaves the pointer
/// in another. Nothing the client draws is dragged today; the flag stays so
/// that a screen that grows something to drag can be tested without one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScriptedPoint {
    /// Seconds after the window opened.
    pub at: f32,
    pub x: f32,
    pub y: f32,
    pub what: PointerEdge,
}

/// What a [`ScriptedPoint`] does to the pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerEdge {
    Press,
    /// The pointer moving with the button down, which is what makes a drag one.
    Move,
    Release,
}

/// How far apart a scripted drag's press, move and release are put.
///
/// Anything over one frame will do; this is a handful of them, so a drag still
/// reads as one gesture where a run is being watched. Expanded at parse time so
/// the frame loop stays a walk down one sorted list.
pub const DRAG_STEP: f32 = 0.05;

/// How many lines of the settings window's lists one notch of the wheel moves.
const WHEEL_LINES: i32 = 3;

/// The events `--hold <at>:<key>:<secs>` arrives as: down, then repeats, then up.
///
/// The repeats are counted rather than stepped up to, so how many there are does
/// not turn on where a float lands.
pub fn hold_events(at: f32, key: winit::keyboard::KeyCode, secs: f32) -> Vec<ScriptedKey> {
    let mut v = vec![ScriptedKey {
        at,
        key,
        what: ScriptedEdge::Down,
    }];
    let repeats = ((secs - REPEAT_DELAY).max(0.0) * REPEAT_RATE as f32) as u32;
    for n in 0..repeats {
        v.push(ScriptedKey {
            at: at + REPEAT_DELAY + n as f32 / REPEAT_RATE as f32,
            key,
            what: ScriptedEdge::Repeat,
        });
    }
    v.push(ScriptedKey {
        at: at + secs,
        key,
        what: ScriptedEdge::Up,
    });
    v
}

pub struct Options {
    pub mode: Mode,
    /// Asset folder overriding the bundled assets (`--assets-dir`).
    pub assets_dir: Option<PathBuf>,
    /// A Continuum install to take assets from (`--continuum-dir`).
    pub continuum_dir: Option<PathBuf>,
    /// Integer pixel scale; `None` picks one from the display's DPI.
    pub scale: Option<u32>,
    pub vsync: bool,
    pub fullscreen: bool,
    /// Save a frame to this file after a short warm-up, then exit.
    pub screenshot: Option<PathBuf>,
    /// Seconds after start to take the screenshot.
    pub screenshot_at: f32,
    /// `--quit-at`: leave after this many seconds, so a timed measurement run
    /// ends by itself and the frame-time summary is printed.
    pub quit_at: Option<f32>,
    /// Drive the ship automatically (for screenshots and smoke tests).
    pub autopilot: bool,
    /// `--press` and `--hold`: scripted keyboard events, soonest first (testing).
    ///
    /// The start-up screen is the one screen no scripted run could reach past,
    /// because `--server` goes straight round it and `--send` only types in game.
    /// This presses its keys, so the path from the form to an arena can be driven
    /// and photographed rather than described.
    ///
    /// The settings window reads the same input, so these drive it too: its
    /// sections, its lines, a rebind and a save can all be pressed.
    pub press: Vec<ScriptedKey>,
    /// `--click` and `--drag`, expanded into pointer edges at the given times
    /// and places, soonest first (testing).
    ///
    /// The start-up screen is mouse-driven, so without this nothing could check
    /// that it is -- the same hole `--press` was, one screen along. Positions
    /// are in UI pixels (game pixels from the top-left), which is what every
    /// position in the drawing code is in, so a click can be aimed at what a
    /// screenshot shows. The settings window is drawn in the same units, so a
    /// click on it is aimed the same way.
    pub pointer: Vec<ScriptedPoint>,
    /// `--send`: lines to type in game at the given times, for scripted smoke tests
    /// (a live `?go`, say). Sorted soonest-last, so the game screen pops them off the
    /// end as its clock passes them.
    pub send: Vec<(f32, String)>,
    pub mute: bool,
    /// Master volume 0-100.
    pub volume: u32,
    /// `--config`: the settings file, if not the default one. It has to exist
    /// already; only the default one is written when it is missing.
    pub config: Option<PathBuf>,
    pub bindings: crate::game::input::Bindings,
    /// Where the settings file lives (for the settings window's Save).
    pub settings_path: Option<PathBuf>,
    /// `--settings`: open the settings window at start.
    pub open_settings: bool,
    /// An in-game screen to open as soon as the game starts (testing).
    pub open_screen: Option<StartScreen>,
    /// Macro keys from the settings file.
    pub macros: Vec<(crate::game::input::Binding, String)>,
    /// Directory servers for the zone browser.
    pub directories: Vec<String>,
    /// Connection encryption.
    pub security: oss_session::security::SecurityMode,
    /// What a build outside this repository supplies for the connection: the
    /// handshake, and the client the session presents itself as. The default is
    /// neither, which is this repository's own client — the modes
    /// [`Options::security`] names, and [`oss_session::VieClient`].
    ///
    /// Set between [`crate::options`] and [`run`]; see [`Supplied`] and the crate
    /// documentation.
    pub supplied: Supplied,
    /// `--security` was given (it wins over the settings file).
    pub security_from_cli: bool,
    /// Offer zones an OpenSec identity key.
    pub identity: bool,
    /// `--zones`: start on the zone browser.
    pub open_zones: bool,
    /// Accessibility settings.
    pub palette: crate::game::palette::PaletteKind,
    pub ui_scale: u32,
    /// Gamepad buttons per action.
    pub pad: crate::game::input::PadBindings,
    /// `--lvz`: LVZ files to show in the sandbox.
    pub lvz: Vec<PathBuf>,
    /// Player name from the settings file (the login screen's default).
    pub player_name: Option<String>,
    /// Where the login screen remembers the last zone and name.
    pub login_file: Option<PathBuf>,
    /// The settings file as it was read, before anything on the command line
    /// was laid over it.
    ///
    /// The settings screen starts from this, so its Save writes back what the
    /// file said plus what the player changed on the screen -- never what the
    /// command line asked for this once. It started from the merged options
    /// instead, and a test run's `--directory 127.0.0.1:4991` was saved into
    /// the lead's own settings, twice over, leaving the next ordinary start
    /// with a local directory and no zone list.
    pub saved: crate::config::Config,
}

/// Waiting for the zone: shows progress until the arena is ready.
struct Connecting {
    net: Online,
    art: Art,
    atlas: TextureId,
    /// The form to go back to if the connection fails.
    login: Option<Login>,
    /// A question the zone has asked, drawn over the progress lines while it
    /// waits for an answer.
    prompt: Option<Prompt>,
    /// When the screen opened: its ship turns and its stars drift by it.
    since: web_time::Instant,
    /// What the browser page was last told about the way in, so it is told once
    /// per change (see [`crate::web::loading_stage`]).
    #[cfg(target_arch = "wasm32")]
    told: Option<(crate::game::online::Stage, String, i32)>,
}

impl Connecting {
    fn new(net: Online, art: Art, atlas: TextureId, login: Option<Login>) -> Box<Self> {
        Box::new(Self {
            net,
            art,
            atlas,
            login,
            prompt: None,
            since: web_time::Instant::now(),
            #[cfg(target_arch = "wasm32")]
            told: None,
        })
    }

    /// Tells the browser page where the way in has got to, when that changed.
    /// Called once a frame has been drawn, so the page's first word is also its
    /// cue that the game's own screen is up to take over from its loader.
    #[cfg(target_arch = "wasm32")]
    fn tell_page(&mut self) {
        let net = &self.net;
        let percent = net
            .progress
            .map_or(-1, |p| (p.clamp(0.0, 1.0) * 100.0).floor() as i32);
        let now = (net.stage, net.detail.clone(), percent);
        if self.told.as_ref() != Some(&now) {
            crate::web::loading_stage(now.0.key(), &now.1, now.2);
            self.told = Some(now);
        }
    }
}

struct LoginScreen {
    login: Login,
    art: Art,
    atlas: TextureId,
    /// The zone list, which is part of this screen rather than a screen of its own.
    browser: crate::game::zones::ZoneBrowser,
    /// Where everything was drawn last frame, so the pointer can find it.
    /// Filled by [`draw_start_screen`] as it draws; see
    /// [`crate::game::start_spots`].
    spots: Spots,
    /// Fills in a saved password when the name and zone have one.
    recall: crate::game::remember::Recall,
    /// The skinned screen; `None` falls back to the plain drawn one.
    skin: Option<Box<SkinUi>>,
}

/// The skinned start screen's state.
struct SkinUi {
    skin: Skin,
    tex: SkinTexture,
    /// Every skin found, for the picker.
    entries: Vec<SkinEntry>,
    hovered: Option<HotSpot>,
    /// Index into [`skin_screen::FOCUS_ORDER`]; shown once Tab is used.
    focus: usize,
    show_focus: bool,
    scroll: usize,
    ping_graph: bool,
    /// A message that replaces the help text for a few seconds.
    note: Option<(String, web_time::Instant)>,
    opened: web_time::Instant,
    last_click: Option<(web_time::Instant, usize)>,
    /// The zone address the list last scrolled to show.
    shown: String,
    /// Actions from events between frames (the mouse wheel).
    pending: Vec<SkinAction>,
    /// The profile dialog's lines as last drawn, in its canvas's pixels.
    dialog_spots: Vec<(Rect, crate::game::profile_dialog::DialogSpot)>,
    /// Foundation's arenas, listed where the zones are (browser build).
    #[cfg(target_arch = "wasm32")]
    arenas: crate::game::arenas::ArenaList,
    /// The arena feed as last taken from the page, to notice a new one.
    #[cfg(target_arch = "wasm32")]
    last_feed: Option<Option<String>>,
    /// "+ CREATE" is waiting for Enter or Esc.
    #[cfg(target_arch = "wasm32")]
    confirm_create: bool,
}

/// Takes a new arena feed from the page, if there is one, into the list the
/// start screen draws (browser build).
#[cfg(target_arch = "wasm32")]
fn pump_arenas(ui: &mut SkinUi, browser: &mut crate::game::zones::ZoneBrowser) {
    let feed = crate::web::foundation_arenas();
    if !browser.zones.is_empty() && (feed.is_none() || feed == ui.last_feed) {
        return;
    }
    if let Some(text) = &feed {
        ui.arenas.set_feed(text.as_deref());
    }
    browser.zones = ui.arenas.as_zones();
    browser.status = if ui.arenas.unavailable {
        "Arena list unavailable".into()
    } else if feed.is_none() {
        "Loading arenas...".into()
    } else {
        match browser.zones.len() {
            1 => "1 arena".into(),
            n => format!("{n} arenas"),
        }
    };
    ui.last_feed = feed;
}

/// What one frame of the skinned start screen asks the app to do.
#[derive(Default)]
struct SkinOutcome {
    choice: Option<Choice>,
    /// Open the options, at this section if any.
    options: Option<Option<crate::game::settings_ui::Section>>,
    next_skin: bool,
}

/// How close two clicks on the same zone must be to count as a double-click.
const DOUBLE_CLICK: std::time::Duration = std::time::Duration::from_millis(400);
/// How long a note replaces the help text.
const NOTE_FOR: std::time::Duration = std::time::Duration::from_secs(3);

/// One frame of input on the skinned start screen.
#[allow(clippy::too_many_arguments)]
fn skin_frame(
    ui: &mut SkinUi,
    login: &mut Login,
    browser: &mut crate::game::zones::ZoneBrowser,
    window: (u32, u32),
    pointer: Option<(f32, f32)>,
    clicked: bool,
    strokes: &[Stroke],
    cmd: bool,
    shift: bool,
) -> SkinOutcome {
    #[cfg(target_arch = "wasm32")]
    pump_arenas(ui, browser);
    let layout = &ui.skin.layout;
    let main = (layout.main.w, layout.main.h);
    let s = skin_view::skin_scale(window, main);
    let o = skin_view::skin_origin(window, main, s);
    let at = pointer.map(|p| skin_view::to_skin(p, o, s));
    ui.hovered = at.and_then(|(x, y)| skin_view::hotspot_at(layout, x, y));
    let rows = skin_view::zone_rows(layout);

    // The selection follows the zone address, so a remembered zone shows
    // selected once the list arrives, and a refresh keeps it.
    // In the browser the list is arenas, and the selection follows the arena.
    #[cfg(not(target_arch = "wasm32"))]
    let (chosen, found) = (
        &login.server,
        skin_screen::keep_selection(Some(&login.server), &browser.zones)
            .filter(|&i| browser.zones[i].address == login.server),
    );
    #[cfg(target_arch = "wasm32")]
    let (chosen, found) = (
        &login.arena,
        skin_screen::keep_arena_selection(&login.arena, &browser.zones),
    );
    if let Some(i) = found {
        browser.selected = i;
        if &ui.shown != chosen {
            ui.scroll = skin_screen::scroll_to_show(i, ui.scroll, rows);
            ui.shown.clone_from(chosen);
        }
    }
    ui.scroll = skin_screen::clamp_scroll(ui.scroll, browser.zones.len(), rows);

    // The profile dialog, while open, has the keyboard and the pointer.
    if login.profile_open() {
        ui.hovered = None;
        ui.pending.clear();
        let mut out = SkinOutcome::default();
        if clicked
            && let Some((px, py)) = pointer
            && let Some((_, spot)) = ui
                .dialog_spots
                .iter()
                .find(|(r, _)| r.holds(px / s as f32, py / s as f32))
        {
            use crate::game::profile_dialog::{DialogLink, DialogSpot};
            match *spot {
                DialogSpot::Field(f) => {
                    login.focus_on(f);
                    if f == crate::game::login::Field::Remember {
                        login.toggle_remember();
                    }
                }
                #[cfg(target_arch = "wasm32")]
                DialogSpot::Link(link) => crate::web::open_foundation_page(match link {
                    DialogLink::Register => crate::web::FoundationPage::Register,
                    DialogLink::Forgot => crate::web::FoundationPage::Forgot,
                }),
                #[cfg(not(target_arch = "wasm32"))]
                DialogSpot::Link(DialogLink::Register | DialogLink::Forgot) => {}
            }
        }
        if let Some(crate::game::login::DialogResult::Play(c)) = login.input_profile(strokes, shift)
        {
            out.choice = Some(c);
        }
        return out;
    }

    // "+ CREATE" asked; Enter opens the application, Esc cancels. The answering
    // key does nothing else (Enter is also Play, Esc also Quit).
    // Any other key or click cancels it and then acts as usual.
    #[cfg(target_arch = "wasm32")]
    if ui.confirm_create {
        use skin_screen::ConfirmStep;
        match skin_screen::confirm_step(strokes, clicked) {
            ConfirmStep::Yes => {
                ui.confirm_create = false;
                crate::web::open_foundation_page(crate::web::FoundationPage::Apply);
                return SkinOutcome::default();
            }
            ConfirmStep::No => {
                ui.confirm_create = false;
                return SkinOutcome::default();
            }
            ConfirmStep::Cancel => ui.confirm_create = false,
            ConfirmStep::Wait => {}
        }
    }

    let mut actions = std::mem::take(&mut ui.pending);
    if clicked && let (Some(h), Some((x, y))) = (ui.hovered.clone(), at) {
        let row = skin_view::zone_row_at(layout, x, y, ui.scroll, browser.zones.len());
        let now = web_time::Instant::now();
        let double = row.is_some()
            && ui
                .last_click
                .is_some_and(|(t, r)| Some(r) == row && now.duration_since(t) < DOUBLE_CLICK);
        ui.last_click = row.map(|r| (now, r));
        actions.extend(skin_screen::click(&h, row, double));
    }
    if strokes
        .iter()
        .any(|st| st.key == Some(KeyCode::Tab) && !st.repeat)
    {
        ui.show_focus = true;
    }
    actions.extend(skin_screen::key(strokes, cmd, shift, &mut ui.focus));

    let mut out = SkinOutcome::default();
    let select = |browser: &mut crate::game::zones::ZoneBrowser, login: &mut Login, i: usize| {
        browser.select(i);
        if let Some(z) = browser.zones.get(i) {
            #[cfg(not(target_arch = "wasm32"))]
            login.server.clone_from(&z.address);
            #[cfg(target_arch = "wasm32")]
            {
                login.arena = crate::game::arenas::arena_for_login(&z.name);
            }
            login.error = None;
        }
    };
    for a in actions {
        match a {
            SkinAction::Play if login.name.trim().is_empty() => {
                login.open_profile();
                login.error = Some("Enter a name".into());
            }
            SkinAction::Play => out.choice = login.play(),
            SkinAction::Quit => out.choice = Some(Choice::Quit),
            SkinAction::Sandbox => out.choice = Some(Choice::Sandbox),
            #[cfg(not(target_arch = "wasm32"))]
            SkinAction::RefreshZones => out.choice = Some(Choice::RefreshZones),
            #[cfg(target_arch = "wasm32")]
            SkinAction::RefreshZones => crate::web::refresh_foundation_arenas(),
            SkinAction::OpenOptions(section) => out.options = Some(section),
            SkinAction::SelectShip(k) => login.ship = k,
            SkinAction::CycleShip(d) => {
                // Ships 0-7, then spectator, round again.
                let at = if login.ship == oss_session::SPECTATOR {
                    8
                } else {
                    i32::from(login.ship.min(7))
                };
                let next = (at + d).rem_euclid(9);
                login.ship = if next == 8 {
                    oss_session::SPECTATOR
                } else {
                    next as u8
                };
            }
            SkinAction::SelectZone(i) => select(browser, login, i),
            SkinAction::PlayZone(i) => {
                select(browser, login, i);
                if login.name.trim().is_empty() {
                    login.open_profile();
                    login.error = Some("Enter a name".into());
                } else {
                    out.choice = login.play();
                }
            }
            SkinAction::ScrollZones(d) => {
                let next = (ui.scroll as i64 + i64::from(d)).max(0) as usize;
                ui.scroll = skin_screen::clamp_scroll(next, browser.zones.len(), rows);
            }
            SkinAction::MoveSelection(d) => {
                if !browser.zones.is_empty() {
                    let last = browser.zones.len() as i64 - 1;
                    let i = (browser.selected as i64 + i64::from(d)).clamp(0, last) as usize;
                    select(browser, login, i);
                    ui.scroll = skin_screen::scroll_to_show(i, ui.scroll, rows);
                    #[cfg(not(target_arch = "wasm32"))]
                    ui.shown.clone_from(&login.server);
                    #[cfg(target_arch = "wasm32")]
                    ui.shown.clone_from(&login.arena);
                }
            }
            #[cfg(not(target_arch = "wasm32"))]
            SkinAction::TogglePing => ui.ping_graph = !ui.ping_graph,
            // SORT, in the browser.
            #[cfg(target_arch = "wasm32")]
            SkinAction::TogglePing => {
                ui.arenas.sort = ui.arenas.sort.next();
                browser.zones = ui.arenas.as_zones();
            }
            SkinAction::OpenProfile => login.open_profile(),
            SkinAction::NextSkin => out.next_skin = true,
            SkinAction::Info(text) => ui.note = Some((text.into(), web_time::Instant::now())),
            #[cfg(not(target_arch = "wasm32"))]
            SkinAction::CreateArena => {}
            #[cfg(target_arch = "wasm32")]
            SkinAction::CreateArena => ui.confirm_create = true,
        }
    }
    out
}

fn read_skin(entry: &SkinEntry) -> Result<std::borrow::Cow<'static, [u8]>, String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::fs::read(&entry.path)
            .map(Into::into)
            .map_err(|error| error.to_string())
    }
    #[cfg(target_arch = "wasm32")]
    {
        let bytes: &'static [u8] = match entry.name.as_str() {
            "Foundation" => include_bytes!("../../../assets/skins/Foundation.skn"),
            "Dare" => include_bytes!("../../../assets/skins/Dare.skn"),
            "Dare Night" => include_bytes!("../../../assets/skins/Dare Night.skn"),
            _ => return Err(format!("Skin {} unavailable", entry.name)),
        };
        Ok(bytes.into())
    }
}

/// Shows `e` in place of the current skin.
fn switch_skin(ui: &mut SkinUi, e: &SkinEntry, r: &mut Renderer) -> Result<(), String> {
    let bytes = read_skin(e)?;
    let skin = crate::game::skin::load_skin(&e.name, &bytes).map_err(|err| err.to_string())?;
    for w in &skin.layout.warnings {
        log::warn!("skin {}: {w}", e.name);
    }
    let tex = SkinTexture::upload(r, &skin);
    std::mem::replace(&mut ui.tex, tex).free(r);
    ui.skin = skin;
    ui.scroll = 0;
    ui.shown.clear();
    ui.note = Some((format!("Skin: {}", e.name), web_time::Instant::now()));
    Ok(())
}

/// Switches to the next skin that loads, and remembers the choice.
fn cycle_skin(options: &mut Options, ls: &mut LoginScreen, r: &mut Renderer) {
    let Some(ui) = ls.skin.as_deref_mut() else {
        return;
    };
    let mut name = ui.skin.name.clone();
    for _ in 0..ui.entries.len() {
        let Some(e) = crate::game::skin::next_skin(&ui.entries, &name).cloned() else {
            return;
        };
        name.clone_from(&e.name);
        match switch_skin(ui, &e, r) {
            Ok(()) => {
                options.saved.skin.clone_from(&e.name);
                if let Some(p) = &options.settings_path
                    && let Err(err) = std::fs::write(p, options.saved.to_text())
                {
                    log::warn!("couldn't save the skin choice: {err}");
                }
                return;
            }
            Err(err) => log::warn!("skin {}: {err}", e.name),
        }
    }
}

/// Draws the skinned start screen; returns the drifting starfield behind it.
fn draw_skin_screen(
    ls: &mut LoginScreen,
    r: &mut Renderer,
    pointer: Option<(f32, f32)>,
) -> Option<crate::gfx::StarParams> {
    let LoginScreen {
        login,
        art,
        atlas,
        browser,
        skin,
        ..
    } = ls;
    let ui = skin.as_deref_mut()?;
    let name = if login.name.is_empty() {
        "(press P)"
    } else {
        login.name.as_str()
    };
    // The Foundation skin labels the third line ARENA; the desktop skins, MACROS.
    #[cfg(target_arch = "wasm32")]
    let third_line = if login.arena.is_empty() {
        crate::game::arenas::PUBLIC
    } else {
        login.arena.as_str()
    };
    #[cfg(not(target_arch = "wasm32"))]
    let third_line = "Default";
    #[cfg(target_arch = "wasm32")]
    let confirming = ui.confirm_create;
    #[cfg(not(target_arch = "wasm32"))]
    let confirming = false;
    let now = web_time::Instant::now();
    let info: String = match (&ui.note, &login.error) {
        _ if confirming => skin_screen::CREATE_CONFIRM.to_string(),
        (Some((text, at)), _) if now.duration_since(*at) < NOTE_FOR => text.clone(),
        (_, Some(err)) => err.clone(),
        _ => ui
            .hovered
            .as_ref()
            .map(skin_screen::screen_help)
            .filter(|h| !h.is_empty())
            .map_or_else(|| browser.status.clone(), str::to_string),
    };
    let elapsed = ui.opened.elapsed();
    let state = SkinState {
        hovered: ui.hovered.clone(),
        focused: ui
            .show_focus
            .then(|| skin_screen::FOCUS_ORDER[ui.focus % skin_screen::FOCUS_ORDER.len()].clone()),
        ship: login.ship,
        tick: skin_view::tick_at(elapsed.as_millis()),
        // The skin format's four lines: profile, player name, macro set, key set.
        profile_lines: ["Default", name, third_line, "Default"],
        zones: &browser.zones,
        // In the browser the highlight is the arena Play will join, or none when
        // that arena is not listed.
        #[cfg(not(target_arch = "wasm32"))]
        selected: (!browser.zones.is_empty()).then_some(browser.selected),
        #[cfg(target_arch = "wasm32")]
        selected: skin_screen::keep_arena_selection(&login.arena, &browser.zones),
        scroll: ui.scroll,
        ping_graph: ui.ping_graph,
        arena_list: skin_screen::IN_BROWSER,
        info: &info,
    };
    let size = r.size();
    skin_view::draw_skin(r, &ui.tex, art, *atlas, &ui.skin, &state, size);
    let main = (ui.skin.layout.main.w, ui.skin.layout.main.h);
    let s = skin_view::skin_scale(size, main);
    ui.dialog_spots.clear();
    if login.profile_open() {
        let c = Canvas::new(art, *atlas, size, s, (0.0, 0.0));
        let at = pointer.map(|(x, y)| (x / s as f32, y / s as f32));
        ui.dialog_spots = crate::game::profile_dialog::draw_profile_dialog(&c, r, login, at);
    }
    let t = elapsed.as_secs_f32();
    Some(crate::gfx::StarParams {
        camera: [t * 14.0, t * 5.0],
        scale: skin_view::skin_scale(size, (ui.skin.layout.main.w, ui.skin.layout.main.h)) as f32,
    })
}

/// The folders skins are looked for in.
#[cfg(not(target_arch = "wasm32"))]
fn skin_folders(options: &Options, dir: &AssetDirs) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = dir
        .roots()
        .iter()
        .rev()
        .filter_map(|r| r.parent().map(|p| p.join("skins")))
        .collect();
    dirs.dedup();
    let settings = options
        .settings_path
        .as_deref()
        .and_then(|p| p.parent())
        .map(PathBuf::from);
    if let Some(sd) = settings {
        dirs.extend(crate::game::skin::skin_dirs(
            None,
            &sd,
            options.continuum_dir.as_deref(),
            &options.saved.skin_folders,
        ));
    } else {
        dirs.extend(options.continuum_dir.iter().map(|c| c.join("skins")));
        dirs.extend(options.saved.skin_folders.iter().cloned());
    }
    dirs
}

/// Loads the chosen skin (falling back to Dare, then to any that loads).
fn load_skin_ui(options: &Options, dir: &AssetDirs, r: &mut Renderer) -> Option<Box<SkinUi>> {
    #[cfg(not(target_arch = "wasm32"))]
    let entries = crate::game::skin::find_skins(&skin_folders(options, dir));
    #[cfg(target_arch = "wasm32")]
    let entries = {
        let _ = dir;
        ["Foundation", "Dare", "Dare Night"]
            .into_iter()
            .map(|name| SkinEntry {
                name: name.into(),
                path: PathBuf::from(format!("{name}.skn")),
            })
            .collect::<Vec<_>>()
    };
    let first = crate::game::skin::pick(&entries, &options.saved.skin)?.clone();
    let order = std::iter::once(&first).chain(entries.iter().filter(|e| e.path != first.path));
    for e in order {
        match read_skin(e)
            .and_then(|b| crate::game::skin::load_skin(&e.name, &b).map_err(|err| err.to_string()))
        {
            Ok(skin) => {
                for w in &skin.layout.warnings {
                    log::warn!("skin {}: {w}", e.name);
                }
                let tex = SkinTexture::upload(r, &skin);
                let now = web_time::Instant::now();
                return Some(Box::new(SkinUi {
                    skin,
                    tex,
                    entries,
                    hovered: None,
                    focus: 0,
                    show_focus: false,
                    scroll: 0,
                    ping_graph: false,
                    note: None,
                    opened: now,
                    last_click: None,
                    shown: String::new(),
                    pending: Vec::new(),
                    dialog_spots: Vec::new(),
                    #[cfg(target_arch = "wasm32")]
                    arenas: Default::default(),
                    #[cfg(target_arch = "wasm32")]
                    last_feed: None,
                    #[cfg(target_arch = "wasm32")]
                    confirm_create: false,
                }));
            }
            Err(err) => log::warn!("skin {} ({}): {err}", e.name, e.path.display()),
        }
    }
    None
}

enum Screen {
    Login(Box<LoginScreen>),
    Connecting(Box<Connecting>),
    Game(Box<Sandbox>),
    /// Placeholder while switching screens.
    Empty,
}

struct Running {
    window: Arc<Window>,
    renderer: Renderer,
    screen: Screen,
    scale: u32,
    started: web_time::Instant,
    captured: bool,
    /// The window can't be seen (minimised/covered); Windows then stops sending
    /// redraws, so the game is driven from `about_to_wait` without drawing.
    occluded: bool,
    /// The login form to come back to when the player leaves the game, as it was
    /// when they went in. `None` when the run never passed through it.
    front_end: Option<Login>,
}

enum AppEvent {
    #[cfg(target_arch = "wasm32")]
    GraphicsReady(Arc<Window>, Result<Renderer, String>),
}

struct App {
    #[cfg(target_arch = "wasm32")]
    proxy: winit::event_loop::EventLoopProxy<AppEvent>,
    #[cfg(target_arch = "wasm32")]
    initializing_graphics: bool,
    options: Options,
    dir: AssetDirs,
    level: Vec<u8>,
    running: Option<Running>,
    input: InputState,
    gilrs: Option<gilrs::Gilrs>,
    /// The settings window, while it is open over whichever screen opened it.
    settings: Option<SettingsUi>,
    error: Option<String>,
    /// How far through [`Options::press`] the clock has got.
    pressed: usize,
    /// How far through [`Options::pointer`] the clock has got.
    pointed: usize,
    /// Where the pointer is, in UI pixels, while it is over the window.
    cursor: Option<(f32, f32)>,
    /// A left click arrived and the screen has not answered it yet. Held here
    /// rather than acted on in the event, so a click is read against the same
    /// regions, in the same place in the frame, as a key is.
    clicked: bool,
    /// This machine's password store, if it has one, for "Remember password".
    vault: crate::game::remember::Vault,
}

pub fn run(options: Options) -> Result<(), String> {
    let search = AssetSearch {
        assets_dir: options.assets_dir.clone(),
        continuum_dir: options.continuum_dir.clone(),
        // A binary built from the checkout also finds the repository's bundled
        // folder, in every profile. This used to be gated on `debug_assertions`,
        // which made `cargo run --release -p oss-client` -- the command the README
        // gives first -- fail on any machine without a Continuum install, because
        // nothing copies `assets/classic` next to the executable. It is still
        // ordered after the folder beside the executable, so a packaged build that
        // ships its own assets wins.
        //
        // The folder is found at run time, by walking up from the running
        // executable. It used to be `env!("CARGO_MANIFEST_DIR")`, which resolves
        // when the client is compiled and therefore wrote the building machine's
        // absolute path -- its user name included -- into every binary built from
        // it, shipped ones included. An installed copy finds nothing above itself
        // and the search simply moves on, as it did with a baked path that did not
        // exist on the player's machine.
        dev_dir: oss_assets::checkout_assets_dir(),
        discover_install: true,
    };
    let dir = AssetDirs::locate(&search).map_err(|e| {
        format!(
            "{e}
Put the game graphics in assets/classic beside the program, or point --assets-dir ({}) or --continuum-dir at a folder that has them.",
            oss_assets::ASSETS_ENV_VAR
        )
    })?;
    let shown: Vec<String> = dir
        .roots()
        .iter()
        .map(|r| r.display().to_string())
        .collect();
    log::info!("assets: {}", shown.join(", "));
    let level = match &options.mode {
        Mode::Sandbox { map: Some(map), .. } => {
            oss_assets::locate::read_file(map).map_err(|e| e.to_string())?
        }
        Mode::Sandbox { map: None, .. } => crate::game::default_map::default_map(),
        Mode::Zone(_) | Mode::Login(_) | Mode::Replay(_) => Vec::new(),
    };

    let event_loop = EventLoop::<AppEvent>::with_user_event()
        .build()
        .map_err(|e| e.to_string())?;
    event_loop.set_control_flow(ControlFlow::Poll);
    let gilrs = match gilrs::Gilrs::new() {
        Ok(g) => Some(g),
        Err(e) => {
            log::warn!("gamepads unavailable: {e}");
            None
        }
    };
    let mut input = InputState::new(options.bindings.clone());
    input.set_pad_bindings(options.pad.clone());
    let app = App {
        #[cfg(target_arch = "wasm32")]
        proxy: event_loop.create_proxy(),
        #[cfg(target_arch = "wasm32")]
        initializing_graphics: false,
        options,
        dir,
        level,
        running: None,
        input,
        gilrs,
        settings: None,
        error: None,
        pressed: 0,
        pointed: 0,
        clicked: false,
        cursor: None,
        #[cfg(not(target_arch = "wasm32"))]
        vault: crate::game::remember::Vault::probe(),
        #[cfg(target_arch = "wasm32")]
        vault: crate::game::remember::Vault::None,
    };
    #[cfg(target_arch = "wasm32")]
    {
        use winit::platform::web::EventLoopExtWebSys;
        event_loop.spawn_app(app);
        Ok(())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut app = app;
        event_loop.run_app(&mut app).map_err(|e| e.to_string())?;
        match app.error {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }
}

/// Leaves the zone before the process ends.
///
/// `event_loop.exit()` only stops the loop, and nothing in the transport sends a
/// disconnect on drop, so every quit -- the Esc menu, the window close button, a
/// finished `--screenshot` run -- reached the server as a timeout and left it
/// holding the slot. Called on each exit that might have a live session.
fn leave_zone(screen: &mut Screen) {
    match screen {
        Screen::Game(game) => game.disconnect(),
        Screen::Connecting(conn) => conn.net.disconnect(),
        _ => {}
    }
}

impl App {
    fn fail(&mut self, event_loop: &ActiveEventLoop, msg: String) {
        log::error!("{msg}");
        // In the browser, the page that started the game shows why and takes over.
        #[cfg(target_arch = "wasm32")]
        crate::web::return_to_page(Some(&msg));
        self.error = Some(msg);
        event_loop.exit();
    }

    /// Leaves the game for the front end, which is where SubSpace and Continuum
    /// put you when you pick Quit on the menu: the program only ends when you quit
    /// the front end itself.
    ///
    /// The form comes back as it was on the way in. A run that started straight in
    /// a zone (`--server`) or in the sandbox never saw one, so one is made from the
    /// settings and from what that run was playing -- the originals always have a
    /// front end to come back to, and vanishing is the thing being fixed.
    fn back_to_front_end(&mut self) -> Result<(), String> {
        // In the browser, a run the page started goes back to that page instead.
        #[cfg(target_arch = "wasm32")]
        if crate::web::return_to_page(None) {
            return Ok(());
        }
        let Some(run) = self.running.as_mut() else {
            return Ok(());
        };
        let mut login = match run.front_end.take() {
            Some(login) => login,
            // This run never passed through the form, so one is made from the
            // settings and from what the run was playing -- which `Login::load`
            // lays over the file for us, the same way it does at start-up.
            None => {
                let base = match &self.options.mode {
                    Mode::Zone(z) | Mode::Login(z) => z.clone(),
                    _ => Box::new(Login::default().into_base()),
                };
                let mut l = Login::load(
                    self.options.login_file.as_deref(),
                    self.options.player_name.as_deref(),
                    base,
                );
                if let Mode::Zone(opts) = &self.options.mode {
                    l.ship = opts.ship;
                }
                l
            }
        };
        login.error = None;
        let (art, atlas) = load_art(&self.dir, &mut run.renderer)?;
        let skin = load_skin_ui(&self.options, &self.dir, &mut run.renderer);
        run.screen = Screen::Login(Box::new(LoginScreen {
            login,
            art,
            atlas,
            browser: crate::game::zones::ZoneBrowser::open(&self.options.directories),
            spots: Spots::default(),
            recall: Default::default(),
            skin,
        }));
        set_title(&run.window, "Open SubSpace");
        self.settings = None;
        self.input.clear();
        run.window.request_redraw();
        Ok(())
    }

    fn audio(&self) -> Option<Audio> {
        if self.options.mute {
            None
        } else {
            Audio::new(&self.dir, self.options.volume)
        }
    }
}

impl ApplicationHandler<AppEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        #[cfg(target_arch = "wasm32")]
        if self.initializing_graphics {
            return;
        }
        if self.running.is_some() {
            return;
        }
        let title = match &self.options.mode {
            Mode::Sandbox { .. } => "Open SubSpace — sandbox".to_string(),
            Mode::Zone(z) => format!("Open SubSpace — {}", z.server),
            Mode::Login(_) => "Open SubSpace".to_string(),
            Mode::Replay(p) => format!("Open SubSpace — replay {}", p.display()),
        };
        // In the browser the page owns the tab title and the canvas size: the
        // canvas fills the window by CSS and the client follows it on resize.
        #[cfg(not(target_arch = "wasm32"))]
        let mut attrs = Window::default_attributes()
            .with_title(title)
            .with_inner_size(LogicalSize::new(1280.0, 800.0));
        #[cfg(target_arch = "wasm32")]
        let mut attrs = {
            let _ = title;
            Window::default_attributes()
        };
        if self.options.fullscreen {
            attrs = attrs.with_fullscreen(Some(winit::window::Fullscreen::Borderless(None)));
        }
        #[cfg(target_arch = "wasm32")]
        {
            use winit::platform::web::WindowAttributesExtWebSys;
            attrs = attrs.with_append(true).with_prevent_default(true);
        }
        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => return self.fail(event_loop, format!("window: {e}")),
        };
        #[cfg(target_arch = "wasm32")]
        {
            self.initializing_graphics = true;
            let proxy = self.proxy.clone();
            let vsync = self.options.vsync;
            wasm_bindgen_futures::spawn_local(async move {
                let renderer = Renderer::new_async(window.clone(), vsync).await;
                let _ = proxy.send_event(AppEvent::GraphicsReady(window, renderer));
            });
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let renderer = Renderer::new(window.clone(), self.options.vsync);
            self.finish_resume(event_loop, window, renderer);
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: AppEvent) {
        let AppEvent::GraphicsReady(window, renderer) = event;
        self.initializing_graphics = false;
        self.finish_resume(event_loop, window, renderer);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if self.running.is_none() {
            return;
        }
        match event {
            WindowEvent::CloseRequested => {
                if let Some(run) = self.running.as_mut() {
                    leave_zone(&mut run.screen);
                }
                event_loop.exit()
            }
            WindowEvent::Resized(size) => {
                log::debug!("resized to {}x{}", size.width, size.height);
                if let Some(run) = self.running.as_mut() {
                    run.renderer.resize(size.width, size.height);
                }
            }
            WindowEvent::Focused(false) => self.input.clear(),
            WindowEvent::Focused(true) => {
                if let Some(Running {
                    screen: Screen::Game(game),
                    ..
                }) = self.running.as_mut()
                {
                    game.refocused();
                }
            }
            WindowEvent::Occluded(hidden) => {
                if let Some(run) = self.running.as_mut() {
                    run.occluded = hidden;
                    if !hidden {
                        run.window.request_redraw();
                    }
                }
            }
            // Every key goes into the client's own input, the settings window's
            // included: it reads the keystrokes from there in `frame`, which is
            // what lets a scripted `--press` reach it as it reaches every other
            // screen.
            WindowEvent::KeyboardInput { event, .. } => {
                let code = match event.physical_key {
                    PhysicalKey::Code(code) => Some(code),
                    _ => None,
                };
                if event.state != ElementState::Pressed {
                    if let Some(code) = code {
                        self.input.release(code);
                    }
                    return;
                }
                // The key and the character it typed are one keystroke, with the
                // OS repeat flag on it. `event.repeat` is the flag that used to
                // be dropped on the floor, which is why a held Backspace cleared
                // one character and stopped.
                let mut chars = event.text.iter().flat_map(|t| t.chars());
                let first = chars.next();
                if code.is_some() || first.is_some() {
                    self.input.press(Stroke {
                        key: code,
                        text: first,
                        repeat: event.repeat,
                    });
                }
                // A key event can carry more than one character when a dead key
                // finishes a composition. The key belongs to the first of them.
                for c in chars {
                    self.input.press(Stroke::text(c).repeated(event.repeat));
                }
            }
            // The pointer, which the start-up screen is built around: Continuum's
            // own is a set of named hot spots, so a keyboard-only version of it
            // is a gap rather than a shortfall. Kept to that screen for now --
            // the game underneath has no pointer of its own yet.
            WindowEvent::CursorMoved { position, .. } => {
                let Some(run) = self.running.as_mut() else {
                    return;
                };
                let scale = run.scale.max(1) as f64;
                let at = ((position.x / scale) as f32, (position.y / scale) as f32);
                self.cursor = Some(at);
                if matches!(run.screen, Screen::Login(_)) || self.settings.is_some() {
                    run.window.request_redraw();
                }
            }
            WindowEvent::CursorLeft { .. } => self.cursor = None,
            // A click is held for `frame`, which hands it to the settings window
            // when that is open and to the screen underneath when it is not.
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => self.clicked = true,
            WindowEvent::MouseWheel { delta, .. } => {
                // `zoneup`/`zonedown` in the skin format are buttons beside the
                // list. The wheel over the list is where a pointer looks for
                // them now.
                let lines = match delta {
                    winit::event::MouseScrollDelta::LineDelta(_, y) => y,
                    winit::event::MouseScrollDelta::PixelDelta(p) => (p.y / 20.0) as f32,
                };
                // Not `signum`, which calls zero positive and would scroll on a
                // wheel that has not turned.
                let rows = if lines > 0.0 {
                    -1
                } else if lines < 0.0 {
                    1
                } else {
                    0
                };
                // The settings window's long lists (a line for every action)
                // scroll under the wheel wherever the pointer is.
                if let Some(ui) = self.settings.as_mut() {
                    ui.scroll(rows * WHEEL_LINES);
                    return;
                }
                let Some(run) = self.running.as_mut() else {
                    return;
                };
                if let Screen::Login(ls) = &mut run.screen
                    && let Some(ui) = ls.skin.as_deref_mut()
                {
                    if let Some(a) = ui
                        .hovered
                        .as_ref()
                        .and_then(|h| skin_screen::wheel(h, rows))
                    {
                        ui.pending.push(a);
                        run.window.request_redraw();
                    }
                    return;
                }
                let size = run.renderer.size();
                let cursor = self.cursor.map(|at| to_start_scale(at, size, run.scale));
                if let Screen::Login(ls) = &mut run.screen
                    && let Some((mx, my)) = cursor
                    && ls.spots.over_list(mx, my)
                {
                    ls.browser.scroll_by(rows);
                    ls.login.focus_zones();
                    if let Some(z) = ls.browser.zones.get(ls.browser.selected) {
                        ls.login.server.clone_from(&z.address);
                        ls.login.error = None;
                    }
                    run.window.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => self.frame(event_loop, true),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let Some(run) = self.running.as_ref() else {
            return;
        };
        if run.occluded {
            // Keep the simulation and connection alive, at a modest rate.
            self.frame(event_loop, false);
            event_loop.set_control_flow(ControlFlow::WaitUntil(
                web_time::Instant::now() + std::time::Duration::from_millis(5),
            ));
        } else {
            event_loop.set_control_flow(ControlFlow::Poll);
        }
    }
}

impl App {
    fn finish_resume(
        &mut self,
        event_loop: &ActiveEventLoop,
        window: Arc<Window>,
        renderer: Result<Renderer, String>,
    ) {
        let mut renderer = match renderer {
            Ok(renderer) => renderer,
            Err(error) => return self.fail(event_loop, format!("graphics: {error}")),
        };
        let scale = wanted_scale(self.options.scale, window.scale_factor());
        let screen = match &self.options.mode {
            Mode::Sandbox { ship, .. } => {
                match Sandbox::new(
                    &mut renderer,
                    &self.dir,
                    &self.level,
                    *ship,
                    scale,
                    self.audio(),
                ) {
                    Ok(mut s) => {
                        s.aim_bot = self.options.autopilot;
                        s.scripted = self.options.send.clone();
                        s.open_settings = self.options.open_settings;
                        s.open_screen(self.options.open_screen);
                        for path in &self.options.lvz {
                            match oss_assets::locate::read_file(path) {
                                Ok(data) => s.load_lvz(&path.display().to_string(), &data),
                                Err(e) => log::warn!("{e}"),
                            }
                        }
                        s.macros = self.options.macros.clone();
                        s.palette = self.options.palette;
                        s.ui_scale_extra = self.options.ui_scale;
                        Screen::Game(Box::new(s))
                    }
                    Err(e) => return self.fail(event_loop, e),
                }
            }
            Mode::Zone(z) => {
                let mut z = (**z).clone();
                apply_settings(&self.options, &mut z);
                let z = &z;
                let size = window.inner_size();
                let res = ((size.width / scale) as u16, (size.height / scale) as u16);
                let net = match Online::connect(z, res, &self.options.supplied) {
                    Ok(n) => n,
                    Err(e) => return self.fail(event_loop, e),
                };
                let (art, atlas) = match load_art(&self.dir, &mut renderer) {
                    Ok(a) => a,
                    Err(e) => return self.fail(event_loop, e),
                };
                Screen::Connecting(Connecting::new(net, art, atlas, None))
            }
            Mode::Replay(path) => {
                let net = match Online::open_replay(path) {
                    Ok(n) => n,
                    Err(e) => return self.fail(event_loop, e),
                };
                let (art, atlas) = match load_art(&self.dir, &mut renderer) {
                    Ok(a) => a,
                    Err(e) => return self.fail(event_loop, e),
                };
                Screen::Connecting(Connecting::new(net, art, atlas, None))
            }
            Mode::Login(z) => {
                let (art, atlas) = match load_art(&self.dir, &mut renderer) {
                    Ok(a) => a,
                    Err(e) => return self.fail(event_loop, e),
                };
                let mut login = Login::load(
                    self.options.login_file.as_deref(),
                    self.options.player_name.as_deref(),
                    z.clone(),
                );
                if self.options.open_zones {
                    login.focus_zones();
                }
                // Opened for one arena (the builder's "Fly in it"): start with it chosen.
                #[cfg(target_arch = "wasm32")]
                if let Some(arena) = crate::web::start_arena() {
                    login.arena = arena;
                }
                let skin = load_skin_ui(&self.options, &self.dir, &mut renderer);
                if matches!(self.options.open_screen, Some(StartScreen::Profile)) {
                    login.open_profile();
                }
                Screen::Login(Box::new(LoginScreen {
                    login,
                    art,
                    atlas,
                    browser: crate::game::zones::ZoneBrowser::open(&self.options.directories),
                    spots: Spots::default(),
                    recall: Default::default(),
                    skin,
                }))
            }
        };
        window.request_redraw();
        self.running = Some(Running {
            window,
            renderer,
            screen,
            front_end: None,
            scale,
            started: web_time::Instant::now(),
            captured: false,
            occluded: false,
        });
    }

    /// One frame: input, network, simulation and (when `draw`) rendering.
    fn frame(&mut self, event_loop: &ActiveEventLoop, draw: bool) {
        let frame_at = web_time::Instant::now();
        if let Some(g) = self.gilrs.as_mut() {
            while let Some(ev) = g.next_event() {
                let pressed = self.input.gamepad(ev.event);
                if let (Some(b), Some(ui)) = (pressed, self.settings.as_mut()) {
                    ui.capture_pad(b);
                }
            }
        }
        let audio_wanted = !self.options.mute;
        let Some(since_start) = self.running.as_ref().map(|r| r.started.elapsed()) else {
            return;
        };
        let elapsed = since_start.as_secs_f32();
        // Scripted pointer events: put the pointer there, then press, which is
        // the order a real pointer does it in and the order the hover highlight
        // needs. Every screen, the settings window included, keeps the same
        // pointer in the same units, which has a place and a click and nothing
        // else yet: a release is theirs to ignore, and a drag is a press in one
        // place and a pointer that ends up in another.
        while let Some(p) = self.options.pointer.get(self.pointed).copied() {
            if p.at > elapsed {
                break;
            }
            log::info!("pointer: {:?} at {},{}", p.what, p.x, p.y);
            self.cursor = Some((p.x, p.y));
            self.clicked |= p.what == PointerEdge::Press;
            self.pointed += 1;
        }
        // Taken before the run is borrowed, because the screens underneath want
        // it and it lives on the app rather than on any one of them.
        let cursor = self.cursor;
        // Scripted keys, before anything reads the input. A key carries the
        // character it types, the way a window keyboard event does, or it drives
        // nothing that reads text -- which is the menu, the chat line, every
        // form field and a macro's line in the settings.
        while let Some(k) = self.options.press.get(self.pressed).copied() {
            if k.at > elapsed {
                break;
            }
            match k.what {
                ScriptedEdge::Down | ScriptedEdge::Repeat => {
                    // Shift is read from what is held, so `--hold` on a Shift
                    // key makes the keys under it type their capitals -- which
                    // is the only way a scripted run can type the colon in a
                    // zone address.
                    let typed = if self.input.shift() {
                        shifted_char(k.key)
                    } else {
                        char_for(k.key)
                    };
                    let repeat = k.what == ScriptedEdge::Repeat;
                    if !repeat {
                        log::info!("--press: {:?}", k.key);
                    }
                    let mut s = Stroke::press(k.key).repeated(repeat);
                    if let Some(c) = typed {
                        s = s.typing(c);
                    }
                    self.input.press(s);
                }
                ScriptedEdge::Up => self.input.release(k.key),
            }
            self.pressed += 1;
        }
        // The settings window, while it is open, has the keyboard and the
        // pointer: it takes this frame's keystrokes and click before the screen
        // underneath can, and the key table's actions are dropped so nothing
        // fires in the game when the window closes.
        if let Some(ui) = self.settings.as_mut() {
            let strokes = self.input.take_strokes();
            self.input.take_edges();
            let mut action = ui.input(&strokes, self.input.shift(), self.input.ctrl());
            if std::mem::take(&mut self.clicked)
                && action == UiAction::None
                && let Some((mx, my)) = cursor
            {
                action = ui.click(mx, my);
            }
            match action {
                UiAction::None => {}
                UiAction::Close => self.settings = None,
                UiAction::Save => self.save_settings(),
            }
        }
        let Some(run) = self.running.as_mut() else {
            return;
        };
        if self.options.autopilot {
            self.input.autopilot(run.started.elapsed().as_secs_f32());
        }

        // Login: wait for a choice.
        if let Screen::Login(ls) = &mut run.screen {
            #[cfg(not(target_arch = "wasm32"))]
            ls.browser.pump();
            // The Remember line appears once the store has said it is there.
            self.vault.poll();
            ls.login.set_vault(self.vault.store().is_some());
            if let Some(store) = self.vault.store() {
                ls.recall
                    .tick(&mut ls.login, &store, web_time::Instant::now());
                if let Some(who) = ls.login.take_forget() {
                    crate::game::remember::forget(&store, who);
                }
            }
            // While the options are up they have the keyboard; the screen underneath
            // would otherwise take every character into whichever field has focus.
            let strokes = if self.settings.is_some() {
                self.input.drop_strokes();
                Vec::new()
            } else {
                self.input.take_strokes()
            };
            self.input.take_edges();
            let mut open_options = None;
            let mut next_skin = false;
            let choice = if let Some(ui) = ls.skin.as_deref_mut() {
                let clicked = std::mem::take(&mut self.clicked) && self.settings.is_none();
                let phys = cursor.map(|(x, y)| (x * run.scale as f32, y * run.scale as f32));
                let out = skin_frame(
                    ui,
                    &mut ls.login,
                    &mut ls.browser,
                    run.renderer.size(),
                    phys,
                    clicked,
                    &strokes,
                    self.input.cmd() || self.input.ctrl(),
                    self.input.shift(),
                );
                open_options = out.options;
                next_skin = out.next_skin;
                out.choice
            } else {
                // The list owns Up/Down and the rest of its keys while it has the focus.
                // Whichever row it settles on fills in the Zone field, so the highlighted
                // row and the address can never disagree; Enter and Esc belong to the
                // screen, and the form below answers them.
                if ls.login.focused() == crate::game::login::Field::Zones {
                    let picked = ls.browser.input(&strokes);
                    if let Some(crate::game::zones::BrowserChoice::Pick(address)) = picked {
                        ls.login.server = address;
                        ls.login.error = None;
                    } else if strokes.iter().any(|s| s.key.is_some())
                        && let Some(z) = ls.browser.zones.get(ls.browser.selected)
                    {
                        ls.login.server.clone_from(&z.address);
                        ls.login.error = None;
                    }
                }
                // A click, against the regions the last frame drew. The keyboard is
                // untouched by this: clicking a field focuses it exactly as tabbing
                // to it does, so everything that worked before still does.
                let clicked = std::mem::take(&mut self.clicked);
                let mut pointed = None;
                if clicked && self.settings.is_none() {
                    let size = run.renderer.size();
                    pointed = cursor
                        .map(|at| to_start_scale(at, size, run.scale))
                        .and_then(|(mx, my)| ls.spots.at(mx, my));
                    match pointed {
                        Some(Spot::Field(field)) => {
                            ls.login.focus_on(field);
                            if field == crate::game::login::Field::Remember {
                                ls.login.toggle_remember();
                            }
                        }
                        // Picking a ship takes the focus with it, so that choosing
                        // the one you already have still shows something: the
                        // heading's name turns yellow and its line lights up.
                        // Without that, clicking Spectator while already
                        // spectating set spectator to spectator and the screen
                        // said nothing at all -- which is indistinguishable from a
                        // click that missed.
                        Some(Spot::Ship(kind)) => {
                            ls.login.ship = kind;
                            ls.login.focus_on(crate::game::login::Field::Ship);
                        }
                        Some(Spot::Spectate) => {
                            ls.login.ship = oss_session::SPECTATOR;
                            ls.login.focus_on(crate::game::login::Field::Ship);
                        }
                        Some(Spot::Zone(row)) => {
                            ls.browser.select(row);
                            ls.login.focus_zones();
                            if let Some(z) = ls.browser.zones.get(row) {
                                ls.login.server.clone_from(&z.address);
                                ls.login.error = None;
                            }
                        }
                        Some(Spot::RefreshZones)
                        | Some(Spot::Play)
                        | Some(Spot::Quit)
                        | Some(Spot::Options)
                        | Some(Spot::Sandbox)
                        | None => {}
                    }
                }
                match pointed {
                    // The four that are choices rather than changes go through the
                    // same door the keys do, so neither can do something the other
                    // cannot.
                    Some(Spot::Play) => ls.login.play(),
                    Some(Spot::Quit) => Some(Choice::Quit),
                    Some(Spot::Options) => Some(Choice::Options),
                    Some(Spot::Sandbox) => Some(Choice::Sandbox),
                    Some(Spot::RefreshZones) => Some(Choice::RefreshZones),
                    _ if self.input.cmd() || self.input.ctrl() => {
                        crate::game::login::Login::shortcut(&strokes)
                    }
                    _ => ls.login.input(&strokes, self.input.shift()),
                }
            };
            if let Some(section) = open_options {
                let mut ui = SettingsUi::new_at(current_config(&self.options), section);
                if let Some(skin) = ls.skin.as_deref() {
                    ui.set_skins(skin.entries.iter().map(|e| e.name.clone()).collect());
                }
                self.settings = Some(ui);
            }
            // A skin picked in the options shows as soon as it is saved.
            if let Some(ui) = ls.skin.as_deref_mut()
                && !ui.skin.name.eq_ignore_ascii_case(&self.options.saved.skin)
            {
                if let Some(e) =
                    crate::game::skin::pick(&ui.entries, &self.options.saved.skin).cloned()
                    && let Err(err) = switch_skin(ui, &e, &mut run.renderer)
                {
                    log::warn!("skin {}: {err}", e.name);
                }
                self.options.saved.skin.clone_from(&ui.skin.name);
            }
            if next_skin {
                cycle_skin(&mut self.options, ls, &mut run.renderer);
            }
            match choice {
                Some(Choice::Options) => {
                    self.settings = Some(SettingsUi::new(current_config(&self.options)));
                }
                Some(Choice::RefreshZones) => {
                    // Keeps the zones on screen while the directories are asked
                    // again; the line under the list says a refresh is running.
                    ls.browser.refresh(&self.options.directories);
                    // Whatever went wrong last time is not what is happening now,
                    // and an error on that line hides the refresh behind it.
                    ls.login.error = None;
                }
                None => {}
                Some(Choice::Quit) => {
                    event_loop.exit();
                    return;
                }
                Some(Choice::Sandbox) => {
                    let level = crate::game::default_map::default_map();
                    let audio = audio_wanted
                        .then(|| Audio::new(&self.dir, self.options.volume))
                        .flatten();
                    match Sandbox::new(&mut run.renderer, &self.dir, &level, 0, run.scale, audio) {
                        Ok(mut s) => {
                            s.macros = self.options.macros.clone();
                            s.palette = self.options.palette;
                            s.ui_scale_extra = self.options.ui_scale;
                            // The start screen goes away here; its skin's texture
                            // goes with it rather than staying on the GPU.
                            if let Some(ui) = ls.skin.take() {
                                ui.tex.free(&mut run.renderer);
                            }
                            run.screen = Screen::Game(Box::new(s));
                        }
                        Err(e) => return self.fail(event_loop, e),
                    }
                }
                Some(Choice::Connect(mut opts)) => {
                    apply_settings(&self.options, &mut opts);
                    if let Some(path) = &self.options.login_file {
                        ls.login.save(path);
                    }
                    let size = run.window.inner_size();
                    let res = (
                        (size.width / run.scale) as u16,
                        (size.height / run.scale) as u16,
                    );
                    match Online::connect(&opts, res, &self.options.supplied) {
                        Ok(net) => {
                            set_title(&run.window, &format!("Open SubSpace — {}", opts.server));
                            let Screen::Login(ls) =
                                std::mem::replace(&mut run.screen, Screen::Empty)
                            else {
                                unreachable!()
                            };
                            let LoginScreen {
                                login,
                                art,
                                atlas,
                                skin,
                                ..
                            } = *ls;
                            if let Some(ui) = skin {
                                ui.tex.free(&mut run.renderer);
                            }
                            run.screen =
                                Screen::Connecting(Connecting::new(net, art, atlas, Some(login)));
                        }
                        Err(e) => ls.login.error = Some(e),
                    }
                }
            }
        }

        // Connecting: pump until the arena is ready, then switch to the game.
        if let Screen::Connecting(conn) = &mut run.screen {
            conn.net.pump();
            // The zone offered to create the name: ask the player, over this
            // screen, with the connection held open behind the box.
            if conn.prompt.is_none()
                && let Some((_, said)) = &conn.net.new_account
            {
                conn.prompt = Some(Prompt::new_account(said.as_deref()));
            }
            // The zone let us in and asks for its registration form: the same
            // box, and the arena waits behind it.
            if conn.prompt.is_none() && conn.net.registration {
                conn.prompt = Some(Prompt::registration());
            }
            let strokes = self.input.take_strokes();
            self.input.take_edges();
            let clicked = std::mem::take(&mut self.clicked);
            // Why the player is going back, when they are.
            let mut give_up = None;
            if let Some(prompt) = conn.prompt.as_mut() {
                let mut answer = prompt.input(&strokes, self.input.shift());
                if answer.is_none()
                    && clicked
                    && let Some((mx, my)) = cursor
                {
                    answer = prompt.click(mx, my);
                }
                match answer {
                    Some(Answer::Create) => {
                        conn.prompt = None;
                        conn.net.create_account();
                    }
                    Some(Answer::Cancel) => give_up = Some("No account was created.".to_string()),
                    Some(Answer::Send(form)) => {
                        conn.prompt = None;
                        conn.net.send_registration(&form);
                    }
                    // Continuum's other way out of its form: leave the zone.
                    Some(Answer::Disconnect) => {
                        give_up = Some("Disconnected from the zone.".to_string());
                    }
                    None => {}
                }
            } else if strokes
                .iter()
                .any(|s| s.first_press() == Some(KeyCode::Escape))
            {
                give_up = Some("Cancelled".to_string());
            }
            let failed = conn.net.error.clone();
            if give_up.is_some() || failed.is_some() {
                // Say so to the zone. Dropping the session sends nothing, and a
                // zone holding a login open for an answer would otherwise wait
                // out the timeout for one that is not coming.
                if failed.is_none() {
                    conn.net.disconnect();
                }
                // Back to the form if there was one; otherwise this was the whole run.
                let Screen::Connecting(conn) = std::mem::replace(&mut run.screen, Screen::Empty)
                else {
                    unreachable!()
                };
                let Connecting {
                    art, atlas, login, ..
                } = *conn;
                match (login, failed) {
                    (Some(mut login), err) => {
                        login.error = err.or(give_up);
                        let skin = load_skin_ui(&self.options, &self.dir, &mut run.renderer);
                        run.screen = Screen::Login(Box::new(LoginScreen {
                            login,
                            art,
                            atlas,
                            browser: crate::game::zones::ZoneBrowser::open(
                                &self.options.directories,
                            ),
                            spots: Spots::default(),
                            recall: Default::default(),
                            skin,
                        }));
                    }
                    (None, Some(err)) => return self.fail(event_loop, err),
                    (None, None) => {
                        if let Some(why) = &give_up {
                            log::info!("{why}");
                        }
                        #[cfg(target_arch = "wasm32")]
                        crate::web::return_to_page(give_up.as_deref());
                        event_loop.exit();
                        return;
                    }
                }
            }
        }
        if let Screen::Connecting(conn) = &mut run.screen
            && conn.net.ready()
        {
            let Screen::Connecting(conn) = std::mem::replace(&mut run.screen, Screen::Empty) else {
                unreachable!()
            };
            // Kept for the way back: leaving the game returns to this form.
            let conn = *conn;
            // The zone has let us in, so the password was right: now, and only
            // now, it is remembered (or a remembered one forgotten).
            if let Some(login) = &conn.login
                && let Some(store) = self.vault.store()
            {
                crate::game::remember::settle(&store, login);
            }
            run.front_end = conn.login;
            let audio = if audio_wanted {
                Audio::new(&self.dir, self.options.volume)
            } else {
                None
            };
            match Sandbox::new_online(&mut run.renderer, &self.dir, conn.net, run.scale, audio) {
                Ok(mut s) => {
                    log::info!("entered arena and initialized game view");
                    #[cfg(target_arch = "wasm32")]
                    crate::web::loading_stage("playing", "", 100);
                    s.aim_bot = self.options.autopilot;
                    s.scripted = self.options.send.clone();
                    s.open_screen(self.options.open_screen);
                    // `--lvz` applies online too, so a zone's own file can be tried
                    // against a real arena (and so an arena change has something to
                    // tear down on a zone that serves none).
                    for path in &self.options.lvz {
                        match oss_assets::locate::read_file(path) {
                            Ok(data) => s.load_lvz(&path.display().to_string(), &data),
                            Err(e) => log::warn!("{e}"),
                        }
                    }
                    s.macros = self.options.macros.clone();
                    s.palette = self.options.palette;
                    s.ui_scale_extra = self.options.ui_scale;
                    run.screen = Screen::Game(Box::new(s));
                }
                Err(e) => return self.fail(event_loop, e),
            }
        }

        let stars = match &mut run.screen {
            Screen::Game(game) => {
                if self.settings.is_none() {
                    // A click on the Esc menu becomes the keystroke that row
                    // answers to, put on the queue in front of everything the
                    // keyboard sent, so the menu reads it as a key and never
                    // learns a pointer was involved.
                    if std::mem::take(&mut self.clicked)
                        && let Some((mx, my)) = cursor
                        && let Some(stroke) = game.click(mx, my)
                    {
                        self.input.press(stroke);
                        // A click is a press and a release. Without the release
                        // the key stays held as far as the key table knows, and
                        // the real key does nothing until the window loses focus.
                        if let Some(key) = stroke.key {
                            self.input.release(key);
                        }
                    }
                    game.update(&mut self.input);
                } else {
                    // The world keeps running; your ship idles while the screen is open.
                    let mut idle = InputState::new(Default::default());
                    game.update(&mut idle);
                }
                if game.leave {
                    game.disconnect();
                    if let Err(e) = self.back_to_front_end() {
                        return self.fail(event_loop, e);
                    }
                    return;
                }
                if std::mem::take(&mut game.open_settings) {
                    self.settings = Some(SettingsUi::new(current_config(&self.options)));
                }
                if std::mem::take(&mut game.screenshot_request) {
                    match screenshot_path(self.options.settings_path.as_deref()) {
                        Ok(path) => {
                            game.message(format!("Screenshot: {}", path.display()), color::GREEN);
                            run.renderer.capture = Some(path);
                        }
                        Err(e) => game.message(format!("Can't save a screenshot: {e}"), color::RED),
                    }
                }
                draw.then(|| {
                    let at = web_time::Instant::now();
                    let stars = game.draw(&mut run.renderer);
                    let took = at.elapsed();
                    run.renderer.perf().record(crate::gfx::Stage::Draw, took);
                    stars
                })
            }
            Screen::Connecting(_) | Screen::Login(_) if !draw => None,
            Screen::Login(ls) => {
                if ls.skin.is_some() {
                    let phys = cursor.map(|(x, y)| (x * run.scale as f32, y * run.scale as f32));
                    draw_skin_screen(ls, &mut run.renderer, phys)
                } else {
                    let size = run.renderer.size();
                    let cursor = cursor.map(|at| to_start_scale(at, size, run.scale));
                    draw_start_screen(ls, &mut run.renderer, start_scale(size, run.scale), cursor);
                    None
                }
            }
            Screen::Connecting(conn) => {
                let c = Canvas::new(
                    &conn.art,
                    conn.atlas,
                    run.renderer.size(),
                    run.scale,
                    (0.0, 0.0),
                );
                let t = conn.since.elapsed().as_secs_f32();
                if let Some(prompt) = conn.prompt.as_mut() {
                    prompt.draw(&c, &mut run.renderer, cursor);
                } else {
                    // The panel is a few lines in a window that may be very
                    // big: drawn twice the size where there is room for it.
                    let size = run.renderer.size();
                    let zoom = crate::game::connect_screen::zoom(size, run.scale);
                    let c = Canvas::new(&conn.art, conn.atlas, size, run.scale * zoom, (0.0, 0.0));
                    crate::game::connect_screen::draw(
                        &c,
                        &mut run.renderer,
                        &conn.art,
                        &conn.net,
                        t,
                    );
                }
                Some(crate::game::connect_screen::stars(t, run.scale))
            }
            Screen::Empty => None,
        };

        if !draw {
            return;
        }
        let screenshot_due = self.options.screenshot.is_some()
            && run.started.elapsed().as_secs_f32() >= self.options.screenshot_at;
        if screenshot_due && !run.captured && run.renderer.capture.is_none() {
            run.renderer.capture = self.options.screenshot.clone();
            run.captured = true;
        }
        if let Some(ui) = self.settings.as_mut() {
            draw_settings(ui, &run.screen, &mut run.renderer, run.scale, cursor);
        }
        match run.renderer.render(stars, 0.0) {
            Ok(()) | Err(FrameError::Skip) => {}
            Err(FrameError::Fatal(e)) => return self.fail(event_loop, e),
        }
        #[cfg(target_arch = "wasm32")]
        if let Screen::Connecting(conn) = &mut run.screen {
            conn.tell_page();
        }
        run.renderer.perf().frame_done(frame_at.elapsed());
        if let Some(at) = self.options.quit_at
            && run.started.elapsed().as_secs_f32() >= at
        {
            log::info!("--quit-at {at}: leaving");
            leave_zone(&mut run.screen);
            event_loop.exit();
            return;
        }
        // A skipped frame (e.g. occluded window) keeps the capture pending.
        if run.captured && run.renderer.capture.is_none() {
            leave_zone(&mut run.screen);
            event_loop.exit();
            return;
        }
        run.window.request_redraw();
    }
}

fn load_art(dir: &AssetDirs, renderer: &mut Renderer) -> Result<(Art, TextureId), String> {
    let (art, size, pixels) = art::load(dir, None)?;
    let atlas = renderer.create_texture(size, size, &pixels);
    Ok((art, atlas))
}

/// Draws the settings window over whichever screen opened it, with that
/// screen's artwork, at the pixel scale the pointer is measured in.
fn draw_settings(
    ui: &mut SettingsUi,
    screen: &Screen,
    r: &mut Renderer,
    scale: u32,
    cursor: Option<(f32, f32)>,
) {
    let (art, atlas) = match screen {
        Screen::Login(ls) => (&ls.art, ls.atlas),
        Screen::Connecting(conn) => (&conn.art, conn.atlas),
        Screen::Game(game) => game.artwork(),
        Screen::Empty => return,
    };
    let mut c = Canvas::new(art, atlas, r.size(), scale, (0.0, 0.0));
    // In the palette being chosen, so a player picking one for their eyes sees
    // it before saving it rather than after.
    c.tints = ui.cfg.palette.text_tints();
    ui.draw(&c, r, cursor);
}

/// What the settings window opens on: the settings file's own values, never the
/// command line's (see [`Options::saved`]).
fn current_config(o: &Options) -> crate::config::Config {
    o.saved.clone()
}

impl App {
    fn save_settings(&mut self) {
        let Some(ui) = self.settings.as_mut() else {
            return;
        };
        let cfg = ui.cfg.clone();
        self.options.saved = cfg.clone();
        let path = self.options.settings_path.clone();
        ui.status = Some(match &path {
            Some(p) => match std::fs::write(p, cfg.to_text()) {
                Ok(()) => {
                    ui.mark_saved();
                    format!("Saved to {}", p.display())
                }
                Err(e) => format!("Couldn't save: {e}"),
            },
            None => {
                ui.mark_saved();
                "No settings folder; applied for this session only".into()
            }
        });
        // Apply what can change now.
        let o = &mut self.options;
        o.scale = cfg.scale;
        o.vsync = cfg.vsync;
        o.mute = cfg.mute;
        o.volume = cfg.volume;
        o.player_name = cfg.name.clone();
        o.bindings = cfg.bindings.clone();
        o.macros = cfg.macros.clone();
        o.palette = cfg.palette;
        o.ui_scale = cfg.ui_scale;
        o.pad = cfg.pad.clone();
        self.input.set_bindings(cfg.bindings.clone());
        self.input.set_pad_bindings(cfg.pad.clone());
        if let Some(run) = self.running.as_mut() {
            if cfg.fullscreen != o.fullscreen {
                run.window.set_fullscreen(
                    cfg.fullscreen
                        .then_some(winit::window::Fullscreen::Borderless(None)),
                );
            }
            // "From display" means take it from the display, which is a value
            // to work out now, not a setting to skip. Skipping it is what made
            // choosing 0 look as though it did nothing: the old scale stayed
            // until the next start. The same function the first frame used, so
            // the two cannot answer differently.
            let wanted = wanted_scale(cfg.scale, run.window.scale_factor());
            run.scale = wanted;
            if let Screen::Game(game) = &mut run.screen {
                game.apply_settings(
                    Some(wanted),
                    cfg.volume,
                    cfg.macros.clone(),
                    cfg.palette,
                    cfg.ui_scale,
                );
            }
        }
        self.options.fullscreen = cfg.fullscreen;
    }
}

/// Names the window. In the browser the page keeps its own title.
fn set_title(window: &Window, title: &str) {
    #[cfg(not(target_arch = "wasm32"))]
    window.set_title(title);
    #[cfg(target_arch = "wasm32")]
    let _ = (window, title);
}

/// The pixel scale to run at: the one that was chosen, or the display's own
/// where nothing was.
///
/// `None` is the settings window's "from display" (0 in the file). It is a
/// value to resolve, not a setting to leave alone -- treating it as the latter
/// is what made choosing 0 appear to do nothing, because the old scale simply
/// stayed.
/// Used both for the first frame and when the settings are saved, so the two
/// cannot disagree.
fn wanted_scale(chosen: Option<u32>, display_factor: f64) -> u32 {
    chosen
        .unwrap_or_else(|| display_factor.round().max(1.0) as u32)
        .clamp(1, 8)
}

/// A new screenshot file: `screenshots/oss-YYYY-MM-DD-HHMMSS.png` beside the
/// settings file (or in the current folder).
fn screenshot_path(settings: Option<&std::path::Path>) -> std::io::Result<PathBuf> {
    let dir = settings
        .and_then(std::path::Path::parent)
        .map_or_else(|| PathBuf::from("screenshots"), |d| d.join("screenshots"));
    std::fs::create_dir_all(&dir)?;
    let secs = web_time::SystemTime::now()
        .duration_since(web_time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let stamp = utc_stamp(secs);
    for n in 0..100 {
        let name = if n == 0 {
            format!("oss-{stamp}.png")
        } else {
            format!("oss-{stamp}-{n}.png")
        };
        let path = dir.join(name);
        if !path.exists() {
            return Ok(path);
        }
    }
    Err(std::io::Error::other("too many screenshots this second"))
}

/// `YYYY-MM-DD-HHMMSS` (UTC) for seconds since 1970.
fn utc_stamp(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    // Days to civil date (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}-{:02}{:02}{:02}",
        rem / 3600,
        rem / 60 % 60,
        rem % 60
    )
}

/// The pixel scale the start-up screen is drawn at: the largest whole number that
/// keeps the panel within most of the window, and never smaller than the game's
/// own. Without this the panel is a small box in the middle of a big window on a
/// high-resolution display.
fn start_scale(screen: (u32, u32), base: u32) -> u32 {
    // The panel's footprint in unscaled pixels, with room for its tallest list.
    const PANEL_W: f32 = START_COLS * 8.0;
    const PANEL_H: f32 = 330.0;
    let fit_w = screen.0 as f32 * 0.85 / PANEL_W;
    let fit_h = screen.1 as f32 * 0.85 / PANEL_H;
    (fit_w.min(fit_h).floor() as u32).clamp(base.max(1), 8)
}

/// Moves a pointer position from the game's scale to the start-up screen's.
fn to_start_scale(at: (f32, f32), screen: (u32, u32), base: u32) -> (f32, f32) {
    let k = base.max(1) as f32 / start_scale(screen, base) as f32;
    (at.0 * k, at.1 * k)
}

/// How wide the start-up screen is, in characters of the 8x12 font.
const START_COLS: f32 = 72.0;
/// The profile and ships take the left of it; the zone list has the rest.
const START_LEFT_COLS: f32 = 30.0;

/// Colour for a zone's round trip, banded good / usable / poor, as the originals
/// coloured them. A zone that has not answered is dimmed rather than coloured: no
/// answer is not the same as a bad answer.
///
/// The exact thresholds the original used are not recorded here, so these are picked
/// to be useful against what live zones actually return — the ones measured today sat
/// between 72 and 115 ms. The fidelity point is that the column is coloured at all.
fn ping_color(ping_ms: Option<u32>) -> u32 {
    match ping_ms {
        None => color::DARK_RED,
        Some(ms) if ms <= 100 => color::GREEN,
        Some(ms) if ms <= 250 => color::YELLOW,
        Some(_) => color::RED,
    }
}

/// The start-up screen: profile, ship, zone list and the way through to the
/// options, on one screen.
///
/// Continuum's start-up screen is a single skinned window carrying all of that --
/// its skin format names `profile`, `ship1`..`ship8`, `spectate`, `zones`, `trace`,
/// `play`, `quit`, `macros`, `keydefs` and `options` as hot spots on one surface --
/// where this client had three screens and made the player walk between them. The
/// art of that window is Continuum's own and not ours to copy, so this is its
/// arrangement drawn in the box the game draws for itself.
fn draw_start_screen(
    ls: &mut LoginScreen,
    r: &mut Renderer,
    scale: u32,
    cursor: Option<(f32, f32)>,
) {
    let c = Canvas::new(&ls.art, ls.atlas, r.size(), scale, (0.0, 0.0));
    let (login, b) = (&ls.login, &ls.browser);
    let focus = login.focused();
    // The regions are collected here, as each thing is drawn, so what a pointer
    // can click is the same rectangle a player can see. Last frame's are thrown
    // away first.
    let spots = &mut ls.spots;
    spots.clear();
    let hovered = |rect: Rect| cursor.is_some_and(|(mx, my)| rect.holds(mx, my));

    let w = START_COLS * 8.0;
    let left_w = START_LEFT_COLS * 8.0;
    let list_x_off = left_w + 16.0;
    let list_w = w - list_x_off;

    // Tall enough for whichever column needs more, and for a list worth reading.
    let rows = b.zones.len().clamp(8, 14);
    let list_h = 14.0 + rows as f32 * 12.0;
    // Heading, the profile lines (four, or five with Remember), the gap, the
    // ship heading, and the ship grid: three rows of forty, the third of them
    // spectator's own cell.
    let profile_lines = login
        .fields()
        .iter()
        .filter(|f| profile_label(**f).is_some())
        .count() as f32;
    let left_h = 14.0 + profile_lines * 14.0 + 10.0 + 14.0 + 3.0 * 40.0;
    let body_h = list_h.max(left_h);
    // The foot carries the zone's blurb, two lines for anything that went wrong,
    // and the two key lines.
    let h = 20.0 + body_h + 68.0;
    let x = ((c.view_w - w) / 2.0).floor();
    let top = ((c.view_h - h) / 2.0).floor().max(4.0);

    c.panel(r, x, top, w, h);
    c.text_center(
        r,
        x + w / 2.0,
        top + 1.0,
        color::GREEN,
        "-= Open SubSpace =-",
    );
    c.separator(r, x, top + 13.0, w);

    // --- Profile -------------------------------------------------------------------
    let mut y = top + 20.0;
    c.text(r, x + 4.0, y, color::DARK_RED, "Profile");
    y += 14.0;
    let value_x = x + 4.0 + 10.0 * 8.0;
    for &field in login.fields() {
        let Some(label) = profile_label(field) else {
            continue;
        };
        let row = Rect::new(x + 2.0, y - 1.0, left_w - 4.0, 13.0);
        spots.add(row, Spot::Field(field));
        if focus == field || hovered(row) {
            c.color_rect(r, ui_color::HIGHLIGHT, row.x, row.y, row.w, row.h);
        }
        c.text(r, x + 4.0, y, color::GREEN, label);
        let mut value = login.value(field);
        if focus == field && field != crate::game::login::Field::Remember {
            value.push('_');
        }
        let max = ((left_w - 4.0 - (value_x - x)) / 8.0).max(1.0) as usize;
        let shown: String = if value.chars().count() > max {
            value.chars().skip(value.chars().count() - max).collect()
        } else {
            value
        };
        c.text(r, value_x, y, color::WHITE, &shown);
        y += 14.0;
    }

    // --- Ships, as the originals show them: the ships themselves ------------------
    y += 10.0;
    // The heading names the chosen ship beside it, and that word -- "Spectator",
    // when you are spectating -- is the only place on this screen the choice
    // appears in writing, so it is the first thing a pointer goes for. It was
    // not a region at all: the click landed on nothing, changed nothing and said
    // nothing, which is what "you can't directly click Spectator" was. It takes
    // the focus now, exactly as clicking a line of the profile does.
    let ship_row = Rect::new(x + 2.0, y - 1.0, left_w - 4.0, 13.0);
    spots.add(ship_row, Spot::Field(crate::game::login::Field::Ship));
    if focus == crate::game::login::Field::Ship || hovered(ship_row) {
        c.color_rect(
            r,
            ui_color::HIGHLIGHT,
            ship_row.x,
            ship_row.y,
            ship_row.w,
            ship_row.h,
        );
    }
    c.text(r, x + 4.0, y, color::DARK_RED, "Ship");
    c.text(
        r,
        x + 4.0 + 6.0 * 8.0,
        y,
        if focus == crate::game::login::Field::Ship {
            color::YELLOW
        } else {
            color::WHITE
        },
        ship_title(login.ship),
    );
    y += 14.0;
    let pitch = 56.0;
    let cols = Login::SHIP_COLUMNS;
    // The eight ships and spectator, as one grid of nine cells.
    //
    // Spectator used to be a line of text under the grid reading `S = Spectator`:
    // the same choice the eight beside it offer as a picture you click, offered
    // instead as a keystroke to type. Two kinds of thing for one action, and the
    // one that looks like a label is the one that is not. Worse, the key it named
    // was never implemented here -- nothing on this screen answered to S -- so the
    // one instruction it did give was false.
    for slot in 0..=cols * 2 {
        let (row, col) = ((slot / cols) as f32, (slot % cols) as f32);
        let (sx, sy) = (x + 6.0 + col * pitch, y + row * 40.0);
        // `ship1`..`ship8` and `spectate`: the skin's own hot spots, and the
        // reason this screen was never meant to be driven from the keyboard.
        let spectate = slot == cols * 2;
        let kind = if spectate {
            oss_session::SPECTATOR
        } else {
            slot
        };
        let box_ = Rect::new(sx - 2.0, sy - 2.0, 40.0, 40.0);
        spots.add(
            box_,
            if spectate {
                Spot::Spectate
            } else {
                Spot::Ship(slot)
            },
        );
        if login.ship == kind || hovered(box_) {
            c.color_rect(r, ui_color::HIGHLIGHT, box_.x, box_.y, box_.w, box_.h);
        }
        if spectate {
            // There is no spectator sprite to draw -- the whole point is that
            // there is no ship -- so the cell carries the word instead, in the
            // same box, the same size, with the same highlight.
            c.text_center(r, box_.x + box_.w / 2.0, sy + 12.0, color::WHITE, "SPEC");
        } else {
            c.ui(r, ls.art.ship(slot, 0), sx, sy, None, [1.0; 4]);
        }
    }

    // --- Zones ---------------------------------------------------------------------
    let lx = x + list_x_off;
    let mut ly = top + 20.0;
    // `zones`: the whole list, so the wheel can stand in for `zoneup`/`zonedown`.
    spots.set_list(Rect::new(lx - 2.0, ly - 1.0, list_w + 2.0, body_h));
    let head = Rect::new(lx - 2.0, ly - 1.0, list_w + 2.0, 13.0);
    spots.add(head, Spot::Field(crate::game::login::Field::Zones));
    if focus == crate::game::login::Field::Zones || hovered(head) {
        c.color_rect(r, ui_color::HIGHLIGHT, head.x, head.y, head.w, head.h);
    }
    c.text(r, lx, ly, color::DARK_RED, "Zone");
    c.text_right(r, x + w - 4.0, ly, color::DARK_RED, "Players  Ping");
    ly += 14.0;
    let first = b.selected.saturating_sub(rows - 1);
    let name_chars = ((list_w - 18.0 * 8.0) / 8.0).max(4.0) as usize;
    for (i, z) in b.zones.iter().enumerate().skip(first).take(rows) {
        let row = Rect::new(lx - 2.0, ly - 1.0, list_w + 2.0, 12.0);
        spots.add(row, Spot::Zone(i));
        if i == b.selected || hovered(row) {
            c.color_rect(r, ui_color::HIGHLIGHT, row.x, row.y, row.w, 13.0);
        }
        let name: String = z.name.chars().take(name_chars).collect();
        c.text(r, lx, ly, color::WHITE, &name);
        // Players and ping are drawn separately so the ping can carry its own
        // colour, as SubSpace and Continuum did.
        c.text_right(
            r,
            x + w - 4.0 - 6.0 * 8.0,
            ly,
            color::WHITE,
            &format!("{:>7}", z.players),
        );
        let ping = z.ping_ms.map_or_else(|| "-".to_string(), |p| p.to_string());
        c.text_right(
            r,
            x + w - 4.0,
            ly,
            ping_color(z.ping_ms),
            &format!("{ping:>4}"),
        );
        ly += 12.0;
    }

    // --- The foot: what the selected zone is, then what to press -------------------
    let foot = top + 20.0 + body_h;
    c.separator(r, x, foot, w);
    let width = (w / 8.0 - 1.0) as usize;
    let described: String = b
        .zones
        .get(b.selected)
        .map(|z| z.description.as_str())
        .unwrap_or_default()
        .chars()
        .take(width)
        .collect();
    c.text(r, x + 4.0, foot + 6.0, color::GREEN, &described);

    // The key lines are buttons as well as reminders. Continuum's `play`, `quit`
    // and `options` are visible buttons on its own start-up screen; ours were a
    // line of footer text, which is why the options could be in the client for a
    // week without the person who asked for them finding them.
    let lines: [&[(&str, Option<Spot>)]; 2] = [
        &[
            ("Enter = Play", Some(Spot::Play)),
            ("Tab/arrows = Move", None),
            ("F2/Cmd-O = Options", Some(Spot::Options)),
        ],
        &[
            ("F3/Cmd-R = Zone Refresh", Some(Spot::RefreshZones)),
            ("F5/Cmd-S = Sandbox", Some(Spot::Sandbox)),
            ("Esc = Quit", Some(Spot::Quit)),
        ],
    ];
    for (n, items) in lines.iter().enumerate() {
        let ky = foot + 44.0 + n as f32 * 12.0;
        // Four spaces between items, which is the spacing these two lines were
        // written with when they were one string each.
        let total: f32 = items
            .iter()
            .map(|(t, _)| t.chars().count() as f32 * 8.0)
            .sum::<f32>()
            + 32.0 * (items.len() - 1) as f32;
        let mut kx = (x + w / 2.0 - total / 2.0).floor();
        for (text, spot) in items.iter() {
            let tw = text.chars().count() as f32 * 8.0;
            if let Some(spot) = *spot {
                let rect = Rect::new(kx - 2.0, ky - 1.0, tw + 4.0, 13.0);
                spots.add(rect, spot);
                if hovered(rect) {
                    c.color_rect(r, ui_color::HIGHLIGHT, rect.x, rect.y, rect.w, rect.h);
                }
            }
            c.text(r, kx, ky, color::YELLOW, text);
            kx += tw + 32.0;
        }
    }

    // The second line is whichever matters most: what went wrong, what the
    // pointer is over, or how the zone list is getting on. The middle one is the
    // job Continuum's `infobox` does, in the place it does it. Drawn after the
    // key lines so that their regions are in before this asks what is under the
    // pointer.
    let hint = cursor.and_then(|(mx, my)| spots.at(mx, my)).map(Spot::hint);
    let (second, colour) = match (&login.error, hint) {
        (Some(err), _) => (err.as_str(), color::RED),
        // While a directory is still being waited on, how the list is getting on
        // beats what the pointer is over. Clicking "F3 = Zone Refresh" leaves the
        // pointer on the button, so the button's own description would otherwise
        // sit there for the whole fetch and the refresh would have no answer at
        // all -- which is the half of this the wording never fixed.
        (None, Some(hint)) if !b.busy() => (hint, color::YELLOW),
        (None, _) => (b.status.as_str(), color::WHITE),
    };
    // Wrapped, not cut. A refused connection explains itself in a sentence and a
    // half -- "the zone didn't prove the key on record; remove its line from
    // known_servers.txt" -- and truncating that at the panel edge throws away the
    // half that says what to do about it.
    for (i, line) in crate::game::chat::wrap(second, width, true)
        .iter()
        .take(2)
        .enumerate()
    {
        c.text(r, x + 4.0, foot + 18.0 + i as f32 * 12.0, colour, line);
    }
}

/// The label for a field of the profile block, or `None` for the fields that are
/// drawn as something other than a line of text.
fn profile_label(field: crate::game::login::Field) -> Option<&'static str> {
    use crate::game::login::Field;
    match field {
        Field::Name | Field::Password | Field::Remember | Field::Arena | Field::Server => {
            Some(field.label())
        }
        Field::Ship | Field::Zones => None,
    }
}

/// The selected ship's name, for the line under the ships.
fn ship_title(ship: u8) -> &'static str {
    const NAMES: [&str; 8] = [
        "Warbird",
        "Javelin",
        "Spider",
        "Leviathan",
        "Terrier",
        "Weasel",
        "Lancaster",
        "Shark",
    ];
    NAMES.get(ship as usize).copied().unwrap_or("Spectator")
}

/// The parts of a connection that come from the settings file rather than from the
/// command line or the form. Both ways into a zone go through here, so neither can
/// be given a different set by accident.
fn apply_settings(o: &Options, opts: &mut ServerOptions) {
    opts.security = o.security;
    opts.known_servers = known_servers_path(o);
    opts.identities = identities_path(o);
}

/// `identities.txt` beside the settings file for your OpenSec identity keys, unless
/// identities are off.
fn identities_path(o: &Options) -> Option<PathBuf> {
    let dir = o.settings_path.as_deref()?.parent()?;
    o.identity.then(|| dir.join("identities.txt"))
}

/// `known_servers.txt` beside the settings file, for OpenSec keys.
fn known_servers_path(o: &Options) -> Option<PathBuf> {
    o.settings_path
        .as_deref()
        .and_then(std::path::Path::parent)
        .map(|d| d.join("known_servers.txt"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A test run's `--directory 127.0.0.1:4991` reached the lead's own
    /// settings file, twice, and the next ordinary start showed a local
    /// directory and no zones. The command line is for this run; the settings
    /// screen, and so its Save, starts from what the file said.
    ///
    /// This fails on the version before the fix, which built the screen's
    /// starting values from the merged options.
    #[test]
    fn the_command_line_is_never_saved_into_the_settings_file() {
        let dir = std::env::temp_dir().join(format!("oss-client-b108-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp folder");
        let file = dir.join("settings.ini");
        std::fs::write(
            &file,
            "[directory]
servers = dir.example:4990
[video]
scale = 3
",
        )
        .expect("written");
        let args = [
            "--config",
            file.to_str().expect("a path"),
            "--directory",
            "127.0.0.1:4991",
            "--scale",
            "1",
            "--mute",
        ];
        let o = crate::cli::options(args.iter().map(|s| s.to_string()))
            .expect("parsed")
            .expect("not --help");
        std::fs::remove_dir_all(&dir).ok();

        // This run does what the command line asked.
        assert!(o.directories.iter().any(|d| d == "127.0.0.1:4991"));
        assert_eq!(o.scale, Some(1));
        assert!(o.mute);
        // What a Save would write is the file's own, untouched.
        let saved = current_config(&o).to_text();
        assert!(!saved.contains("127.0.0.1"), "{saved}");
        assert!(saved.contains("servers = dir.example:4990"), "{saved}");
        assert!(saved.contains("scale = 3"), "{saved}");
        assert!(saved.contains("mute = false"), "{saved}");
    }

    /// "The scale number doesn't seem to stick or even take effect" -- the
    /// second half of that was choosing 0, which resolved to nothing at all.
    #[test]
    fn zero_means_the_display_rather_than_no_change() {
        // "From display" reaches here as `None`, and must come back as a real
        // scale. Returning the old one is what made the setting look dead.
        assert_eq!(wanted_scale(None, 1.0), 1, "an ordinary display");
        assert_eq!(wanted_scale(None, 2.0), 2, "a high-DPI one");
        assert_eq!(wanted_scale(None, 1.5), 2, "rounded, not truncated");
        assert_eq!(wanted_scale(None, 0.0), 1, "never zero, whatever is asked");
        // A chosen scale is kept, and kept inside what the renderer accepts.
        assert_eq!(wanted_scale(Some(3), 1.0), 3);
        assert_eq!(wanted_scale(Some(99), 1.0), 8);
        assert_eq!(wanted_scale(Some(0), 2.0), 1, "0 as a value, not as None");
    }

    #[test]
    fn utc_stamps() {
        assert_eq!(utc_stamp(0), "1970-01-01-000000");
        assert_eq!(utc_stamp(951_782_400), "2000-02-29-000000");
        assert_eq!(utc_stamp(1_798_765_432), "2027-01-01-010352");
    }

    #[test]
    fn ping_is_banded_by_quality() {
        use crate::game::view::color;
        // Unanswered is dimmed, not red: it is not a bad ping, it is no ping.
        assert_eq!(super::ping_color(None), color::DARK_RED);
        assert_eq!(super::ping_color(Some(0)), color::GREEN);
        // The zones measured against live servers sit around here.
        assert_eq!(super::ping_color(Some(72)), color::GREEN);
        assert_eq!(super::ping_color(Some(115)), color::YELLOW);
        // Band edges land on the better side.
        assert_eq!(super::ping_color(Some(100)), color::GREEN);
        assert_eq!(super::ping_color(Some(101)), color::YELLOW);
        assert_eq!(super::ping_color(Some(250)), color::YELLOW);
        assert_eq!(super::ping_color(Some(251)), color::RED);
    }
}
