//! Browser platform entry point and bridge to the WebTransport datagram driver.
use std::io;
use wasm_bindgen::prelude::*;
use web_time::Instant;

#[wasm_bindgen]
extern "C" {
    type Link;

    #[wasm_bindgen(js_namespace = console, js_name = error)]
    fn log_error(message: &str);

    #[wasm_bindgen(js_namespace = console, js_name = info)]
    fn log_info(message: &str);

    #[wasm_bindgen(js_namespace = globalThis, js_name = openDareGateway, catch)]
    fn open_gateway() -> Result<Link, JsValue>;

    #[wasm_bindgen(method, getter, js_name = serverIp)]
    fn server_ip(this: &Link) -> u32;

    #[wasm_bindgen(method, getter, js_name = boundPort)]
    fn bound_port(this: &Link) -> u16;

    #[wasm_bindgen(method, getter)]
    fn error(this: &Link) -> Option<String>;

    #[wasm_bindgen(method)]
    fn receive(this: &Link) -> Option<js_sys::Uint8Array>;

    #[wasm_bindgen(method)]
    fn send(this: &Link, packet: &[u8]);

    #[wasm_bindgen(method)]
    fn close(this: &Link);

    #[wasm_bindgen(js_namespace = globalThis, js_name = foundationArenas, catch)]
    fn js_foundation_arenas() -> Result<JsValue, JsValue>;

    #[wasm_bindgen(js_namespace = globalThis, js_name = refreshFoundationArenas, catch)]
    fn js_refresh_foundation_arenas() -> Result<(), JsValue>;

    #[wasm_bindgen(js_namespace = globalThis, js_name = openFoundationPage, catch)]
    fn js_open_foundation_page(path: &str) -> Result<(), JsValue>;

    #[wasm_bindgen(js_namespace = globalThis, js_name = foundationStartArena, catch)]
    fn js_start_arena() -> Result<JsValue, JsValue>;

    #[wasm_bindgen(js_namespace = globalThis, js_name = foundationLeaveZone, catch)]
    fn js_leave_zone(reason: Option<String>) -> Result<(), JsValue>;

    #[wasm_bindgen(js_namespace = globalThis, js_name = foundationOpenGraphics, catch)]
    fn js_open_graphics() -> Result<JsValue, JsValue>;

    #[wasm_bindgen(js_namespace = globalThis, js_name = dareLoadingStage, catch)]
    fn js_loading_stage(stage: &str, detail: &str, percent: i32) -> Result<(), JsValue>;
}

/// Tells the page where the way into the arena has got to, so its loader can
/// show the same steps and hand over to the game's own screen without a jump:
/// `stage` is a [`crate::game::online::Stage::key`] (or `playing` once the arena
/// is on screen), `detail` the line under the bar, and `percent` how far
/// through this step's downloads, or -1 when that is not known. The first call
/// comes once the game has drawn its connecting screen. A page without the hook
/// is fine: nothing happens.
pub(crate) fn loading_stage(stage: &str, detail: &str, percent: i32) {
    let _ = js_loading_stage(stage, detail, percent);
}

/// Asks the page to open its custom graphics overlay. False when the page has none.
pub(crate) fn open_graphics() -> bool {
    js_open_graphics().is_ok_and(|v| v.as_bool() != Some(false))
}

/// Uses `sheet` (a PNG or BMP of 40 square frames in 10 columns and 4 rows; frame
/// 0 nose-up, then clockwise in 9 degree steps) for ship `ship` (1..8) for this
/// player. The game picks it up on its next frame; before a visit, it is used
/// from the start.
#[wasm_bindgen]
pub fn set_custom_ship(ship: u8, sheet: &[u8]) -> Result<(), JsValue> {
    let img = oss_assets::decode_image(sheet).map_err(|e| JsValue::from_str(&e.to_string()))?;
    crate::game::custom_ships::set(slot(ship)?, Some(img)).map_err(|e| JsValue::from_str(&e))
}

/// Gives ship `ship` (1..8) back to the arena's or the game's own graphics.
#[wasm_bindgen]
pub fn clear_custom_ship(ship: u8) -> Result<(), JsValue> {
    crate::game::custom_ships::set(slot(ship)?, None).map_err(|e| JsValue::from_str(&e))
}

