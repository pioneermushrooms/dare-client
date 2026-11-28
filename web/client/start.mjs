// The Foundation home page's game: downloads the client in the background, and Play signs
// the pilot in (only if the site doesn't know them yet), gets a one-time game pass and
// starts straight in the chosen arena. The page markup is the site's (/); this module is
// served from /play/ and finds its parts by id.
// The page loads this module as start.mjs?v=<version>; the same version goes on every file
// it fetches, so a browser holding an older copy of any of them can't mix the two.
const version = new URL(import.meta.url).searchParams.get("v") ?? "";
const here = (path) => new URL(version ? `${path}?v=${encodeURIComponent(version)}` : path, import.meta.url);
// The game (its script, its WebAssembly and the zone link) starts downloading and compiling in
// the background once the page has loaded, so it is ready, or nearly, by the time the pilot
// presses Enter. Its sounds come separately after the game has started.
let game = null; // { init, start_dare_in } once its script is loaded
let connectZone = null;

const WASM = here("pkg/oss_client_bg.wasm");
const SOUNDS = here("pkg/sounds.pack");
const GATEWAY = "https://subspacefoundation.com:4443/play/transport";
const BUILD = here("build.json");
const ARENA_KEY = "foundation.arena";
const HEADERS = { "Content-Type": "application/json", "X-Requested-With": "dare-arena" };

const $ = (id) => document.getElementById(id);
const entry = $("entry");
const button = $("start");
const label = $("start-label");
const status = $("status");
const picker = $("arena");
const pilotButton = $("pilot");
const pilotMenu = $("pilotmenu");
const newHere = $("newhere");
const dialog = $("signin");
const form = $("signin-form");
const signinMsg = $("signin-msg");
const forgot = $("signin-forgot");

// ---- the game's link to the zone (WebTransport to the Foundation gateway) ------------------
// Opened when Enter is pressed, while the sign-in box and the download finish; the game takes
// it over when it starts (one use only), or opens its own.
let warmGateway = null; // { connecting, sink, fail } until the game takes it
function warmUpGateway() {
  if (warmGateway || !connectZone) return;
  const warm = { sink: null, fail: null, early: [] };
  warm.connecting = connectZone(
    GATEWAY,
    (packet) => (warm.sink ? warm.sink(packet) : warm.early.length < 256 && warm.early.push(packet)),
    (error) => { warm.failed = error ?? new Error("Gateway disconnected."); warm.fail?.(warm.failed); },
  );
  warm.connecting.catch(() => {});
  warmGateway = warm;
  // Not used in time (the pilot went back): let it go rather than hold a slot at the gateway.
  setTimeout(() => {
    if (warmGateway === warm) {
      warmGateway = null;
      warm.connecting.then((link) => link.close(), () => {});
    }
  }, 40000); // inside the gateway's 45 s idle limit
}

