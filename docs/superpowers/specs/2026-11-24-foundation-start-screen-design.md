# Foundation start screen (browser client) — design

Date: 2026-11-24 · Branch: `play-polish` (dare-client) · Status: draft for approval

## Intent

Players reach the game from subspacefoundation.com. By the time the client's start screen
appears they have already chosen Foundation on `/play/`, so a zone list there is
redundant, and the screen still says "DARE", "OPEN SUBSPACE CLIENT" and
"DARE ZONE – OSS-CLIENT". Rebrand it as SubSpace Foundation and use the space the zone list
took to showcase Foundation's own arenas: a live list you can join from directly, with a
clear way to create your own. This is roadmap item 5 (splash screen), browser first.

## Goals

1. **Foundation look.** A new bundled skin, "Foundation", drawn by `skins/src/make_skin.py`
   in the site's colours (navy `#070c15`, panel `#0d1624`, cyan `#71e4fa`) with the
   "◈ SUBSPACE FOUNDATION" brand. No "Dare", "Open SubSpace client" or "oss-client" text
   on it. It is the default skin in the browser build. The Dare skins stay available as
   alternates.
2. **Foundation arenas instead of zones (browser).** The right-hand panel (the skin's
   `zonetext` list) lists Foundation arenas live: the public arena first, then
   player-owned arenas that are active. Each row: arena name and pilot count.
   - Capped at 12 rows shown, scrollable with the existing up/down hot spots.
   - Sort toggle: by pilots (default) or by name. It reuses the skin's `ping` hot spot
     (today "ping number ↔ bar"), labelled `SORT` in the Foundation skin.
   - Refreshes every 30 s while the screen is open. The skin's `zones` button
     (today `REFRESH`) refreshes it now.
   - Selecting a row sets the login form's arena. Double-click (or Enter) plays straight
     into that arena. The profile box's arena line shows the choice.
3. **"+ CREATE".** The skin's `trace` hot spot (today "coming soon") becomes `+ CREATE`.
   Clicking it shows a confirmation in the info box ("Open the arena application in a new
   tab? Enter to confirm, Esc to cancel"). On confirm it opens
   `https://subspacefoundation.com/arena/` in a new browser tab. The game keeps running.
4. **Profile panel tidied.** In the profile box and dialog, the browser build shows Name and
   Arena clearly. Two links, "Register" (`/account/signup`) and "Forgot password"
   (`/account/forgot`), open the site in a new tab. The zone address field is hidden in the
   browser, where the zone is fixed.
5. **Home page "Play now".** On subspacefoundation.com, "Enter the zone" becomes
   "Play now" and links to `/play/` (dare-zone `site/index.html`).

## Non-goals

- The desktop Dare app's zone list (it keeps today's zone list; it gets the Foundation skin
  as an option). The desktop arena list is a later step.
- Instant draft arenas (build before applying). That's a later, larger feature. "+ CREATE"
  opens today's application.
- Other zones in the browser (parked: it would route other zones' traffic through
  Foundation's IP).
- Multiple profiles (separate project).

## Anti-goals

- A start screen that waits on the network. If the arena list hasn't arrived or fails,
  the panel shows "public" plus "Arena list unavailable", and Play still works.
- Opening pages without a click: new tabs only open from a click or keypress the player made.
- Shipping art we may not give away. The skin is drawn by our generator.

## Constraints

- The skin stays a valid Continuum `.skn` (640×420), so the hot spot names are the
  standard ones. Only the labels drawn into the art change.
- The browser build's existing bridge pattern: JavaScript globals that Rust calls through
  `wasm_bindgen` (see `crates/oss-client/src/web.rs`, `openDareGateway`).
- Arena data: `GET /arena/api/public` (same origin) returns `arenas[]` with `name`,
  `status`, `players` (only active arenas have a count). The public arena isn't in it.
- oss-client repo rules: fmt, clippy `-D warnings`, tests, `cargo doc -D warnings` before
  commits. Untrusted data (the feed) never panics.

## Approaches considered

- **A. Browser now, desktop later (chosen by Chris).** The web page fetches the arena list
  and hands it to the game through a JS bridge, like the gateway. Smallest change, no new
  network code in the shared client. Desktop keeps the zone list for now.
- **B. The game fetches the list itself in both builds.** Browser fetch in the browser
  build, plus a small HTTP library in the desktop build. Both builds match, but it adds a
  dependency and desktop testing now.
- **C. Ask the zone after sign-in.** The protocol can list arenas only after you're
  connected, which breaks joining directly from the start screen. Dropped.

## Design

### Bridge (web)

- `start.mjs` defines `globalThis.foundationArenas()`, which returns the latest list as a
  JSON string (or `""` before the first answer). It fetches `/arena/api/public` on load
  and every 30 s, plus on demand via `globalThis.refreshFoundationArenas()`.
- `globalThis.openFoundationPage(path)` opens `https://subspacefoundation.com` + `path` in
  a new tab. Only fixed paths are passed from Rust (`/arena/`, `/account/signup`,
  `/account/forgot`).
- The Rust side (`web.rs`) parses the JSON into `Vec<ArenaRow { name, players }>`,
  dropping non-active arenas and bad entries. It always prepends `public`.

### Start screen (`skin_screen.rs`, `zones.rs`, `app.rs`)

- In the browser build, the `zonetext` list is driven by an arena list model instead of
  `ZoneBrowser`. Same row drawing: the name where the zone name goes, the pilot count
  where the player count goes. The ping column is blank.
- Sort state is remembered for the session only.
- Choosing an arena writes `Login.arena`. `public` maps to an empty arena (the server's
  default public arena).
- The info box shows help for the new hot spots and the "+ CREATE" confirmation.

### Skin (`skins/src/make_skin.py`)

- New theme "Foundation". The title bar draws the ◈ mark and "SUBSPACE FOUNDATION". The
  panel titles are PILOT, SHIP and FOUNDATION ARENAS. The buttons are REFRESH (`zones`),
  + CREATE (`trace`) and SORT (`ping`). There is no footer text, or "subspacefoundation.com".
- Output `assets/skins/Foundation.skn`. The default skin is `Foundation` when built for
  wasm32, `Dare` otherwise.

### Arena pilot count for "public"

- The public arena's count isn't in `/arena/api/public`. Add `publicPlayers` (players in
  the server's public arenas) to that reply in the Dare.Arenas plugin, then deploy it
  with the next plugin deploy (zone restart, no players online). Until then the public
  row shows no count.

## Testing

- Rust unit tests: parse the feed (good, empty, malformed, huge), sort orders, cap and
  scroll, `public` → empty arena.
- Generator: build Foundation.skn and check it loads with the existing skin parser tests.
- In the browser: on `/play-next/`, the list shows, sorts and refreshes. Double-click
  joins that arena (Chris signs in). "+ CREATE" confirms, then opens the tab. The
  unavailable state shows when the feed is blocked. Resize still fits.
- Then promote to `/play/`, as before.

## Left to the builder

The skin's exact art and spacing, info box wording, the row cap if 12 doesn't fit the
skin's list height, and how sort is shown (label or arrow).
