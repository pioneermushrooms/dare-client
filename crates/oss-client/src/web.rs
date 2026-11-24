//! Browser platform entry point and bridge to the WebTransport datagram driver.
use std::io;
use web_time::Instant;
use wasm_bindgen::prelude::*;

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
        Ok(Self { link, started: Instant::now(), reported_endpoint: false })
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
