# Foundation Start Screen Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rebrand the browser client's start screen as SubSpace Foundation and replace its zone list with a live, joinable list of Foundation arenas plus a "+ CREATE" button.

**Architecture:** A new generated skin supplies the look. In the browser build, `start.mjs` fetches `/arena/api/public` and exposes the rows to Rust as tab-separated text through a JS bridge, like `openDareGateway`. The start screen's existing `zonetext` list draws those rows: selecting one sets `Login.arena` instead of `Login.server`. Desktop behaviour is unchanged.

**Tech Stack:** Rust (oss-client, wgpu, winit, wasm-bindgen), Python 3 skin generator, plain JS module, C# Dare.Arenas plugin, static HTML.

**Spec:** `docs/superpowers/specs/2026-11-24-foundation-start-screen-design.md`

Repos: dare-client worktree `~/oss-src/play-polish` (branch `play-polish`, oss-core sibling at `eae56a0`), and dare-zone worktree `~/dare-zone/.claude/worktrees/zone-directory` (branch `zone-directory`). Rust toolchain: `source ~/.cargo/env`.

## Global Constraints

- Skin stays a valid Continuum `.skn`, 640×420 main region, standard hot spot names. Only the drawn labels change.
- Foundation colours: navy `#070c15`, panel `#0d1624`, cyan `#71e4fa`. Brand text "SUBSPACE FOUNDATION" with the ◈ mark. No "Dare", "Open SubSpace client" or "oss-client" text in the Foundation skin.
- Default skin: `Foundation` on `wasm32`, `Dare` elsewhere. The Dare skins stay selectable.
- Arena list: `public` always first. Cap 12 visible rows (scroll for more). Sort by pilots (default) or by name. Refresh every 30 s and on REFRESH.
- `public` maps to an empty `Login.arena` (the server's default public arena).
- New tabs open only from a player's click or key: fixed paths `/arena/`, `/account/signup`, `/account/forgot` on `https://subspacefoundation.com`.
- The feed is untrusted: never panic on it. A missing or failed feed shows `public` plus "Arena list unavailable", and Play still works.
- oss-client rules before every commit: `cargo fmt --all`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, `RUSTDOCFLAGS=-Dwarnings cargo doc --no-deps -p oss-client`, plus `cargo check -p oss-client --lib --target wasm32-unknown-unknown`.

## Review Focus

- Arena names from the feed that are long, empty, or have tabs or newlines: rows truncate and odd lines are skipped (Task 2 test `parse_rows_skips_bad_lines`).
- The list changes while a row is selected (arenas appear or vanish on refresh): the selection follows the arena name, not the row index (Task 4 test `selection_follows_arena_name`).
- Enter while the "+ CREATE" confirmation is showing confirms, and doesn't also Play (Task 3 test `confirm_takes_enter_before_play`).
- An arena with more than 12 rows' worth of arenas: scroll arrows and wheel reach the last one (covered by the existing `clamp_scroll` tests, since the list reuses them).
- The feed never answers: after load the panel shows `public` plus the unavailable note, not an empty well (Task 2 test `unavailable_still_lists_public`).

---

### Task 1: Foundation skin

**Files:**
- Modify: `skins/src/make_skin.py` (add theme "Foundation"; per-theme brand text, panel title, button labels, footer)
- Create: `assets/skins/Foundation.skn` (generated)
- Modify: `crates/oss-client/src/game/skin.rs:362` (`DEFAULT_SKIN`)
- Modify: `crates/oss-client/src/app.rs:455-470` (`read_skin` include) and `:610-616` (wasm entry list)
- Test: existing skin tests in `crates/oss-client/src/game/skin.rs` (the one asserting `(640, 420)` around line 815)

**Interfaces:**
- Produces: skin name `"Foundation"`. `pub const DEFAULT_SKIN: &str` is `"Foundation"` under `cfg(target_arch = "wasm32")`, `"Dare"` otherwise.

- [ ] **Step 1:** Add `"Foundation"` to the name list of the bundled-skin test near `skin.rs:815`, so it parses `assets/skins/Foundation.skn` and asserts main `(640, 420)` and that the hot spots `zonetext`, `zones`, `trace`, `ping`, `play`, `quit` and `proftext` exist.
- [ ] **Step 2:** Run `cargo test -p oss-client skin`. Expected: FAIL (file missing).
- [ ] **Step 3:** In `make_skin.py`, add a theme dict `"Foundation"` with the colours above and keys `brand` (`"SUBSPACE FOUNDATION"`), `tagline` (`"FOUNDATION ZONE"`), `list_title` (`"FOUNDATION ARENAS"`), `labels` (`{"zones": "REFRESH", "trace": "+ CREATE", "ping": "SORT"}`) and `footer` (`"SUBSPACEFOUNDATION.COM"`). Make `draw_main`/`build` read the title, labels and footer from the theme, with the Dare themes' current values as their entries, so the Dare skins come out byte-identical. In place of the `logo(…, "DARE")` call, the Foundation title is a ◈ mark (a cyan pixel diamond) plus the brand text in the existing pixel font at a scale that fits 640 px.
- [ ] **Step 4:** Generate with `python3 -I skins/src/make_skin.py "Foundation" "assets/skins/Foundation.skn" --preview /tmp/foundation.png` and look at the preview. Regenerate both Dare skins to a temp folder and `cmp` them against `assets/skins/` (must be identical).
- [ ] **Step 5:** Add `"Foundation"` to `read_skin`'s `include_bytes!` match and to the wasm entry list (first). Make `DEFAULT_SKIN` cfg-dependent as in Interfaces.
- [ ] **Step 6:** Run the skin tests, plus the wasm check from Global Constraints. Expected: PASS.
- [ ] **Step 7:** Commit `skin: Foundation start-screen skin, default in the browser`.

### Task 2: Arena list model

**Files:**
- Create: `crates/oss-client/src/game/arenas.rs`
- Modify: `crates/oss-client/src/game/mod.rs` (add `pub mod arenas;`)

**Interfaces:**
- Produces:
  - `pub struct ArenaRow { pub name: String, pub players: Option<u32> }`
  - `pub enum ArenaSort { Pilots, Name }`, plus `fn next(self) -> Self`
  - `pub fn parse_rows(text: &str) -> Vec<ArenaRow>`: one arena per line, `name\tplayers`. `players` is a decimal number or empty. Lines without a valid name are skipped. Names are trimmed and cut to 16 chars. At most 200 rows.
  - `pub struct ArenaList { pub rows: Vec<ArenaRow>, pub sort: ArenaSort, pub unavailable: bool }`, with:
    - `pub fn set_feed(&mut self, text: Option<&str>)`: `None` means the feed failed, so `unavailable = true`. Rows are kept from the last good feed.
    - `pub fn shown(&self) -> Vec<ArenaRow>`: `public` first (players taken from a `public` line if the feed has one), then the rest sorted (pilots descending then name, or name case-insensitive), with no duplicate `public`.
    - `pub fn as_zones(&self) -> Vec<oss_session::directory::ZoneInfo>`: maps `shown()` to `ZoneInfo { name, description: "", address: name, players: players.unwrap_or(0), ping_ms: None }`.
  - `pub fn arena_for_login(name: &str) -> String`: `"public"` becomes `""`, other names unchanged.

- [ ] **Step 1:** Write tests in `arenas.rs`:
  - `parse_rows_reads_names_and_counts`: `"duel\t3\nskyrace\t\n"` gives `[duel Some(3), skyrace None]`.
  - `parse_rows_skips_bad_lines`: blank lines, `"\t5"`, `"a\tb\tc"` and a 40-char name. Bad lines are dropped, and the long name is cut to 16.
  - `shown_puts_public_first_and_sorts`: Pilots sort gives `public, big(9), small(1), zero(None)`. Name sort gives `public, big, small, zero`.
  - `unavailable_still_lists_public`: `set_feed(None)` on a new list gives `shown() == [public]` and `unavailable == true`.
  - `arena_for_login_maps_public_to_empty`.
- [ ] **Step 2:** Run `cargo test -p oss-client arenas`. Expected: FAIL (not defined).
- [ ] **Step 3:** Implement the Interfaces.
- [ ] **Step 4:** Run the tests. Expected: PASS. Then the full check list.
- [ ] **Step 5:** Commit `start screen: Foundation arena list model`.

### Task 3: Skin actions for CREATE, SORT and the confirmation

**Files:**
- Modify: `crates/oss-client/src/game/skin_screen.rs` (`SkinAction`, `click`, `help`, tests)

**Interfaces:**
- Produces:
  - New `SkinAction::CreateArena`.
  - `pub fn confirm_key(strokes: &[Stroke]) -> Option<bool>`: `Some(true)` on Enter/NumpadEnter, `Some(false)` on Escape, otherwise `None`.
  - `pub const CREATE_CONFIRM: &str = "Open the arena application in a new tab? Enter to confirm, Esc to cancel."`
- Under `cfg(target_arch = "wasm32")`:
  - `click(Trace)` gives `CreateArena`.
  - `help(Trace)` is `"+ Create: apply for your own Foundation arena."`
  - `help(Ping)` is `"Sort arenas by pilots or by name."`
  - `help(Zones)` is `"Refresh the arena list. (F3, Cmd-R)"`
  - `help(ZoneText)` is `"Click an arena to pick it; double-click to play there. (Up/Down)"`
  - Desktop keeps today's results. `TogglePing` stays the action for `Ping`, and the app gives it the sort meaning in arena mode (Task 4).

- [ ] **Step 1:** Write the tests `confirm_key_reads_enter_and_escape` and `confirm_takes_enter_before_play`. The second asserts `confirm_key` returns `Some(true)` for the same Enter stroke that `key()` maps to `Play`, so the caller can check it first. Under wasm the existing `click(&HotSpot::Trace…)` test asserts `CreateArena`. Gate it with `cfg` so native tests still assert the Info text.
- [ ] **Step 2:** Run `cargo test -p oss-client skin_screen`. Expected: FAIL.
- [ ] **Step 3:** Implement the Interfaces.
- [ ] **Step 4:** Run the tests and the wasm check. Expected: PASS.
- [ ] **Step 5:** Commit `start screen: + CREATE action and confirmation keys`.

### Task 4: Browser bridge and arena mode on the start screen

**Files:**
- Modify: `web/client/start.mjs` (arena feed and page opener)
- Modify: `crates/oss-client/src/web.rs` (bridge bindings)
- Modify: `crates/oss-client/src/app.rs` (start-screen state ~line 280-300, `skin_frame` ~320-455, the `RefreshZones` handling ~1476, and the per-frame pump ~1331)
- Test: unit tests for the selection helper in `skin_screen.rs`

**Interfaces:**
- JS (in `start.mjs`):
  - `globalThis.foundationArenas()` returns the latest rows as `name\tplayers\n…` text, `""` before the first answer, or `null` after a failed fetch.
  - It fetches `/arena/api/public` on load, every 30 s and when `globalThis.refreshFoundationArenas()` is called. It keeps `arenas[]` with `status === "Active"` and emits `name\tplayers`. When the reply has a numeric `publicPlayers`, it also emits `public\t<n>`.
  - `globalThis.openFoundationPage(path)` calls `window.open("https://subspacefoundation.com" + path, "_blank", "noopener")` only when `path` is one of `/arena/`, `/account/signup` or `/account/forgot`.
- Rust (`web.rs`, `pub(crate)`):
  - `fn foundation_arenas() -> Option<Option<String>>`: the outer `None` means not answered yet. `Some(None)` means it failed.
  - `fn refresh_foundation_arenas()`
  - `fn open_foundation_page(path: FoundationPage)` with `pub(crate) enum FoundationPage { Apply, Register, Forgot }`, which maps to the three paths.
- `skin_screen.rs`: `pub fn keep_arena_selection(arena: &str, zones: &[ZoneInfo]) -> Option<usize>`. It finds the row whose `address` equals `arena`, or `"public"` when `arena` is empty.
- In the app, the start-screen UI gains `arenas: crate::game::arenas::ArenaList`, `confirm_create: bool` and `last_feed: Option<String>`, all wasm only.

- [ ] **Step 1:** Write the test `selection_follows_arena_name`: rows `[public, duel, race]` with arena `"race"` give `Some(2)`. Arena `""` gives `Some(0)`. Arena `"gone"` gives `None`.
- [ ] **Step 2:** Run it. Expected: FAIL. Implement `keep_arena_selection`, then run again. Expected: PASS.
- [ ] **Step 3:** Add the JS bridge in `start.mjs` and the bindings in `web.rs` (as in Interfaces).
- [ ] **Step 4:** Wire arena mode in `app.rs`, all behind `cfg(target_arch = "wasm32")`:
  - Each start-screen frame, read `foundation_arenas()`. When the text differs from `last_feed`, call `arenas.set_feed(…)` and set `browser.zones = arenas.as_zones()`. Don't call `browser.pump()`.
  - Make the selection use `keep_arena_selection(&login.arena, …)`. The `select` closure sets `login.arena = arena_for_login(&z.name)` and leaves `login.server` alone.
  - `PlayZone` selects, then plays, as today.
  - `TogglePing` sets `arenas.sort = arenas.sort.next()` and rebuilds `browser.zones`.
  - `RefreshZones` calls `refresh_foundation_arenas()` instead of the directory refresh.
  - `CreateArena` sets `confirm_create = true`. While it's true, check `confirm_key(strokes)` before the other actions: `Some(true)` calls `open_foundation_page(FoundationPage::Apply)`, and either answer clears the flag and eats the stroke. While it's true, the info box shows `CREATE_CONFIRM`.
  - While `arenas.unavailable` is true, the info box status shows "Arena list unavailable" when nothing is hovered.
  - Cap: when `skin_view::zone_rows(layout)` is more than 12, use 12 as the page size.
- [ ] **Step 5:** Run the full check list. Build with `web/build-client.sh` to the scratchpad and serve it locally. With no feed (local), the list shows `public` and the unavailable note, and "+ CREATE" shows the confirmation. Enter opens the tab, and Esc cancels.
- [ ] **Step 6:** Commit `start screen: live Foundation arenas and + CREATE in the browser`.

### Task 5: Profile dialog in the browser

**Files:**
- Modify: `crates/oss-client/src/game/profile_dialog.rs` (fields shown, link rows)
- Modify: `crates/oss-client/src/app.rs` (dialog clicks ~line 350-370)

**Interfaces:**
- Consumes: `open_foundation_page(FoundationPage::Register | FoundationPage::Forgot)` (Task 4).
- Produces: under wasm, the dialog leaves out `Field::Server`. It shows two extra clickable lines under the fields, "Register a pilot name" and "Forgot password", recorded in `ui.dialog_spots` as a new `DialogSpot::Link(FoundationPage)` alongside the existing field spots. Tab order skips the zone field.

- [ ] **Step 1:** Write the test `browser_dialog_has_no_zone_field`: under wasm, the dialog's field list doesn't contain `Field::Server`. Natively it still does.
- [ ] **Step 2:** Run it. Expected: FAIL. Implement, then run again. Expected: PASS.
- [ ] **Step 3:** Wire the link clicks in `app.rs` to `open_foundation_page`. Run the full check list.
- [ ] **Step 4:** Commit `profile: Foundation links and no zone field in the browser`.

### Task 6: Server count for "public", and "Play now" on the home page (dare-zone)

**Files:**
- Modify: `src/Dare.Arenas/Web/WebServer.cs` (`PublicAsync` reply, ~line 90-110)
- Modify: `site/index.html` ("Enter the zone" → "Play now")
- Test: `tests/Dare.Arenas.Tests` (whichever test file covers the public reply; add one if none)

**Interfaces:**
- Produces: `/arena/api/public` gains `publicPlayers`, a number: the players in arenas that aren't owned arenas and whose names aren't private (starting with `#`). That's the server's public arenas `0`, `1` and so on.

- [ ] **Step 1:** Write a test for the counting helper. Players in arenas `0`, `0`, `1`, `duel` (owned) and `#secret` give `publicPlayers == 3`.
- [ ] **Step 2:** Run `dotnet test`. Expected: FAIL. Implement the helper and add the field to the reply. Run again. Expected: PASS (all tests).
- [ ] **Step 3:** In `site/index.html`, change the portal action's `<strong>Enter the zone</strong>` to `<strong>Play now</strong>` and the `aria-label` to "Play SubSpace Foundation now in your browser". The link stays `/play/`.
- [ ] **Step 4:** Commit `public arena count; home page Play now`.

### Task 7: Stage, check and go live

- [ ] **Step 1:** Build with `web/build-client.sh <scratch>/play-new`. Stage it on the server as `/var/www/foundation/play-next/` (tar with `COPYFILE_DISABLE=1`, as before).
- [ ] **Step 2:** Ask Chris before the plugin deploy, which restarts the zone. Then run `deploy/deploy.sh` from the zone-directory worktree, which also publishes `site/index.html`. Check that `/arena/api/public` has `publicPlayers`, and that the home page says "Play now".
- [ ] **Step 3:** In the browser pane, on `/play-next/`, check:
  - The start screen shows the Foundation skin and the arena list with `public` first.
  - SORT switches the order, and REFRESH updates.
  - "+ CREATE" confirms, then opens `/arena/`.
  - The profile dialog has no zone field, and both links work.
  - Chris signs in, and a double-click on an arena joins it.
- [ ] **Step 4:** With Chris's OK, move `/play/` to a backup under `/root/` and `play-next` to `play`. Push `play-polish` and `zone-directory`.