/// The sheet ship `ship` (1..8) is drawn with now, as `{ width, height, rgba }`
/// (laid out like [`set_custom_ship`]'s), or `null` before the game has built
/// its graphics.
#[wasm_bindgen]
pub fn ship_sheet(ship: u8) -> Result<JsValue, JsValue> {
    let Some(img) = crate::game::custom_ships::shown(slot(ship)?) else {
        return Ok(JsValue::NULL);
    };
    let out = js_sys::Object::new();
    js_sys::Reflect::set(&out, &"width".into(), &img.width.into())?;
    js_sys::Reflect::set(&out, &"height".into(), &img.height.into())?;
    js_sys::Reflect::set(
        &out,
        &"rgba".into(),
        &js_sys::Uint8ClampedArray::from(&img.rgba[..]),
    )?;
    Ok(out.into())
}

/// Uses `sheet` (PNG or BMP bytes, laid out like the game's own sheet of that
/// name) for the graphic `name` (bombs, bullets, prizes...; not ships, which are
/// set one by one) for this player. The game picks it up on its next frame.
#[wasm_bindgen]
pub fn set_custom_graphic(name: &str, sheet: &[u8]) -> Result<(), JsValue> {
    let img = oss_assets::decode_image(sheet).map_err(|e| JsValue::from_str(&e.to_string()))?;
    crate::game::custom_ships::set_graphic(name, Some(img)).map_err(|e| JsValue::from_str(&e))
}

/// Gives the graphic `name` back to the arena's or the game's own.
#[wasm_bindgen]
pub fn clear_custom_graphic(name: &str) -> Result<(), JsValue> {
    crate::game::custom_ships::set_graphic(name, None).map_err(|e| JsValue::from_str(&e))
}

/// Whether the pilot's own ships and graphics are shown (true) or the arena's.
#[wasm_bindgen]
pub fn use_own_graphics(on: bool) {
    crate::game::custom_ships::set_use_own(on);
}

fn slot(ship: u8) -> Result<usize, JsValue> {
    match ship {
        1..=8 => Ok(ship as usize - 1),
        _ => Err(JsValue::from_str("ships are numbered 1 to 8")),
    }
}

/// Set by [`start_dare_in`]: the page started this run, so leaving the zone goes
/// back to the page rather than to the start-up screen.
static STARTED_FROM_PAGE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Hands the pilot back to the page that started the game, with why if the visit
/// ended on its own (a failed or lost connection). True when the page took over;
/// false for a run the page didn't start, or a page without the hook.
pub(crate) fn return_to_page(reason: Option<&str>) -> bool {
    STARTED_FROM_PAGE.load(std::sync::atomic::Ordering::Relaxed)
        && js_leave_zone(reason.map(str::to_owned)).is_ok()
}

/// The latest arena list from the page: `None` before its first answer (the page
/// gives `undefined`), `Some(None)` when the last fetch failed (`null`), else the
/// `name<TAB>players` lines, which may be empty.
pub(crate) fn foundation_arenas() -> Option<Option<String>> {
    let value = js_foundation_arenas().ok()?;
    if value.is_undefined() {
        None
    } else {
        Some(value.as_string())
    }
}

/// The arena the page was opened for (`/play/?arena=...`, e.g. a draft from the
/// builder's "Fly in it"), if any. Only arena-name characters are kept.
pub(crate) fn start_arena() -> Option<String> {
    let name = js_start_arena().ok()?.as_string()?;
    let name: String = name
        .trim()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '#')
        .take(16)
        .collect();
    (!name.is_empty()).then_some(name.to_ascii_lowercase())
}

/// Asks the page to fetch the arena list now.
pub(crate) fn refresh_foundation_arenas() {
    let _ = js_refresh_foundation_arenas();
}

/// A Foundation site page the game may open in a new tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FoundationPage {
    Apply,
    Register,
    Forgot,
}

impl FoundationPage {
    fn path(self) -> &'static str {
        match self {
            FoundationPage::Apply => "/build.html",
            FoundationPage::Register => "/account/signup",
            FoundationPage::Forgot => "/account/forgot",
        }
    }
}

/// Opens `page` in a new tab. Only from a click or key the player made.
pub(crate) fn open_foundation_page(page: FoundationPage) {
    let _ = js_open_foundation_page(page.path());
}

struct BrowserLog;

