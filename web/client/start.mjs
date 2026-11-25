// The Foundation home page's game: downloads the client in the background, and Play signs
// the pilot in (only if the site doesn't know them yet), gets a one-time game pass and
// starts straight in the chosen arena. The page markup is the site's (/); this module is
// served from /play/ and finds its parts by id.
// The page loads this module as start.mjs?v=<version>; the same version goes on every file
// it fetches, so a browser holding an older copy of any of them can't mix the two.
const version = new URL(import.meta.url).searchParams.get("v") ?? "";
const here = (path) => new URL(version ? `${path}?v=${encodeURIComponent(version)}` : path, import.meta.url);
const { default: init, start_dare_in } = await import(here("pkg/oss_client.js"));
const { connectZone } = await import(here("transport.mjs"));

const WASM = here("pkg/oss_client_bg.wasm");
const BUILD = here("build.json");
const ARENA_KEY = "foundation.arena";
const HEADERS = { "Content-Type": "application/json", "X-Requested-With": "dare-arena" };

const $ = (id) => document.getElementById(id);
const entry = $("entry");
const button = $("start");
const label = $("start-label");
const status = $("status");
const picker = $("arena");
const pilotLink = $("pilot");
const newHere = $("newhere");
const dialog = $("signin");
const form = $("signin-form");
const signinMsg = $("signin-msg");
const forgot = $("signin-forgot");

// ---- the game's link to the zone (WebTransport to the Foundation gateway) ------------------
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

// ---- arenas: the picker on this page, and the feed the game's own start screen reads ------
// (the game shows its start screen after a pilot leaves the zone)
let arenaFeed;
let arenas = [];
let myDrafts = [];
const linkArena = new URLSearchParams(location.search).get("arena"); // the builder's "Fly in it"

function savedArena() {
  try { return localStorage.getItem(ARENA_KEY) ?? ""; } catch { return ""; }
}
function saveArena(name) {
  try { localStorage.setItem(ARENA_KEY, name); } catch {}
}

const clean = (name) => String(name ?? "").replace(/[\t\r\n]/g, " ").trim();

async function fetchArenas() {
  try {
    const response = await fetch("/arena/api/public", { cache: "no-store", signal: AbortSignal.timeout(10000) });
    if (!response.ok) throw new Error();
    const data = await response.json();
    const publicPlayers = Number.isInteger(data.publicPlayers) ? data.publicPlayers : null;
    arenas = [{ name: "", label: "public", players: publicPlayers }].concat(
      (data.arenas ?? [])
        .filter((arena) => arena.status === "Active" && clean(arena.name))
        .map((arena) => ({ name: clean(arena.name), label: clean(arena.name), players: Number.isInteger(arena.players) ? arena.players : null })),
    );
    const lines = arenas.filter((a) => a.name).map((a) => `${a.name}\t${a.players ?? ""}`);
    if (publicPlayers != null) lines.unshift(`public\t${publicPlayers}`);
    arenaFeed = lines.join("\n");
  } catch {
    arenaFeed = null;
    if (arenas.length === 0) arenas = [{ name: "", label: "public", players: null }];
  }
  renderPicker();
}
globalThis.foundationArenas = () => arenaFeed;
globalThis.refreshFoundationArenas = () => void fetchArenas();
globalThis.foundationStartArena = () => picker.value || undefined;
const FOUNDATION_PAGES = new Set(["/build.html", "/account/signup", "/account/forgot"]);
globalThis.openFoundationPage = (path) => {
  if (FOUNDATION_PAGES.has(path)) window.open(`https://subspacefoundation.com${path}`, "_blank", "noopener");
};

async function fetchDrafts() {
  myDrafts = [];
  if (!me) return renderPicker();
  try {
    const response = await fetch("/arena/api/drafts/mine", { cache: "no-store", signal: AbortSignal.timeout(10000) });
    if (response.ok) {
      const data = await response.json();
      myDrafts = (data.drafts ?? []).filter((d) => d.status !== "Archived").map((d) => `#${clean(d.name)}`);
    }
  } catch {}
  renderPicker();
}

function option(value, text) {
  const o = document.createElement("option");
  o.value = value;
  o.textContent = text;
  return o;
}

function renderPicker() {
  const want = picker.dataset.touched ? picker.value : (linkArena ?? savedArena());
  // the pilot count only where someone is flying
  const options = arenas.map((a) => option(a.name, a.players ? `${a.label} · ${a.players} flying` : a.label));
  const known = new Set(arenas.map((a) => a.name));
  if (myDrafts.length) {
    const group = document.createElement("optgroup");
    group.label = "Your drafts";
    for (const d of myDrafts) { group.append(option(d, d)); known.add(d); }
    options.push(group);
  }
  // an arena asked for by name that isn't listed (yet): keep it choosable
  if (want && !known.has(want)) options.push(option(want, want));
  picker.replaceChildren(...options);
  picker.value = want;
  if (picker.value !== want) picker.value = "";
}

picker.addEventListener("change", () => {
  picker.dataset.touched = "1";
  saveArena(picker.value);
});

// ---- who is signed in -------------------------------------------------------------------
let me = null; // the signed-in pilot's name, or null

async function whoAmI() {
  try {
    const response = await fetch("/account/api/me", { cache: "no-store", credentials: "same-origin" });
    me = response.ok ? (await response.json()).name ?? null : null;
  } catch {
    me = null;
  }
  showPilot();
}

function showPilot() {
  pilotLink.textContent = me ?? "Sign in";
  pilotLink.href = me ? "/account/" : "#signin";
  newHere.hidden = !!me;
}

