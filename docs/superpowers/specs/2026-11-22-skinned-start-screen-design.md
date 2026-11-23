# Skinned start screen (Dare client) — design

Date: 2026-11-22 · Branch: `dare` · Status: draft for approval

## Intent

Make the Dare client's start screen look and behave like Continuum 0.40's: a skinned
window that fills the screen, where the look comes from a skin file and the player can
switch between several skins. It is first for Chris's own Mac and will later be handed
to Dare players, so everything shipped must be ours to give away.

This is project 1 of 3. Project 2 is multiple saved profiles (name, password, keys,
macros per profile); project 3 is zone-list extras (custom zones, favourites, trace).
Where this project meets them it leaves a clean hook rather than a half-built feature.

## Goals

1. **Continuum `.skn` skins drive the start screen.** Any 0.40 skin (BMP + appended
   TXT, as described in Continuum's *Skins Development Guide* v1.2) loads and works:
   every hot spot, NORM/ALT overlays, multi-region hot spots, `(*)` placeholders,
   tick counts (≈200 ms ticks), and the colour definitions.
2. **Several skins to pick from**, as in 0.40: a skin picker, remembered between runs.
3. **Original skins we can ship**: a default "Dare" skin plus at least one alternative,
   drawn by us, in the same `.skn` format, built from a generator script in the repo.
4. **Fills the window.** The skin is drawn at the largest whole-number scale that fits,
   centred, over an animated starfield; pixel art stays crisp. Pointer positions map
   back to skin pixels exactly.
5. **Nothing the current screen does is lost**: choose ship or spectator, pick a zone,
   enter name/password/arena, type a custom zone address, register a new name,
   "remember password", options, zone refresh, sandbox, play, quit, all keyboard
   shortcuts (including the Cmd shortcuts) and the mouse.

## Non-goals

- Multiple profiles, macro sets and keydef sets (project 2). `prevprof`/`nextprof` are
  parsed and drawn as "no other profile"; `proftext` shows one profile.
- Custom zone lists and `trace` (project 3). `trace` is parsed; clicking it shows
  "Coming soon" in the info box.
- A native macOS menu bar.
- Skinning anything but the start screen (in-game HUD, dialogs keep their look).

## Anti-goals

- **Shipping art we may not give away.** The three skins in Chris's Continuum install
  (Ceinwyn, Pipe Dreams, VIEd) carry no redistribution permission; they are loaded from
  the player's own folders, never bundled. Continuum's built-in `<default>` skin art is
  not copied or imitated pixel-for-pixel.
- A skin that can crash the client: a malformed `.skn` is reported and skipped.
- Losing keyboard-only play: every hot spot has a key path.

## Constraints

- Lives in the `dare` branch of oss-client; respects that repo's `CLAUDE.md` (fmt,
  clippy `-D warnings`, tests, `cargo doc -D warnings` before commits; untrusted file
  data never panics; no captures or Continuum files committed).
- Rendering goes through the existing wgpu sprite renderer (WebGPU-compatible).
- BMP decoding uses `oss_assets::image::decode_image` (8- and 24-bit both occur in
  real skins). Overlay rectangles are inclusive (`x2,y2` included).

## Approaches considered

- **A. The start screen *is* a skin (chosen).** One code path; the default look is just
  the shipped `dare.skn`. Behaviour matches 0.40 exactly and community skins drop in.
- **B. Restyle the code-drawn screen and add skins as a second mode.** Faster to a
  nicer look, but two screens forever and the default never behaves like a skin.
- **C. Our own layout format (JSON etc.).** Rejected: incompatible with every existing
  skin, which is the point of the feature.

Chris picked A ("do whatever nice thing you can").

## Design

### Skin files and discovery

- Parser module `game/skin.rs`: split file at the BMP's declared size (`bfSize`); decode
  the BMP; parse the TXT per the guide (case-insensitive names, `;` comments, any
  whitespace, `HOTSPOT = RECT [NORM…] [+ ALT…]`, `COLORDEF = (r,g,b)`, repeated lines
  append regions). Unknown hot-spot names are kept (user-defined regions animate per
  their NORM list) but have no action. Overlays out of the bitmap's bounds are dropped
  with a warning, never panics.
- Skins are listed from, in order: bundled `assets/skins/`, the user folder
  `<settings dir>/skins/`, and `<continuum dir>/skins/` when a Continuum install is
  known (`--continuum-dir`). Duplicate names: user folder wins.
- Chosen skin is remembered in `login.ini` as `skin = <name>`; default `Dare`.

### Drawing