impl log::Log for BrowserLog {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.target().starts_with("oss_") && metadata.level() <= log::Level::Info
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            let message = record.args().to_string();
            if record.level() == log::Level::Error {
                log_error(&message);
            } else {
                log_info(&message);
            }
        }
    }

    fn flush(&self) {}
}

static LOGGER: BrowserLog = BrowserLog;

#[wasm_bindgen]
pub fn start_dare() -> Result<(), JsValue> {
    if log::set_logger(&LOGGER).is_ok() {
        log::set_max_level(log::LevelFilter::Info);
    }
    std::panic::set_hook(Box::new(|error| log_error(&error.to_string())));
    let mut options = crate::options(std::iter::empty())
        .map_err(|error| JsValue::from_str(&error))?
        .ok_or_else(|| JsValue::from_str("Dare client options unavailable"))?;
    options.directories.clear();
    crate::run(options).map_err(|error| JsValue::from_str(&error))
}

/// Starts the game straight in the Foundation zone, signed in as `name`, without the
/// start-up screen: the page has already signed the pilot in, and `password` is what
/// it was given to log in with. `arena` is the arena to enter, or empty for any
/// public one. `settings` is the pilot's own keys and macros, as the `[keys]` and
/// `[macros]` sections of the settings file (empty for the defaults); other
/// sections in it are ignored. Leaving the zone goes back to the page.
#[wasm_bindgen]
pub fn start_dare_in(
    name: &str,
    password: &str,
    arena: &str,
    settings: &str,
) -> Result<(), JsValue> {
    if log::set_logger(&LOGGER).is_ok() {
        log::set_max_level(log::LevelFilter::Info);
    }
    std::panic::set_hook(Box::new(|error| log_error(&error.to_string())));
    let name = name.trim();
    if name.is_empty() {
        return Err(JsValue::from_str(
            "A pilot name is needed to start in the zone",
        ));
    }
    let mut args = vec![
        "--server".to_string(),
        "5.161.238.245:5000".to_string(),
        "--name".to_string(),
        name.to_string(),
        "--password".to_string(),
        password.to_string(),
    ];
    let arena = arena.trim();
    if !arena.is_empty() {
        args.push("--arena".to_string());
        args.push(arena.to_string());
    }
    let mut options = crate::options(args.into_iter())
        .map_err(|error| JsValue::from_str(&error))?
        .ok_or_else(|| JsValue::from_str("Dare client options unavailable"))?;
    options.directories.clear();
    if !settings.trim().is_empty() {
        let (pilot, warnings) = crate::config::Config::parse(settings);
        for warning in warnings {
            log::warn!("pilot settings: {warning}");
        }
        options.bindings = pilot.bindings;
        options.macros = pilot.macros;
    }
    STARTED_FROM_PAGE.store(true, std::sync::atomic::Ordering::Relaxed);
    crate::run(options).map_err(|error| JsValue::from_str(&error))
}

pub(crate) struct WebDriver {
    link: Link,
    started: Instant,
    reported_endpoint: bool,
}

impl WebDriver {
    pub fn connect(server: &str) -> io::Result<Self> {
        if !matches!(server, "5.161.238.245:5000" | "subspacefoundation.com:5000") {
            return Err(io::Error::other("Browser play connects to Foundation only"));
        }
        let link = open_gateway()
            .map_err(|error| io::Error::other(format!("browser transport: {error:?}")))?;
        Ok(Self {
            link,
            started: Instant::now(),
            reported_endpoint: false,
        })
    }

    pub fn now_ms(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }

    pub fn pump(&mut self, session: &mut oss_session::Session) -> io::Result<()> {
        if let Some(error) = self.link.error() {
            return Err(io::Error::other(error));
        }
        let port = self.link.bound_port();
        if port == 0 {
            return Ok(());
        }
        if !self.reported_endpoint {
            session.set_local_endpoint(self.link.server_ip(), port);
            self.reported_endpoint = true;
        }
        while let Some(packet) = self.link.receive() {
            session.handle_datagram(self.now_ms(), &packet.to_vec());
        }
        session.tick(self.now_ms());
        self.send_pending(session)
    }

    pub fn send_pending(&mut self, session: &mut oss_session::Session) -> io::Result<()> {
        if !self.reported_endpoint {
            return Ok(());
        }
        while let Some(packet) = session.poll_transmit() {
            self.link.send(&packet);
        }
        Ok(())
    }
}

impl Drop for WebDriver {
    fn drop(&mut self) {
        self.link.close();
    }
}