pilotLink.addEventListener("click", (event) => {
  if (me) return;
  event.preventDefault();
  void askToSignIn().then(() => { if (me) void fetchDrafts(); });
});

// The sign-in box. Resolves when the pilot signed in (me is set) or gave up (me stays null).
function askToSignIn() {
  signinMsg.textContent = "";
  forgot.hidden = true;
  dialog.showModal();
  $("signin-name").focus();
  return new Promise((resolve) => {
    dialog.addEventListener("close", () => resolve(), { once: true });
  });
}

form.addEventListener("submit", async (event) => {
  event.preventDefault();
  const go = $("signin-go");
  go.disabled = true;
  signinMsg.textContent = "";
  try {
    const response = await fetch("/account/api/login", {
      method: "POST",
      credentials: "same-origin",
      headers: HEADERS,
      body: JSON.stringify({ name: $("signin-name").value.trim(), password: $("signin-password").value }),
    });
    const body = await response.json().catch(() => ({}));
    if (response.ok && body.ok) {
      me = body.name;
      $("signin-password").value = "";
      showPilot();
      dialog.close();
    } else {
      signinMsg.textContent = body.message || "Couldn't sign in.";
      forgot.hidden = false; // only after a failed try
    }
  } catch {
    signinMsg.textContent = "Couldn't reach the site. Check your connection and try again.";
  } finally {
    go.disabled = false;
  }
});
$("signin-cancel").addEventListener("click", () => dialog.close());

// ---- the download ------------------------------------------------------------------------
function say(text, isError = false) {
  status.textContent = text;
  status.classList.toggle("error", isError);
}

const megabytes = (bytes) => (bytes / 1e6).toFixed(1);

// What stops this browser from running the game, or null if nothing does.
async function missingFeature() {
  if (typeof WebTransport === "undefined" || !navigator.gpu) {
    return "This browser can't run the game yet. Use desktop Chrome or Edge.";
  }
  const adapter = await navigator.gpu.requestAdapter().catch(() => null);
  if (!adapter) {
    return "Your browser has graphics acceleration (WebGPU) turned off or unavailable. Turn it on, or use desktop Chrome or Edge.";
  }
  return null;
}

// The uncompressed size of the game, written by build-client.sh. The server may
// compress the download, so the response's own length can't measure progress.
async function expectedSize() {
  try {
    const response = await fetch(BUILD, { cache: "no-cache" });
    if (response.ok) return (await response.json()).wasmBytes ?? 0;
  } catch {}
  return 0;
}

let percent = 0;
// While the game downloads, the button says how far along it is (it can still be pressed).
function progress(value) {
  percent = value;
  button.style.setProperty("--p", `${value}%`);
  button.classList.toggle("downloading", value < 100);
  label.textContent = value < 100 ? `Loading ${value}%` : "Play";
  if (waiting) say(`Starting as soon as the game has loaded… ${value}%`);
}

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
    if (total > 0) progress(Math.min(99, Math.floor((received / total) * 100)));
  }
  const bytes = new Uint8Array(received);
  let offset = 0;
  for (const chunk of chunks) {
    bytes.set(chunk, offset);
    offset += chunk.length;
  }
  return bytes;
}

// ---- Play -----------------------------------------------------------------------------------
let waiting = false; // Play was pressed before the game finished loading
let gameReady;       // resolves when the game is loaded

async function gamePass() {
  const response = await fetch("/account/api/gamepass", { method: "POST", credentials: "same-origin", headers: HEADERS, body: "{}" });
  if (response.status === 401) return null;
  const body = await response.json().catch(() => ({}));
  if (!response.ok || !body.ok) throw new Error(body.message || "Couldn't get you into the zone. Try again.");
  return body;
}

async function play() {
  button.disabled = true;
  try {
    if (!me) await askToSignIn();
    if (!me) return; // cancelled
    if (percent < 100) {
      waiting = true;
      button.classList.add("loading");
      say(`Starting as soon as the game has loaded… ${percent}%`);
      await gameReady;
      waiting = false;
    }
    let pass = await gamePass();
    if (!pass) { // the site session ended: sign in again
      me = null;
      showPilot();
      await askToSignIn();
      if (!me) return;
      pass = await gamePass();
      if (!pass) throw new Error("Couldn't sign you in. Try again.");
    }
    enter(pass.name, pass.pass, picker.value);
  } catch (error) {
    say(error?.message ?? String(error), true);
  } finally {
    waiting = false;
    button.classList.remove("loading");
    if (!entry.hidden) button.disabled = false;
  }
}

function enter(name, pass, arena) {
  try {
    entry.hidden = true;
    document.body.classList.add("playing");
    const observer = new MutationObserver(() => {
      const canvas = document.querySelector("canvas");
      if (canvas) {
        canvas.tabIndex = 0;
        canvas.focus();
        observer.disconnect();
      }
    });
    observer.observe(document.body, { childList: true, subtree: true });
    start_dare_in(name, pass, arena);
  } catch (error) {
    entry.hidden = false;
    document.body.classList.remove("playing");
    say(error?.message ?? String(error), true);
  }
}

// ---- start ----------------------------------------------------------------------------------
void fetchArenas();
setInterval(() => { if (!entry.hidden) void fetchArenas(); }, 30000);
void whoAmI().then(fetchDrafts);

const missing = await missingFeature();
if (missing) {
  say(missing, true);
} else {
  button.disabled = false;
  progress(0);
  button.addEventListener("click", play);
  gameReady = (async () => {
    const bytes = await download();
    await init({ module_or_path: bytes });
    progress(100);
  })();
  gameReady.catch((error) => {
    button.disabled = true;
    say(error?.message ?? String(error), true);
  });
}
