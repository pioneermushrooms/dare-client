import init, { start_dare } from "./pkg/oss_client.js";
import { connectZone } from "./transport.mjs";

const WASM = "pkg/oss_client_bg.wasm";
const button = document.querySelector("#start");
const status = document.querySelector("#status");
const entry = document.querySelector("#entry");
const progress = document.querySelector("#progress");
const bar = progress.querySelector("span");

globalThis.openDareGateway = () => {
  const queue = [];
  let link;
  let closed = false;
  const bridge = {
    serverIp: 0,
    boundPort: 0,
    error: undefined,
    receive: () => queue.shift(),
    send: (packet) => link?.send(packet),
    close() {
      closed = true;
      queue.length = 0;
      link?.close();
    },
  };
  function failed(error) {
    if (!closed) bridge.error = error?.message ?? String(error ?? "Gateway disconnected.");
  }
  void connectZone(
    "https://subspacefoundation.com:4443/play/transport",
    (packet) => {
      if (!closed && queue.length < 256) queue.push(packet);
    },
    failed,
  ).then((connected) => {
    if (closed) {
      connected.close();
      return;
    }
    link = connected;
    bridge.serverIp = link.serverIp;
    bridge.boundPort = link.boundPort;
  }).catch(failed);
  return bridge;
};

// Foundation's arenas for the game's start screen, as "name<TAB>players" lines.
// undefined until the first answer, null after a failed fetch.
let arenaFeed;
let arenaFetching = false;
async function fetchArenas() {
  if (arenaFetching) return;
  arenaFetching = true;
  try {
    const response = await fetch("/arena/api/public", { cache: "no-store", signal: AbortSignal.timeout(10000) });
    if (!response.ok) throw new Error();
    const data = await response.json();
    const clean = (name) => String(name ?? "").replace(/[\t\r\n]/g, " ").trim();
    const lines = (data.arenas ?? [])
      .filter((arena) => arena.status === "Active" && clean(arena.name))
      .map((arena) => `${clean(arena.name)}\t${Number.isInteger(arena.players) ? arena.players : ""}`);
    if (Number.isInteger(data.publicPlayers)) lines.unshift(`public\t${data.publicPlayers}`);
    arenaFeed = lines.join("\n");
  } catch {
    arenaFeed = null;
  } finally {
    arenaFetching = false;
  }
}
globalThis.foundationArenas = () => arenaFeed;
globalThis.refreshFoundationArenas = () => void fetchArenas();
const FOUNDATION_PAGES = new Set(["/build.html", "/account/signup", "/account/forgot"]);
// /play/?arena=%23name (the builder's "Fly in it") starts the game with that arena filled in.
const startArena = new URLSearchParams(location.search).get("arena");
globalThis.foundationStartArena = () => startArena ?? undefined;
globalThis.openFoundationPage = (path) => {
  if (FOUNDATION_PAGES.has(path)) window.open(`https://subspacefoundation.com${path}`, "_blank", "noopener");
};
void fetchArenas();
setInterval(() => void fetchArenas(), 30000);

function say(text, isError = false) {
  status.textContent = text;
  status.classList.toggle("error", isError);
}

function megabytes(bytes) {
  return (bytes / 1e6).toFixed(1);
}

// What stops this browser from running the game, or null if nothing does.
async function missingFeature() {
  if (typeof WebTransport === "undefined" || !navigator.gpu) {
    return "This browser can't run the game yet. Use desktop Chrome or Edge.";
  }
  const adapter = await navigator.gpu.requestAdapter().catch(() => null);
  if (!adapter) {
    return "Your browser has graphics acceleration (WebGPU) turned off or unavailable. Turn it on, or try desktop Chrome or Edge.";
  }
  return null;
}

// The uncompressed size of the game, written by build-client.sh. The server may
// compress the download, so the response's own length can't measure progress.
async function expectedSize() {
  try {
    const response = await fetch("build.json", { cache: "no-cache" });
    if (response.ok) return (await response.json()).wasmBytes ?? 0;
  } catch {}
  return 0;
}