globalThis.openDareGateway = () => {
  const queue = [];
  const unsent = []; // what the game sends before the link is open (its first connect packet)
  let link;
  let closed = false;
  const bridge = {
    serverIp: 0,
    boundPort: 0,
    error: undefined,
    receive: () => queue.shift(),
    send: (packet) => {
      if (link) link.send(packet);
      else if (!closed && unsent.length < 16) unsent.push(packet.slice());
    },
    close() {
      closed = true;
      queue.length = 0;
      link?.close();
    },
  };
  function failed(error) {
    if (!closed) bridge.error = error?.message ?? String(error ?? "Gateway disconnected.");
  }
  const onPacket = (packet) => {
    if (!closed && queue.length < 256) queue.push(packet);
  };
  const warm = warmGateway && !warmGateway.failed ? warmGateway : null;
  warmGateway = null;
  let connecting;
  if (warm) {
    warm.sink = onPacket;
    warm.fail = failed;
    for (const packet of warm.early.splice(0)) onPacket(packet);
    // (if it failed meanwhile, a fresh one)
    connecting = warm.connecting.catch(() => connectZone(GATEWAY, onPacket, failed));
  } else {
    connecting = connectZone(GATEWAY, onPacket, failed);
  }
  void connecting.then((connected) => {
    if (closed) {
      connected.close();
      return;
    }
    link = connected;
    bridge.serverIp = link.serverIp;
    bridge.boundPort = link.boundPort;
    for (const packet of unsent.splice(0)) link.send(packet);
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
    // every arena the zone has, as its ?arena -a list shows them ("0" is the public arena)
    const response = await fetch("/arena/api/arenas", { cache: "no-store", signal: AbortSignal.timeout(10000) });
    if (!response.ok) throw new Error();
    const data = await response.json();
    arenas = (data.arenas ?? [])
      .filter((arena) => clean(arena.name))
      .map((arena) => {
        const name = clean(arena.name);
        return { name: name === "0" ? "" : name, label: name === "0" ? "public" : name, players: Number.isInteger(arena.players) ? arena.players : 0 };
      });
    if (!arenas.some((a) => a.name === "")) arenas.unshift({ name: "", label: "public", players: 0 });
    arenaFeed = arenas.map((a) => `${a.label}\t${a.players}`).join("\n");
  } catch {
    arenaFeed = null;
    if (arenas.length === 0) arenas = [{ name: "", label: "public", players: 0 }];
  }
  renderPicker();
}
globalThis.foundationArenas = () => arenaFeed;
globalThis.refreshFoundationArenas = () => void fetchArenas();
globalThis.foundationStartArena = () => picker.value || undefined;
// Leaving the zone (Exit, or a lost connection) comes back here: a fresh home page, with why.
const LEFT_KEY = "foundation.left";
globalThis.foundationLeaveZone = (reason) => {
  try { sessionStorage.setItem(LEFT_KEY, reason ? String(reason) : ""); } catch {}
  location.reload();
};
const FOUNDATION_PAGES = new Set(["/build.html", "/account/signup", "/account/forgot"]);
globalThis.openFoundationPage = (path) => {
  if (FOUNDATION_PAGES.has(path)) window.open(`https://subspacefoundation.com${path}`, "_blank", "noopener");
};

// The pilot's own arenas (owner or co-mod), so a private one is choosable too.
async function fetchDrafts() {
  myDrafts = [];
  if (!me) return renderPicker();
  try {
    const response = await fetch("/arena/api/me", { cache: "no-store", credentials: "same-origin", signal: AbortSignal.timeout(10000) });
    if (response.ok) {
      const data = await response.json();
      // (#name copies left from the old draft workflow aren't arenas to fly in)
      myDrafts = (data.arenas ?? []).map((a) => clean(a.name)).filter((n) => n && !n.startsWith("#"));
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
  // how many are in each, so you know what you're getting into
  const options = arenas.map((a) => option(a.name, `${a.label} · ${a.players}`));
  const known = new Set(arenas.map((a) => a.name));
  if (myDrafts.length) {
    const group = document.createElement("optgroup");
    group.label = "Your private arenas";
    for (const d of myDrafts) if (!known.has(d)) { group.append(option(d, d)); known.add(d); }
    if (group.children.length) options.push(group);
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
  $("pilot-name").textContent = me ?? "Sign in";
  $("pilot-label").hidden = !me;
  $("pilot-caret").hidden = !me;
  newHere.hidden = !!me;
  if (!me) closePilotMenu();
}

// ---- the pilot menu: the names on this pilot's email (switch), account links, sign out -------
function closePilotMenu() {
  pilotMenu.hidden = true;
  pilotButton.setAttribute("aria-expanded", "false");
}

async function openPilotMenu() {
  const list = $("pilotlist");
  list.replaceChildren(option("", "…"));
  pilotMenu.hidden = false;
  pilotButton.setAttribute("aria-expanded", "true");
  let names = [me];
  try {
    const response = await fetch("/account/api/pilots", { cache: "no-store", credentials: "same-origin" });
    if (response.status === 401) { me = null; showPilot(); return; }
    if (response.ok) names = ((await response.json()).pilots ?? []).map((p) => p.name);
  } catch {}
  list.replaceChildren(...names.map((name) => {
    const item = document.createElement("button");
    item.type = "button";
    item.setAttribute("role", "menuitemradio");
    item.setAttribute("aria-checked", String(name === me));
    item.textContent = name;
    if (name === me) item.className = "current";
    else item.addEventListener("click", () => void switchPilot(name));
    return item;
  }));
}

async function switchPilot(name) {
  try {
    const response = await fetch("/account/api/pilots/switch", { method: "POST", credentials: "same-origin", headers: HEADERS, body: JSON.stringify({ name }) });
    const body = await response.json().catch(() => ({}));
    if (!response.ok || !body.ok) throw new Error(body.message || "Couldn't switch names.");
    me = body.name;
    showPilot();
    closePilotMenu();
    void fetchDrafts();
  } catch (error) {
    say(error?.message ?? String(error), true);
  }
}

pilotButton.addEventListener("click", () => {
  if (!me) {
    void askToSignIn("account").then(() => { if (me) void fetchDrafts(); });
  } else if (pilotMenu.hidden) {
    void openPilotMenu();
  } else {
    closePilotMenu();
  }
});
document.addEventListener("click", (event) => {
  if (!pilotMenu.hidden && !pilotMenu.contains(event.target) && !pilotButton.contains(event.target)) closePilotMenu();
});
document.addEventListener("keydown", (event) => {
  if (event.key === "Escape" && !pilotMenu.hidden) { closePilotMenu(); pilotButton.focus(); }
});
$("signout").addEventListener("click", async () => {
  try {
    await fetch("/account/api/logout", { method: "POST", credentials: "same-origin", headers: HEADERS, body: "{}" });
  } catch {}
  me = null;
  showPilot();
  void fetchDrafts();
});

// For the page's other parts (the chat panel): the signed-in pilot, asking to sign in if needed.
globalThis.foundationSignIn = async () => {
  if (!me) {
    await askToSignIn("chat");
    if (me) void fetchDrafts();
  }
  return me;
};

// The sign-in box, worded for why it opened ("fly", "chat", or just signing in). Resolves
// when the pilot signed in (me is set) or gave up (me stays null).
const SIGNIN_WORDS = {
  fly: ["Sign in to fly", "Sign in and enter"],
  chat: ["Sign in to chat", "Sign in"],
  account: ["Sign in", "Sign in"],
};
let flyAsGuest = false; // the sign-in box's "Fly as a guest" was chosen
let signedInOrNot = () => {}; // ends the wait for the sign-in box
function askToSignIn(why = "fly") {
  const [title, button] = SIGNIN_WORDS[why] ?? SIGNIN_WORDS.fly;
  $("signin-title").textContent = title;
  $("signin-go").textContent = button;
  signinMsg.textContent = "";
  forgot.hidden = true;
  flyAsGuest = false;
  const guestBox = $("signin-guestbox");
  if (guestBox) guestBox.hidden = why !== "fly"; // guests fly; they don't chat on the site
  dialog.showModal();
  $("signin-name").focus();
  return new Promise((resolve) => {
    // Resolved by the box's own buttons as well as its close event, which a browser may hold
    // back until it next draws the page.
    signedInOrNot = resolve;
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
      signedInOrNot();
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
// A guest: the zone gives the name "guest" a fresh random GuestNNNN name, no password needed.
$("signin-guest")?.addEventListener("click", () => {
  flyAsGuest = true;
  dialog.close();
  signedInOrNot();
});

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
let showProgress = false; // after Enter: the button shows how far the download is
// While the game downloads (after Enter), the button says how far along it is.
function progress(value) {
  percent = value;
  if (!showProgress) return;
  button.style.setProperty("--p", `${value}%`);
  button.classList.toggle("downloading", value < 100);
  label.textContent = value < 100 ? `Loading ${value}%` : "Enter";
}

// Downloads and compiles the game in one go (the browser compiles as the bytes arrive).
async function download() {
  const [total, response] = await Promise.all([expectedSize(), fetch(WASM)]);
  if (!response.ok || !response.body) throw new Error(`The game download failed (${response.status}).`);
  let received = 0;
  const counted = response.body.pipeThrough(new TransformStream({
    transform(chunk, controller) {
      received += chunk.length;
      if (total > 0) progress(Math.min(99, Math.floor((received / total) * 100)));
      controller.enqueue(chunk);
    },
  }));
  return WebAssembly.compileStreaming(new Response(counted, { headers: { "Content-Type": "application/wasm" } }));
}

// ---- what the browser keeps between visits: the zone's maps, LVZ and news -------------------
// The game can't write files here; it hands each download to dareCacheSave, kept in IndexedDB,
// and gets them all back before it starts, so a return visit needn't download them again.
const CACHE_DB = "dare-zone-files";
function cacheDb() {
  return new Promise((resolve, reject) => {
    const open = indexedDB.open(CACHE_DB, 2);
    open.onupgradeneeded = () => {
      if (!open.result.objectStoreNames.contains("files")) open.result.createObjectStore("files");
    };
    open.onsuccess = () => resolve(open.result);
    open.onerror = () => reject(open.error);
  });
}
// (never holding up the start: storage that is slow, blocked or broken just means downloading)
function cachedFiles() {
  return Promise.race([readCachedFiles(), new Promise((resolve) => setTimeout(() => resolve([]), 2000))]);
}
async function readCachedFiles() {
  try {
    const db = await cacheDb();
    const files = await new Promise((resolve, reject) => {
      const store = db.transaction("files").objectStore("files");
      const keys = store.getAllKeys();
      const values = store.getAll();
      values.onsuccess = () => resolve(keys.result.map((key, i) => [key, values.result[i]]));
      values.onerror = () => reject(values.error);
    });
    db.close();
    return files;
  } catch {
    return [];
  }
}
globalThis.dareCacheSave = (name, data) => {
  const bytes = data.slice(); // a copy: the game's memory may move
  void cacheDb().then((db) => {
    const tx = db.transaction("files", "readwrite");
    tx.objectStore("files").put(bytes, String(name));
    tx.oncomplete = tx.onerror = () => db.close();
  }).catch(() => {});
};

// ---- the game's sounds: fetched once the game has started, then handed to it ------------------
// (pkg/sounds.pack, from build-client.sh: a 1-byte name length, the name, a 4-byte size, the file)
async function loadSounds() {
  try {
    const response = await fetch(SOUNDS, { priority: "low" });
    if (!response.ok) throw new Error(`sounds: ${response.status}`);
    const pack = new Uint8Array(await response.arrayBuffer());
    const view = new DataView(pack.buffer);
    const names = new TextDecoder();
    let at = 0;
    while (at < pack.length) {
      const nameLength = pack[at];
      const name = names.decode(pack.subarray(at + 1, at + 1 + nameLength));
      at += 1 + nameLength;
      const size = view.getUint32(at, true);
      at += 4;
      game.provide_asset(name, pack.subarray(at, at + size));
      at += size;
    }
  } catch (error) {
    console.warn(`sounds: ${error?.message ?? error}`);
  } finally {
    game.assets_provided();
  }
}

// ---- the pilot's own ship graphics ----------------------------------------------------------
// The page's Graphics overlay (Esc then G, or F8, in the game) uses these to show and replace
// ships 1-8 for this player. A sheet is 40 square frames in 10 columns and 4 rows (360x144 for
// stock ships): frame 0 at the top left is nose-up, then clockwise in 9 degree steps.
function shareShips() {
  globalThis.dareShips = {
    set: (ship, sheetBytes) => game.set_custom_ship(ship, sheetBytes), // PNG or BMP bytes
    clear: (ship) => game.clear_custom_ship(ship),
    sheet: (ship) => game.ship_sheet(ship), // { width, height, rgba } or null before the game draws
  };
  // Every other graphic the arena could replace (bombs, bullets, prizes...), and whether the
  // pilot's own are shown or the arena's.
  globalThis.dareGraphics = {
    set: (name, sheetBytes) => game.set_custom_graphic(name, sheetBytes), // PNG or BMP bytes
    clear: (name) => game.clear_custom_graphic(name),
    useOwn: (on) => game.use_own_graphics(!!on),
  };
  globalThis.dispatchEvent(new Event("dare-ships-ready"));
  // The zone's tutorial: the page's mentor box sends its buttons' commands (?skip, ?leave,
  // ?tutorial, ?go <arena>; nothing else is taken) and asks how the player's keys read
  // ("gun" -> "Ctrl"). The game hands it the hint lines through globalThis.foundationTutorial.
  globalThis.dareTutorial = {
    command: (line) => game.tutorial_command(line),
    keyFor: (action) => game.key_for(action),
  };
}

// A guest who has been through the tutorial says so when logging in, so the zone doesn't start
// it again (the page sets the key when a tutorial ends).
const TUTORIAL_DONE_KEY = "dare.tutorial.done";
function guestPassword() {
  try { if (localStorage.getItem(TUTORIAL_DONE_KEY)) return "guest:done"; } catch {}
  return "guest";
}

// ---- Enter --------------------------------------------------------------------------------
let loading = null; // the game's download and start-up, begun once the page has loaded
let sounds = null;

function loadGame() {
  loading ??= (async () => {
    progress(0);
    const [glue, transport, module, files] = await Promise.all([
      import(here("pkg/oss_client.js")),
      import(here("transport.mjs")).then((t) => { connectZone = t.connectZone; return t; }),
      download(),
      cachedFiles(),
    ]);
    await glue.default({ module_or_path: module });
    game = glue;
    game.use_page_cache();
    for (const [name, data] of files) game.preload_cached_file(name, data);
    shareShips();
    progress(100);
  })();
  loading.catch(() => { loading = null; button.classList.remove("downloading"); label.textContent = "Enter"; });
  return loading;
}

// After Enter: the download so far shows on the button, and the zone link opens meanwhile.
function showLoading() {
  showProgress = true;
  if (!game) progress(percent);
  const transportReady = import(here("transport.mjs")).then((t) => { connectZone = t.connectZone; });
  void transportReady.then(warmUpGateway, () => {});
  return loadGame();
}

async function gamePass() {
  const response = await fetch("/account/api/gamepass", { method: "POST", credentials: "same-origin", headers: HEADERS, body: "{}" });
  if (response.status === 401) return null;
  const body = await response.json().catch(() => ({}));
  if (!response.ok || !body.ok) throw new Error(body.message || "Couldn't get you into the zone. Try again.");
  return body;
}

async function play() {
  button.disabled = true;
  say("");
  try {
    const loaded = showLoading();
    loaded.catch(() => {});
    if (!me) await askToSignIn();
    if (!me && flyAsGuest) {
      await loaded;
      enter("guest", guestPassword(), picker.value, "");
      return;
    }
    if (!me) { // cancelled: the download carries on unseen
      showProgress = false;
      button.classList.remove("downloading");
      label.textContent = "Enter";
      return;
    }
    await loaded;
    await globalThis.foundationChatLeave?.(); // the same name can't be in the zone twice
    let pass = await gamePass();
    if (!pass) { // the site session ended: sign in again
      me = null;
      showPilot();
      await askToSignIn();
      if (!me) return;
      pass = await gamePass();
      if (!pass) throw new Error("Couldn't sign you in. Try again.");
    }
    enter(pass.name, pass.pass, picker.value, pass.settings ?? "");
  } catch (error) {
    say(error?.message ?? String(error), true);
  } finally {
    if (!entry.hidden) button.disabled = false;
  }
}

// The game's own settings (zoom, HUD, radar, sound...), kept per pilot in this browser: the
// Options window's Save hands the whole file over, and the next Enter gives it back.
const SETTINGS_KEY = "dare.settings.";
function localSettings(name) {
  try { return localStorage.getItem(SETTINGS_KEY + name.toLowerCase()) ?? ""; } catch { return ""; }
}
function keepSettings(name) {
  globalThis.dareSaveSettings = (text) => {
    try { localStorage.setItem(SETTINGS_KEY + name.toLowerCase(), String(text)); return true; } catch { return false; }
  };
}

function enter(name, pass, arena, settings) {
  try {
    entry.hidden = true;
    document.body.classList.add("playing");
    const observer = new MutationObserver(() => {
      const canvas = document.querySelector("body > canvas"); // the game's (the page has others)
      if (canvas) {
        canvas.tabIndex = 0;
        canvas.focus();
        observer.disconnect();
      }
    });
    observer.observe(document.body, { childList: true, subtree: true });
    startReport(name, arena);
    // The sounds once the pilot is in the arena, so they don't slow the map's download.
    const stage = globalThis.dareLoadingStage;
    globalThis.dareLoadingStage = (step, ...rest) => {
      if (step === "playing") sounds ??= loadSounds();
      return stage?.(step, ...rest);
    };
    setTimeout(() => { sounds ??= loadSounds(); }, 30000); // (a game that never says)
    keepSettings(name);
    game.start_dare_in(name, pass, arena, settings, localSettings(name));
  } catch (error) {
    entry.hidden = false;
    document.body.classList.remove("playing");
    say(error?.message ?? String(error), true);
  }
}

// ---- the game's log, for the zone's staff ------------------------------------------------------
// While the pilot is in the game, what the game writes to the console (connecting, its "perf:" and
// "tracking:" figures every ten seconds, errors) goes to the site every 30 s and when the page
// closes, so a report of lag can be looked into without asking the pilot for anything. The game
// writes no chat, keys or passwords there. At most 400 lines per send; the rest are counted.
const REPORT_URL = "/clientlog";
const report = { id: "", pilot: "", arena: "", seq: 0, started: 0, lines: [], dropped: 0 };

function note(level, args) {
  if (!report.started) return;
  if (report.lines.length >= 400) { report.dropped++; return; }
  const text = args.map((a) => (typeof a === "string" ? a : a?.message ?? String(a))).join(" ");
  report.lines.push([Math.round(performance.now() - report.started), level, text.slice(0, 2000)]);
}
for (const level of ["info", "warn", "error"]) {
  const original = console[level].bind(console);
  console[level] = (...args) => { note(level, args); original(...args); };
}
addEventListener("error", (e) => note("error", [`page: ${e.message}`]));
addEventListener("unhandledrejection", (e) => note("error", [`page: ${e.reason?.message ?? e.reason}`]));

function sendReport(closing) {
  if (!report.started || (!report.lines.length && !closing)) return;
  const net = navigator.connection;
  const body = JSON.stringify({
    id: report.id, seq: report.seq++, version, pilot: report.pilot, arena: report.arena,
    seconds: Math.round((performance.now() - report.started) / 1000), closing,
    ua: navigator.userAgent, platform: navigator.userAgentData?.platform ?? navigator.platform,
    screen: `${screen.width}x${screen.height}@${devicePixelRatio}`, cores: navigator.hardwareConcurrency,
    net: net ? { type: net.effectiveType, rtt: net.rtt, downlink: net.downlink } : null,
    lines: report.lines, dropped: report.dropped,
  });
  report.lines = [];
  report.dropped = 0;
  const blob = new Blob([body], { type: "application/json" });
  if (closing && navigator.sendBeacon?.(REPORT_URL, blob)) return;
  fetch(REPORT_URL, { method: "POST", body: blob, keepalive: true }).catch(() => {});
}

function startReport(pilot, arena) {
  report.id = crypto.randomUUID?.() ?? `${Date.now()}-${Math.random().toString(36).slice(2)}`;
  report.pilot = pilot;
  report.arena = arena;
  report.started = performance.now();
  note("info", [`page: entering ${arena || "public"} as ${pilot}`]);
  setInterval(() => sendReport(false), 30000);
}
addEventListener("pagehide", () => sendReport(true));

// ---- start ----------------------------------------------------------------------------------
try {
  const left = sessionStorage.getItem(LEFT_KEY);
  if (left !== null) {
    sessionStorage.removeItem(LEFT_KEY);
    if (left) say(left, true);
  }
} catch {}
void fetchArenas();
setInterval(() => { if (!entry.hidden) void fetchArenas(); }, 30000);
void whoAmI().then(fetchDrafts);

const missing = await missingFeature();
if (missing) {
  say(missing, true);
} else {
  button.disabled = false;
  button.addEventListener("click", play);
  // Get the game ready while the pilot looks around (a failure here is retried on Enter).
  const prepare = () => { loadGame().catch(() => {}); };
  if (document.readyState === "complete") setTimeout(prepare, 0);
  else addEventListener("load", () => setTimeout(prepare, 0), { once: true });
}
