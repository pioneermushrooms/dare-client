// The Foundation home page's game: downloads the client in the background, and Play signs
// the pilot in (only if the site doesn't know them yet), gets a one-time game pass and
// starts straight in the chosen arena. The page markup is the site's (/); this module is
// served from /play/ and finds its parts by id.
// The page loads this module as start.mjs?v=<version>; the same version goes on every file
// it fetches, so a browser holding an older copy of any of them can't mix the two.
const version = new URL(import.meta.url).searchParams.get("v") ?? "";
const here = (path) => new URL(version ? `${path}?v=${encodeURIComponent(version)}` : path, import.meta.url);
// The game (its script, its 13 MB WebAssembly and the zone link) is fetched only when the
// pilot presses Enter: opening the page downloads nothing of it.
let game = null; // { init, start_dare_in } once its script is loaded
let connectZone = null;

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
const pilotButton = $("pilot");
const pilotMenu = $("pilotmenu");
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
// A guest: the zone gives the name "guest" a fresh random GuestNNNN name, no password needed.
$("signin-guest")?.addEventListener("click", () => {
  flyAsGuest = true;
  dialog.close();
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
// While the game downloads (after Enter), the button says how far along it is.
function progress(value) {
  percent = value;
  button.style.setProperty("--p", `${value}%`);
  button.classList.toggle("downloading", value < 100);
  label.textContent = value < 100 ? `Loading ${value}%` : "Enter";
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
  globalThis.dispatchEvent(new Event("dare-ships-ready"));
}

// ---- Enter --------------------------------------------------------------------------------
let loading = null; // the game's download and start-up, begun by the first Enter

function loadGame() {
  loading ??= (async () => {
    progress(0);
    const [glue, transport] = await Promise.all([import(here("pkg/oss_client.js")), import(here("transport.mjs"))]);
    connectZone = transport.connectZone;
    const bytes = await download();
    await glue.default({ module_or_path: bytes });
    game = glue;
    shareShips();
    progress(100);
  })();
  loading.catch(() => { loading = null; button.classList.remove("downloading"); label.textContent = "Enter"; });
  return loading;
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
    if (!me) await askToSignIn();
    if (!me && flyAsGuest) {
      await loadGame();
      enter("guest", "guest", picker.value, "");
      return;
    }
    if (!me) return; // cancelled
    await loadGame();
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
    game.start_dare_in(name, pass, arena, settings);
  } catch (error) {
    entry.hidden = false;
    document.body.classList.remove("playing");
    say(error?.message ?? String(error), true);
  }
}

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
}