// Downloads the game, reporting progress, and returns its bytes.
async function download() {
  const [total, response] = await Promise.all([expectedSize(), fetch(WASM)]);
  if (!response.ok || !response.body) throw new Error(`The game download failed (${response.status}).`);
  const reader = response.body.getReader();
  const chunks = [];
  let received = 0;
  for (;;) {
    const { value, done } = await reader.read();
    if (done) break;
    chunks.push(value);
    received += value.length;
    if (total > 0) {
      const percent = Math.min(99, Math.floor((received / total) * 100));
      bar.style.width = `${percent}%`;
      progress.setAttribute("aria-valuenow", String(percent));
      say(`Downloading the game… ${megabytes(received)} of ${megabytes(total)} MB`);
    } else {
      say(`Downloading the game… ${megabytes(received)} MB`);
    }
  }
  const bytes = new Uint8Array(received);
  let offset = 0;
  for (const chunk of chunks) {
    bytes.set(chunk, offset);
    offset += chunk.length;
  }
  return bytes;
}

// The zone directory: every zone the public directory servers list, by name.
// Only zones marked `browser` can be joined from this page.
const zoneList = document.querySelector("#zone-list");
const zonesMeta = document.querySelector("#zones-meta");
const browserOnly = document.querySelector("#browser-only");
const FOUNDATION = { name: "SubSpace Foundation", description: "The shared zone with player-owned arenas.", players: null, browser: true };
let zones = null;
let ready = false;

function node(tag, className, text) {
  const element = document.createElement(tag);
  if (className) element.className = className;
  if (text != null) element.textContent = text;
  return element;
}

function pilots(count) {
  return count == null ? "" : `${count} ${count === 1 ? "pilot" : "pilots"}`;
}

function renderZones() {
  if (!zones) return;
  const shown = browserOnly.checked ? zones.filter((zone) => zone.browser) : zones;
  const rows = shown.map((zone) => {
    const row = node("li", zone.browser ? "playable" : "");
    row.append(node("span", "name", zone.name), node("span", "desc", zone.description || ""));
    const side = node("span", "side");
    if (zone.browser) {
      const play = node("button", "tag browser", ready ? "Play in browser" : "Browser · loading");
      play.type = "button";
      play.disabled = !ready;
      play.setAttribute("aria-label", `Play ${zone.name} in the browser`);
      play.addEventListener("click", () => button.click());
      side.append(play);
    } else {
      side.append(node("span", "tag desktop", "Desktop client"));
    }
    side.append(node("span", "pilots", pilots(zone.players)));
    row.append(side);
    return row;
  });
  if (rows.length === 0) {
    const empty = node("li");
    empty.append(node("span", "note", "No zones to show."));
    rows.push(empty);
  }
  zoneList.replaceChildren(...rows);
}

async function loadZones() {
  try {
    const response = await fetch("/arena/api/zones", { cache: "no-store", signal: AbortSignal.timeout(10000) });
    if (!response.ok) throw new Error();
    const data = await response.json();
    zones = (data.zones ?? []).map((zone) => ({
      name: String(zone.name ?? ""),
      description: String(zone.description ?? ""),
      players: Number.isFinite(zone.players) ? zone.players : null,
      browser: zone.browser === true,
    }));
    // The server fills its list a moment after its first request; ask again shortly.
    if (zones.length === 0 && data.at == null) {
      zonesMeta.textContent = "Asking the directory servers…";
      setTimeout(loadZones, 3000);
    } else {
      const playable = zones.filter((zone) => zone.browser).length;
      zonesMeta.textContent = `${zones.length} ${zones.length === 1 ? "zone" : "zones"} · ${playable} in browser`;
    }
  } catch {
    zones = [];
    zonesMeta.textContent = "Directory unavailable";
  }
  // Foundation is always playable here, listed or not.
  if (!zones.some((zone) => zone.browser)) zones = [FOUNDATION, ...zones];
  renderZones();
}

browserOnly.addEventListener("change", renderZones);
void loadZones();
setInterval(() => { if (!entry.hidden) void loadZones(); }, 60000);

function enter() {
  try {
    entry.hidden = true;
    const observer = new MutationObserver(() => {
      const canvas = document.querySelector("canvas");
      if (canvas) {
        canvas.tabIndex = 0;
        canvas.focus();
        observer.disconnect();
      }
    });
    observer.observe(document.body, { childList: true, subtree: true });
    start_dare();
  } catch (error) {
    entry.hidden = false;
    say(error?.message ?? String(error), true);
  }
}

try {
  const missing = await missingFeature();
  if (missing) throw new Error(missing);
  const bytes = await download();
  say("Starting…");
  await init({ module_or_path: bytes });
  progress.hidden = true;
  say("Ready.");
  button.disabled = false;
  ready = true;
  renderZones();
  button.focus();
  button.addEventListener("click", enter, { once: true });
} catch (error) {
  progress.hidden = true;
  say(error?.message ?? String(error), true);
}