- Scale `s` = largest integer with `main.w*s ≤ 0.92·window.w` and
  `main.h*s ≤ 0.92·window.h`, minimum 1. Centred; the margin shows a slow-drifting
  starfield (existing star art) so the window never looks letterboxed.
- Each frame: draw `main`, then for each hot spot each region's current overlay
  (NORM when not highlighted, ALT when highlighted; tick index advances every 200 ms;
  `(*)` draws nothing). Highlighted = pointer over it, or it is the keyboard focus, or
  (ships/spectate) it is the selected ship.
- Ships and spectate: the client draws the ship sprite (existing ship art) centred in
  the region's first rectangle; selected ship gets ALT overlay plus a frame.
- `proftext`: four lines, 16 px pitch, 8×12 font, max 23 chars: profile name
  ("Default"), player name, arena (`(any)`), zone name/address. (0.40 showed macro and
  keydef set names; those return with project 2.)
- `zonetext`: one line per zone, 14 px pitch: 3-digit ping (or ping bar when graph mode
  is on, toggled by `ping`), zone name truncated to fit, 3-digit player count. Colours
  from `zonecolor1-3` / `zoneselcolor1-2` / `profilecolor1-4`, falling back to today's
  ping colours.
- `infobox`: help text for whatever the pointer is over (wrapped 8×12), else status
  (connecting errors, "24 zones", refresh progress) — the job today's footer line does.
- A skin with no `zonetext`/`proftext` still works; missing pieces just are not shown.

### Interaction (hot spot → action)

| Hot spot | Action |
|---|---|
| `play` | play (same as Enter) |
| `quit` | quit (Esc, Cmd-Q) |
| `ship1..8`, `spectate` | select ship; mouse wheel over `shipbox` cycles ships |
| `zonetext` row | select zone; double-click plays |
| `zoneup` / `zonedown`, wheel over `zonebox` | scroll list |
| `zones` | refresh directory list (F3 / Cmd-R) |
| `profile`, `profbox` click | open **Profile dialog** (below) |
| `options` / `advopt` | open existing Options (F2 / Cmd-O) |
| `keydefs` / `macros` | open Options on the Keys / Macros section |
| `ping` | toggle ping number ↔ bar graph |
| `trace` | info box: "Trace is coming in a later update" |
| `prevprof`/`nextprof` | no-op until project 2 |

Keyboard: Enter plays; Esc quits; Tab cycles the clickable hot spots in a fixed order
(profile, ships, zone list, zones, options, play, quit) drawing ALT on the focused one;
arrows move within ships/zone list; F-keys and Cmd shortcuts unchanged; Cmd-K / F7
cycles skins; `P` or Cmd-P opens the Profile dialog.

### Profile dialog

The name/password/arena/zone-address fields move out of the main screen into a modal
panel (drawn with the existing in-game dialog style, centred over the skin), opened by
`profile`, by typing any letter while nothing else has focus, or by pressing Enter with
no name set. Fields: Name, Password, Remember, Arena, Zone address (custom address
entry), plus "New user" for registration. Same validation and `login.ini` memory as
today's `Login` model — the model is reused, only its drawing moves. This dialog is
the seam project 2 grows into (profile list on its left).

### Skin picker

Cmd-K/F7 cycles skins with the name shown in the info box; Options gains a "Skin" row
listing all found skins. Change applies instantly and is remembered.

### Original skins

- `skins/src/` holds a stdlib-only Python generator producing `dare.bmp` + `dare.txt` →
  `dare.skn` (makeskin-compatible layout). Art is drawn procedurally: dark gunmetal
  panels with bevels, recessed ship bays, a neon-cyan/orange "DARE" logotype in our own
  pixel lettering, glowing NORM/ALT button states, a blinking Play button, a sweeping
  light animation along the top bar. Target main size 640×420 (scales 3× at 2560×1600).
- A second skin, "Dare Night" (purple/teal, minimal), from the same generator with a
  different palette and layout, proving the picker.
- Built `.skn` files are committed under `assets/skins/` so the app needs no Python.
- Checkpoint: the Dare skin image is rendered and shown to Chris before wiring the rest.

### Testing

- Unit tests for the TXT parser against synthetic skin text covering every syntax form
  in the guide (counts, `(*)`, `+`, multi-region, colours, comments, bad lines).
- Unit test for overlay tick sequencing (A B B B A …).
- Scale/centre/pointer-mapping tests.
- Manual check with the three real skins from Chris's install (not committed) and the
  two shipped skins, via `--screenshot`.

## Left to the builder

Exact pixel art, help-text wording, starfield speed, Tab order details, and how the
dialog panel is styled, within the above.
