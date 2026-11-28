# Settings, graphics and sound

## The settings file

`oss-client.ini` lives in your settings folder — `%APPDATA%\OpenSubSpace` on Windows,
`~/.config/open-subspace` elsewhere — or wherever `--config <file>` says. It is written with
the defaults on the first run, and command-line flags override it for that run without
rewriting it.

It holds video and audio settings, your player name, every key and gamepad binding, macro
keys (`[macros]`, e.g. `Ctrl+F1 = 'help at base`), the accessibility palette and interface
scale, the directory servers for the zone browser (`[directory] servers = host:port`), and
the encryption policy (`[network] security`, `[network] identity`).

`known_servers.txt` and `identities.txt` sit beside it (see [encryption.md](encryption.md)),
Print Screen saves into a `screenshots` folder there, and maps, LVZ files and zone
news are cached per server. The key prints the file they just wrote into the chat log,
so a screenshot taken after another one shows a local path; use `--screenshot` (see
[testing.md](testing.md)) for anything you intend to publish.

## The settings window

Most of the file can be changed from inside the client. Press **F2** (or click **Options**) on
the start-up screen, or **Esc then O** in the game. The window is drawn in the game's own box:
the sections down the left — Video, HUD, Audio, Accessibility, Keys, Gamepad, Macros — and the
picked section's settings on the right.

| Section | Holds |
|---|---|
| Video | Zoom (Auto, or 1x to 8x in fine steps, shown as how many tiles of the map fit on screen), fullscreen, vsync |
| HUD | HUD size, radar size, radar zoom, names on ships, chat lines, name column, frame rate |
| Audio | Mute, volume, voice volume |
| Accessibility | Colour palette, Enter to chat |
| Keys | The keys for every action: Change, Add or Clear each one, or restore Continuum's defaults |
| Gamepad | The button for every action: Change or Clear each one, or restore the default layout |
| Macros | Keys that send a line of chat: add one, type its line, or remove it |

It works fully from the keyboard and fully from the pointer:

| Key | Does | With the pointer |
|---|---|---|
| Tab / Shift+Tab | Next or previous section | Click a section |
| Up / Down (PgUp, PgDn, Home, End) | Move between lines | Click a line; the wheel, or `^` `v` beside the heading, scrolls a long list |
| Left / Right | Change a number or a choice, flip a switch, or move along a line's buttons | Click `<` or `>`, a switch, or a button |
| Enter | Change the picked line or press the picked button | Click it |
| Ctrl+S | Save to the settings file | Click `Ctrl+S = Save` |
| Esc | Close. With changes not yet saved, the first Esc says so and the second closes without saving | Click `Esc = Close` |

Change or Add on a key waits for the next key you press, with Shift or Ctrl if held; Esc
cancels the wait, and so does a click. On a macro, typing goes into the line it sends.
The line at the foot says what the picked line, or the one under the pointer, does.

Save writes the file and applies at once everything that can change while playing: keys,
buttons, macros, volume, zoom, fullscreen, palette and the HUD section. Vsync and
mute take effect at the next start. Opened over the game (Esc then O), the zoom, the HUD
section, the volumes and the palette change behind the window as you pick them, so you see
each one on the game itself; closing without saving puts them back. It does not have a
player name: that is set on the start-up screen.

In browser play there is no settings file: Save keeps the whole file in the browser, for
the pilot who is signed in, and the next Enter as that pilot starts with it. Keys and macros
the pilot's account holds still win over the browser's copy.

## Accessibility and scaling

`[chat] enter_to_chat` (on by default; *Enter to chat* in the window's Accessibility section):
Enter opens the chat line and V opens the voice menu. Off, every key types straight into chat
as in SubSpace and Continuum, and the voice menu is not available.

`[accessibility] palette` picks a colour-blind or high-contrast palette.

`[video] scale` is the world's zoom: screen pixels to each game pixel, 0 for the display's
own (2 on a high-DPI screen). Between 1 and 2 it moves in eighths, then quarters up to 4,
then 6 and 8; every step draws a map tile a whole number of pixels wide. A whole number from
an older file still reads. `--scale <n>` sets it for one run and `Ctrl +/-` steps it in game.

`[hud]` sizes what is drawn over the world, each in percent of what the game picks by itself:
`size` (text, gauges, chat and menus, 50-300; the old `[accessibility] ui_scale` is read
as this), `radar_size` (50-250) and `radar_zoom` (25-400; 200 shows half as much of the map,
twice as large). `ship_names` is `name_and_bounty`, `name` or `off`; `frame_rate` is what
F12's line shows at the start (`off`, `on`, `detail`). `[chat] lines` (0 = from the screen's
height) and `name_width` (1-24) are `?lines` and `?namelen` kept between games, and
`[audio] voice_volume` sets voice lines and chat sounds under the volume.

The HUD keeps the display's whole-number scale (or the zoom's whole part, where that is
more) at a size of 100%, so a fine zoom like 1.5x moves the world without blurring the
8x12 letters. `--fullscreen` is borderless. `--vsync` caps the frame rate to the display
refresh; `--mute` turns the sound off.

## Where graphics and sounds come from

Each file is looked up, one at a time, in this order — so a folder that holds only a few
files overrides only those:

1. `--assets-dir <dir>` / `OSS_ASSETS_DIR` — your own folder
2. `--continuum-dir <dir>` / `OSS_CONTINUUM_DIR` — a Continuum install
3. `assets/classic` beside the program, and the repository's `assets/classic` for a binary
   built from a checkout (any profile, so `cargo run --release` works too)
4. a Continuum install found automatically (Steam or the standard paths)

`assets/classic` in this repository holds Continuum's own graphics and sounds, bundled so
that a fresh clone plays with no Continuum install anywhere. They are not this project's
work and are not covered by its GPL licence;
[assets/classic/README.md](../assets/classic/README.md) says what they are and whose. If you
have an install of your own, `--continuum-dir` uses it instead: `oss-assets` reads
Continuum's `.bm2` and `.wa2` containers as well as `.bmp`, `.png`, `.jpg`, `.gif` and
`.wav`.

## LVZ

Online, an arena's LVZ files — zone graphics: map and screen objects, images, animations and
the server's toggles for them — are downloaded and cached along with the map. Offline you can
point at one directly:

```sh
oss-client --sandbox path/to/map.lvl --lvz path/to/art.lvz
```

`--lvz` is repeatable and also works in a zone, on top of whatever the arena sends.
