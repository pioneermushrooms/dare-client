# Skinned Start Screen Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace oss-client's code-drawn start screen with a Continuum-0.40-compatible skinned screen that fills the window, ships two original skins, and lets the player switch skins.

**Architecture:** A pure parser (`game/skin.rs`) turns a `.skn` (BMP + appended TXT) into a `Skin` (RGBA image + layout). Pure layout/hit-test math lives in `game/skin_view.rs`, which also draws the skin through the existing `Renderer::quad`. `game/skin_screen.rs` maps hot spots and keys to actions that reuse today's `Choice` handling in `app.rs`. The name/password/arena/zone fields move into a modal profile dialog driven by the existing `Login` model.

**Tech Stack:** Rust 2024, winit, wgpu via `crate::gfx::Renderer`, `oss_assets::image::decode_image`; Python 3 stdlib for the skin generator.

**Spec:** `docs/superpowers/specs/2026-11-22-skinned-start-screen-design.md`

## Global Constraints

- Branch `dare` in `~/oss-src/oss-client`; follow `CLAUDE.md`: before each commit run `cargo fmt --check` (never `--all`), `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`.
- Untrusted skin data never panics; a bad skin is skipped with a logged warning.
- Never commit Continuum files or the three third-party skins (Ceinwyn, Pipe Dreams, VIEd); tests use synthetic skin text and generated images only.
- Skin TXT syntax and hot-spot semantics follow Continuum's Skins Development Guide v1.2 exactly as quoted in the spec; rectangles are inclusive; tick = 200 ms; `proftext` pitch 16 px, max 23 chars; `zonetext` pitch 14 px; text 8×12.
- Scale = largest integer `s ≥ 1` with `main.w*s ≤ 0.92·win.w` and `main.h*s ≤ 0.92·win.h`; skin centred.
- Default skin name `Dare`; second shipped skin `Dare Night`. Chosen skin persists as `[video] skin = <name>` in `oss-client.ini` (deviation from spec's `login.ini`, so Options can edit it).
- Skin search order: `<exe-dir or checkout>/assets/skins`, `<settings dir>/skins`, `<continuum dir>/skins`, then each `[skins] folders` entry; on a name clash (case-insensitive) the later folder wins.
- Max `.skn` file size read: 32 MiB.

## Review Focus

- A real-world skin with lines the guide does not cover (stray text, `rect` with x2<x1, trailing junk): expect the line skipped with a warning, rest of skin works. → Task 1 test `skips_bad_lines_keeps_rest`.
- Overlay point whose rectangle runs past the bitmap edge (common in hand-made skins): expect that overlay dropped, hot spot still clickable. → Task 1 test `drops_out_of_bounds_overlays`.
- Window smaller than the skin (laptop, resized window): expect scale 1, skin centred and clipped, clicks still map. → Task 4 test `scale_never_below_one_and_maps_when_clipped`.
- Zone list longer than `zonetext` holds and list refreshed while scrolled: expect scroll clamped and selection kept on the same zone address. → Task 5 test `scroll_clamps_after_refresh`.
- Skin chosen in settings no longer present (file deleted): expect fallback to `Dare`, no crash. → Task 6 test `missing_skin_falls_back_to_dare`.

---

### Task 1: Skin file parser

**Files:**
- Create: `crates/oss-client/src/game/skin.rs`
- Modify: `crates/oss-client/src/game/mod.rs` (add `pub mod skin;`)

**Interfaces:**
- Produces:
  - `pub const TICK_MS: u64 = 200;`
  - `#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub struct SkinRect { pub x: u32, pub y: u32, pub w: u32, pub h: u32 }` with `pub fn holds(&self, x: u32, y: u32) -> bool`
  - `#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum Frame { At(u32, u32), Blank }`
  - `#[derive(Clone, Debug, PartialEq, Eq)] pub struct Step { pub ticks: u32, pub frame: Frame }`
  - `#[derive(Clone, Debug, PartialEq, Eq)] pub struct SkinRegion { pub rect: SkinRect, pub norm: Vec<Step>, pub alt: Vec<Step> }`
  - `#[derive(Clone, Debug, PartialEq, Eq, Hash)] pub enum HotSpot { Main, Profile, PrevProf, NextProf, Macros, KeyDefs, Spectate, Ship(u8) /*1..=8*/, Zones, Trace, ZoneUp, ZoneDown, Ping, Options, AdvOpt, Play, Quit, ProfText, ProfBox, ShipBox, InfoBox, ZoneText, ZoneBox, Other(String) }`
  - `#[derive(Clone, Debug, Default, PartialEq, Eq)] pub struct SkinColors { pub zone: [Option<[u8;3]>; 3], pub zone_sel: [Option<[u8;3]>; 2], pub profile: [Option<[u8;3]>; 4] }`
  - `#[derive(Clone, Debug, PartialEq, Eq)] pub struct SkinLayout { pub main: SkinRect, pub spots: Vec<(HotSpot, SkinRegion)>, pub colors: SkinColors, pub warnings: Vec<String> }` with `pub fn regions<'a>(&'a self, h: &'a HotSpot) -> impl Iterator<Item = &'a SkinRegion>` and `pub fn first_rect(&self, h: &HotSpot) -> Option<SkinRect>`
  - `#[derive(Debug)] pub enum SkinError { TooLarge, NotBmp, Image(String), NoMain, Empty }` implementing `Display`
  - `pub fn parse_layout(text: &str) -> Result<SkinLayout, SkinError>`
  - `pub struct Skin { pub name: String, pub image: oss_assets::image::Image, pub layout: SkinLayout }`
  - `pub fn load_skin(name: &str, bytes: &[u8]) -> Result<Skin, SkinError>` — splits at BMP `bfSize` (u32 LE at offset 2), decodes with `oss_assets::image::decode_image`, parses the tail as Latin-1, drops overlays whose rect (point + region size) exceeds the image, recording a warning.
  - `pub fn step_at(steps: &[Step], tick: u64) -> Option<Frame>` — `None` for empty list; cycles through steps honouring each `ticks`.

- [ ] **Step 1: Write failing parser tests** in `skin.rs` `#[cfg(test)] mod tests`:

```rust
#[test] fn parses_main_and_simple_alt() {
    let l = parse_layout("MAIN=(0,0,639,479)\nprofile = (0,0,19,19) + (640,0)").unwrap();
    assert_eq!(l.main, SkinRect { x: 0, y: 0, w: 640, h: 480 });
    let r = l.regions(&HotSpot::Profile).next().unwrap();
    assert_eq!(r.rect, SkinRect { x: 0, y: 0, w: 20, h: 20 });
    assert!(r.norm.is_empty());
    assert_eq!(r.alt, vec![Step { ticks: 1, frame: Frame::At(640, 0) }]);
}
#[test] fn counts_blanks_and_norm_alt_split() {
    let l = parse_layout("main=(0,0,9,9)\nzones=(1,1,5,5)(0,800)2(0,805)(*)2(0,810)+(20,800)3(20,810)").unwrap();
    let r = l.regions(&HotSpot::Zones).next().unwrap();
    assert_eq!(r.norm.iter().map(|s| s.ticks).collect::<Vec<_>>(), vec![1, 2, 1, 2]);
    assert_eq!(r.norm[2].frame, Frame::Blank);
    assert_eq!(r.alt.len(), 2);
}
#[test] fn whitespace_case_and_comments_ignored() {
    let a = parse_layout("main=(0,0,9,9)\nprofile=(0,0,20,20)(800,0)(810,0)+(900,0)2(910,0) ; x").unwrap();
    let b = parse_layout("MAIN = ( 0 , 0 , 9 , 9 )\r\nProfile        =(0,0,     20,20)(800    ,0)(810,0)+(    900,0)2     (910,0)").unwrap();
    assert_eq!(a.spots, b.spots);
}
#[test] fn repeated_lines_add_regions() { /* two `play =` lines -> regions(&Play).count() == 2 */ }
#[test] fn ships_and_unknown_names() { /* "ship3=..." -> HotSpot::Ship(3); "user7=..." -> HotSpot::Other("user7") */ }
#[test] fn colours_parse() { /* zoneselcolor1=(255,0,255) -> colors.zone_sel[0]==Some([255,0,255]); profilecolor4 -> profile[3] */ }
#[test] fn missing_main_is_error() { assert!(matches!(parse_layout("play=(0,0,1,1)"), Err(SkinError::NoMain))); }
#[test] fn skips_bad_lines_keeps_rest() {
    let l = parse_layout("main=(0,0,99,99)\nplay=(10,10,5,5)\ngarbage\nquit=(1,1,2,2)+(oops)\nzones=(0,0,9,9)").unwrap();
    assert!(l.first_rect(&HotSpot::Zones).is_some());
    assert!(l.first_rect(&HotSpot::Play).is_none());
    assert!(l.warnings.len() >= 3);
}
#[test] fn step_sequence_cycles_with_counts() {
    let s = vec![Step { ticks: 1, frame: Frame::At(0,0) }, Step { ticks: 3, frame: Frame::At(1,1) }];
    let got: Vec<_> = (0..8).map(|t| step_at(&s, t).unwrap()).collect();
    assert_eq!(got, vec![Frame::At(0,0), Frame::At(1,1), Frame::At(1,1), Frame::At(1,1), Frame::At(0,0), Frame::At(1,1), Frame::At(1,1), Frame::At(1,1)]);
}
#[test] fn drops_out_of_bounds_overlays() { /* build a 24-bit 50x50 BMP in-test (helper fn bmp(w,h)->Vec<u8>), append "main=(0,0,39,39)\nplay=(0,0,9,9)+(45,45)"; load_skin -> play region alt empty, rect kept, warnings non-empty */ }
#[test] fn load_skin_rejects_non_bmp_and_huge() { /* b"PK.." -> NotBmp; vec of 33 MiB -> TooLarge */ }
```

- [ ] **Step 2: Run** `cargo test -p oss-client skin::` — Expected: compile failure / FAIL.
- [ ] **Step 3: Implement** the Interfaces above. Hand-written tokenizer (no regex crate): strip `;` comment, remove all whitespace, split at first `=`, then scan `(`…`)` groups with optional leading decimal count and a single `+` switching to ALT. Name match case-insensitive; `ship1..ship8` → `Ship(n)`; colour names → `SkinColors`; anything else → `Other(lowercase name)`. `main` with overlays → warning, overlays ignored. RECT with `x2<x1` or `y2<y1` → line skipped with warning.
- [ ] **Step 4: Run** `cargo test -p oss-client skin::` — Expected: all PASS.
- [ ] **Step 5: Commit** — `git commit -m "skin: parse Continuum .skn skins"` (after the four CLAUDE.md gates).

### Task 2: Skin discovery and the remembered choice

**Files:**
- Modify: `crates/oss-client/src/game/skin.rs` (discovery fns)
- Modify: `crates/oss-client/src/config.rs` (`[video] skin`, `[skins] folders`)

**Interfaces:**
- Consumes: `load_skin`, `Skin` (Task 1).
- Produces:
  - `pub const DEFAULT_SKIN: &str = "Dare";`
  - `#[derive(Clone, Debug, PartialEq, Eq)] pub struct SkinEntry { pub name: String, pub path: std::path::PathBuf }`
  - `pub fn find_skins(dirs: &[std::path::PathBuf]) -> Vec<SkinEntry>` — `*.skn` (case-insensitive ext) in each dir, name = file stem; later dir wins a case-insensitive clash; result sorted with `Dare` first, then case-insensitive alphabetical; unreadable dirs skipped.
  - `pub fn skin_dirs(bundled: Option<&Path>, settings_dir: &Path, continuum: Option<&Path>, extra: &[PathBuf]) -> Vec<PathBuf>` — order per Global Constraints.
  - `pub fn pick<'a>(entries: &'a [SkinEntry], wanted: &str) -> Option<&'a SkinEntry>` — case-insensitive name, else the `Dare` entry, else first.
  - `Config` gains `pub skin: String` (default `DEFAULT_SKIN`) read/written as `[video] skin`, and `pub skin_folders: Vec<PathBuf>` read as `[skins] folders = a, b` (comma-separated, trimmed, empty = none).

- [ ] **Step 1: Failing tests** in `skin.rs`: `later_folder_wins_and_dare_first` (tempdirs under `std::env::temp_dir()` with files `b.skn`, `Dare.skn`, `a.SKN`, and a clash `A.skn` in the later dir → names `["Dare","A","b"]`, `A` path from later dir); `pick_is_case_insensitive_then_dare`. In `config.rs`: `skin_and_folders_round_trip` (parse ini with `[video]\nskin = Dare Night\n[skins]\nfolders = /x, /y` → fields set; write then re-read equal) and `skin_defaults_to_dare`.
- [ ] **Step 2: Run** `cargo test -p oss-client skin:: config::` — FAIL.
- [ ] **Step 3: Implement** the Interfaces, following how `config.rs` already parses `[directory] servers` and writes defaults.
- [ ] **Step 4: Run** same — PASS.
- [ ] **Step 5: Commit** `skin: find skins and remember the choice`.

### Task 3: Original skins and their generator

**Files:**
- Create: `skins/src/make_skin.py` (stdlib only), `skins/src/README.md` (how to regenerate; licence: same as repo, art original)
- Create: `assets/skins/Dare.skn`, `assets/skins/Dare Night.skn` (generated, committed)
- Test: `crates/oss-client/src/game/skin.rs` test `bundled_skins_load`

**Interfaces:**
- Produces: two `.skn` files loadable by `load_skin`, each defining at least: `main` (640×420), `profile`, `proftext` (≥184×64), `profbox`, `ship1..8`, `spectate`, `shipbox`, `zones`, `trace`, `zonetext`, `zonebox`, `zoneup`, `zonedown`, `ping`, `infobox`, `options`, `keydefs`, `macros`, `play`, `quit`, `prevprof`, `nextprof`, and `zonecolor1-3`, `zoneselcolor1`, `profilecolor1-4`.
- `python3 -I skins/src/make_skin.py <name> <out.skn>` with names `Dare` | `Dare Night`.

- [ ] **Step 1: Failing test** `bundled_skins_load`: for each of the two files under `assets/skins` (path via `oss_assets::checkout_assets_dir()`), `load_skin` OK, `warnings.is_empty()`, `layout.main.w == 640 && layout.main.h == 420`, and every hot spot listed above has `first_rect` Some.
- [ ] **Step 2: Run** `cargo test -p oss-client bundled_skins_load` — FAIL (files absent).
- [ ] **Step 3: Write the generator.** 24-bit BMP writer + TXT writer appended. Art drawn procedurally per spec: gunmetal bevelled panels, recessed ship bays (36×36 sprite area inside 44×44 bays), "DARE" logotype in our own 5×7-cell block pixel font scaled ×4 with glow, neon cyan/orange accents (Dare) or purple/teal (Dare Night), button NORM art in main region and ALT (lit) overlays placed to the right of main, Play button with a 2-frame blink NORM animation (`3(x,y) 3(*)`), a 6-frame light sweep along the top bar as `user1` NORM animation. No Continuum art used.
- [ ] **Step 4: Generate both, run test** — PASS. Render each with `oss-client --screenshot` after Task 5 exists; for now produce `Dare.png` preview via the generator's `--preview` flag.
- [ ] **Step 5: CHECKPOINT — show Chris the Dare preview image and wait for his OK on the look** before continuing.
- [ ] **Step 6: Commit** `skins: original Dare and Dare Night skins with generator`.

### Task 4: Skin layout math and drawing

**Files:**
- Create: `crates/oss-client/src/game/skin_view.rs`
- Modify: `crates/oss-client/src/game/mod.rs`

**Interfaces:**
- Consumes: `Skin`, `SkinLayout`, `HotSpot`, `step_at`, `TICK_MS` (Task 1); `Renderer::{create_texture, free_texture, quad, size}`; `Art` font region (`art.tallfont`), `color::WHITE` glyph row.
- Produces:
  - `pub fn skin_scale(window: (u32, u32), main: (u32, u32)) -> u32`
  - `pub fn skin_origin(window: (u32, u32), main: (u32, u32), scale: u32) -> (f32, f32)` (physical px, floored, may be negative when clipped)
  - `pub fn to_skin(physical: (f32, f32), origin: (f32, f32), scale: u32) -> (i32, i32)` (skin pixel; may be outside main)
  - `pub fn hotspot_at(layout: &SkinLayout, x: i32, y: i32) -> Option<HotSpot>` — priority: buttons and ships (any region) over `zonetext`/`proftext` over `zonebox`/`profbox`/`shipbox`/`infobox`; never `Main`/`Other`.
  - `pub struct SkinTexture { pub id: TextureId, pub w: u32, pub h: u32 }` with `pub fn upload(r: &mut Renderer, skin: &Skin) -> SkinTexture`
  - `pub struct SkinState<'a> { pub hovered: Option<HotSpot>, pub focused: Option<HotSpot>, pub ship: u8, pub tick: u64, pub profile_lines: [&'a str; 4], pub zones: &'a [ZoneLine], pub selected: Option<usize>, pub scroll: usize, pub ping_graph: bool, pub info: &'a str }`
  - `pub struct ZoneLine { pub name: String, pub players: Option<u32>, pub ping_ms: Option<u32> }`
  - `pub fn zone_rows(layout: &SkinLayout) -> usize` (zonetext.h / 14)
  - `pub fn zone_row_at(layout: &SkinLayout, x: i32, y: i32, scroll: usize, count: usize) -> Option<usize>`
  - `pub fn draw_skin(r: &mut Renderer, tex: &SkinTexture, art: &Art, atlas: TextureId, skin: &Skin, st: &SkinState, window: (u32, u32))` — draws main, overlays (ALT when spot == hovered/focused, or for `Ship(n)`/`Spectate` when selected; `ZoneUp`/`ZoneDown` per guide's scroll rules), ship sprites centred in first rect (frame 0 of each ship sheet via `art`), `proftext` 4 lines pitch 16 max 23 chars in `profilecolor1-4` (fallback green/white/yellow/white), `zonetext` lines pitch 14 as `"PPP name… NNN"` with ping colours `zonecolor1-3` (≤100/≤250/>250 ms, fallback today's `ping_color`), selection bar `zoneselcolor1`, `infobox` word-wrapped. Text helper tints `color::WHITE` glyphs with RGB.

- [ ] **Step 1: Failing tests** in `skin_view.rs`: `scale_fits_3x_on_2560x1600` (main 640×420 → 3; origin `(320.0, 170.0)`), `scale_never_below_one_and_maps_when_clipped` (window 500×300 → 1; origin negative; `to_skin((0.0,0.0), origin, 1) == (70, 60)`), `hotspot_priority` (synthetic layout: `play` inside `zonebox` → `Play`; point only in `zonebox` → `ZoneBox`; point in nothing → None), `zone_row_at_maps_rows_and_scroll` (zonetext y=100 h=140 → rows 10; y=100+14*2+3 with scroll 5, count 20 → Some(7); past count → None).
- [ ] **Step 2: Run** `cargo test -p oss-client skin_view::` — FAIL.
- [ ] **Step 3: Implement** the pure fns, then `upload`/`draw_skin` (overlay quads use `Region { x, y, w: rect.w, h: rect.h }` of the skin texture at `origin + rect.xy*scale`).
- [ ] **Step 4: Run** — PASS.
- [ ] **Step 5: Commit** `skin: layout, hit testing and drawing`.

### Task 5: Skinned start screen wired into the app

**Files:**
- Create: `crates/oss-client/src/game/skin_screen.rs`
- Modify: `crates/oss-client/src/app.rs` (`LoginScreen`, frame/input handling, draw call ~`draw_start_screen`), `crates/oss-client/src/game/login.rs`

**Interfaces:**
- Consumes: Tasks 1–4; existing `Login`, `Choice`, `ZoneBrowser`, `SettingsUi`, `Section`.
- Produces:
  - `#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum SkinAction { Play, Quit, Sandbox, RefreshZones, OpenOptions(Option<Section>), SelectShip(u8), CycleShip(i32), SelectZone(usize), PlayZone(usize), ScrollZones(i32), TogglePing, OpenProfile, NextSkin, Info(&'static str) }` — `Play` goes through `Login::play()`; the others map onto the existing `Choice` handling in `app.rs`.
  - `pub fn click(spot: &HotSpot, zone_row: Option<usize>, double: bool) -> Option<SkinAction>` — table from spec §Interaction (`Trace` → `Info("Trace is coming in a later update")`, `PrevProf`/`NextProf` → None).
  - `pub fn wheel(spot: &HotSpot, rows: i32) -> Option<SkinAction>` — over `ZoneBox`/`ZoneText` → `ScrollZones`, over `ShipBox`/ships → `CycleShip`.
  - `pub fn key(strokes: &[Stroke], cmd: bool, focus: &mut usize) -> Option<SkinAction>` — Enter → Play, Esc → Quit, F2/Cmd-O/Cmd-, → Options, F3/Cmd-R → refresh, F5/Cmd-S → Sandbox, Cmd-Q → Quit, Cmd-K/F7 → NextSkin, `P`/Cmd-P → OpenProfile, Left/Right → CycleShip, Up/Down → ScrollZones(∓1) selecting, Tab/Shift-Tab cycle `FOCUS_ORDER`.
  - `pub const FOCUS_ORDER: [HotSpot; 7]` = Profile, Ship(1), ZoneText, Zones, Options, Play, Quit.
  - `pub fn help(spot: &HotSpot) -> &'static str` — info-box text per hot spot.
  - `Login` keeps its model; `LoginScreen` gains `skin: Skin, skin_tex: SkinTexture, skins: Vec<SkinEntry>, hovered: Option<HotSpot>, focus: usize, scroll: usize, ping_graph: bool, info: String, started: Instant, last_click: Option<(Instant, usize)>`.
  - Cursor for the skin is the physical position (`self.cursor` × `run.scale`), mapped with `to_skin`.
  - Login screen renders with the starfield (`StarParams`, as the game uses) behind the skin.
  - Pressing Play with an empty name opens the profile dialog (Task 6) instead of erroring.
  - The old `draw_start_screen`, `start_scale`, `to_start_scale`, `START_*` consts and `Spot`-based footer are removed once nothing calls them.

- [ ] **Step 1: Failing tests** in `skin_screen.rs`: `click_table` (Play → `SkinAction::Play`; `Ship(4)` → `SelectShip(3)` using 0-based kinds as `Login.ship`; `Spectate` → `SelectShip(SPECTATOR)`; `KeyDefs` → `OpenOptions(Some(Section::Keys))`; `Trace` → Info), `keys_table` (Cmd-K → NextSkin; F7 → NextSkin; Tab advances focus 0→1, Shift-Tab from 0 → 6), `double_click_plays_zone`, `scroll_clamps_after_refresh` (helper `clamp_scroll(scroll, count, rows) -> usize`, plus `keep_selection(old_addr, new_list) -> Option<usize>`: list shrinks from 40 to 5 with scroll 30 → 0; selected address present at new index 2 → Some(2)).
- [ ] **Step 2: Run** `cargo test -p oss-client skin_screen::` — FAIL.
- [ ] **Step 3: Implement** `skin_screen.rs`, then replace the login branch in `app.rs`: load chosen skin at `LoginScreen` creation (`pick` → `load_skin`, falling back to `Dare`, then to the old screen only if even `Dare` fails — log error), feed `SkinState` from `Login` (`profile_lines = ["Default", name, arena or "(any arena)", zone name or address]`) and `ZoneBrowser`, translate `SkinAction`s into the existing `Choice` match.
- [ ] **Step 4: Run** full gates; `--screenshot` with each bundled skin and Chris's three local skins (`[skins] folders` pointed at his Continuum `skins` dir in his own ini, not committed). Inspect images.
- [ ] **Step 5: Commit** `start screen: draw and drive it from the selected skin`.

### Task 6: Profile dialog and skin picker

**Files:**
- Create: `crates/oss-client/src/game/profile_dialog.rs`
- Modify: `crates/oss-client/src/game/login.rs`, `crates/oss-client/src/game/settings_ui.rs`, `crates/oss-client/src/app.rs`

**Interfaces:**
- Consumes: `Login` (`input`, fields), `Canvas::{panel, text}`, `SkinAction::{OpenProfile, NextSkin}`, `find_skins`, `pick`, `Config.skin`.
- Produces:
  - `Login::profile_fields(&self) -> Vec<Field>` (Name, Password, Remember when vault, Arena, Server) and `Login::input_profile(&mut self, strokes: &[Stroke], shift: bool) -> Option<DialogResult>`; `pub enum DialogResult { Close, Play }` (Esc → Close, Enter on valid form → Play, Tab/arrows move among profile fields only; existing `Login::input` stays for tests).
  - `pub fn draw_profile_dialog(c: &Canvas, r: &mut Renderer, login: &Login, cursor: Option<(f32, f32)>) -> Vec<(Rect, Field)>` — centred panel titled "Profile", one line per field, focused line highlighted, clickable lines.
  - `SettingsUi` gains a Video row "Skin" cycling through names passed by `SettingsUi::new_with_skins(cfg: Config, skins: Vec<String>)`; saving writes `[video] skin`.
  - `pub fn next_skin(entries: &[SkinEntry], current: &str) -> &SkinEntry` (wraps).
- [ ] **Step 1: Failing tests**: in `login.rs` `profile_input_moves_only_through_profile_fields` (Tab from Server wraps to Name; typing goes to focused field; Esc → Close; Enter with name+server → Play); in `skin.rs` `next_skin_wraps` and `missing_skin_falls_back_to_dare` (`pick(entries, "Gone")` → Dare); in `settings_ui.rs` `skin_row_cycles_and_saves`.
- [ ] **Step 2: Run** those tests — FAIL.
- [ ] **Step 3: Implement**; in `app.rs` the dialog, while open, owns keys and clicks; `OpenProfile` opens it; `NextSkin` swaps `LoginScreen.skin`/`skin_tex` (freeing the old texture), writes `Config.skin`, and shows the name in the info box.
- [ ] **Step 4: Run** full gates — PASS; screenshot with the dialog open (`--screen` testing flag extended with `profile` if needed).
- [ ] **Step 5: Commit** `start screen: profile dialog and skin picker`.

### Task 7: Package, document, verify

**Files:**
- Modify: `mac/make-app.sh` (nothing if `assets/` copy already includes `assets/skins`; verify), `README.md` (short "Skins" section: where to put `.skn`, Cmd-K/F7, Options → Skin), `docs/playing.md` (keys)

- [ ] **Step 1:** Run `mac/make-app.sh`; launch `/Applications/Dare.app`; confirm skin visible, Cmd-K cycles, profile dialog opens, ship/zone selection, play to Dare (`5.161.238.245:5000`) works.
- [ ] **Step 2:** Final gates; screenshots of Dare, Dare Night, and the three local skins for Chris.
- [ ] **Step 3: Commit** `docs: skins`.
