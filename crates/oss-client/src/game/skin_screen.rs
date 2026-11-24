//! What the skinned start screen does when a hot spot is clicked or a key pressed.
//!
//! Pure tables from input to [`SkinAction`]; `app.rs` carries the actions out with
//! the same handling the old start screen used.

use crate::game::input::Stroke;
use crate::game::settings_ui::Section;
use crate::game::skin::HotSpot;
use oss_session::directory::ZoneInfo;
use winit::keyboard::KeyCode;

/// Something the start screen should do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkinAction {
    Play,
    Quit,
    Sandbox,
    RefreshZones,
    /// Open the options, at a section if given.
    OpenOptions(Option<Section>),
    /// A ship 0-7, or the spectator value.
    SelectShip(u8),
    CycleShip(i32),
    SelectZone(usize),
    PlayZone(usize),
    ScrollZones(i32),
    /// Move the zone selection up or down.
    MoveSelection(i32),
    TogglePing,
    OpenProfile,
    NextSkin,
    Info(&'static str),
    /// Ask to open the arena application (browser build).
    CreateArena,
}

/// Running in the browser, where the list is Foundation's arenas.
pub const IN_BROWSER: bool = cfg!(target_arch = "wasm32");

/// The confirmation "+ CREATE" shows before it opens a tab.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub const CREATE_CONFIRM: &str =
    "Open the arena application in a new tab? Enter to confirm, Esc to cancel.";

/// What the `trace` hot spot does: "+ CREATE" in the browser, a note elsewhere.
pub fn trace_action(browser: bool) -> SkinAction {
    if browser {
        SkinAction::CreateArena
    } else {
        SkinAction::Info("Trace is coming in a later update.")
    }
}

/// The answer to a pending confirmation in this frame's keys: Enter is yes, Esc is
/// no. Asked before [`key`], so the Enter is not also taken as Play.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub fn confirm_key(strokes: &[Stroke]) -> Option<bool> {
    strokes
        .iter()
        .filter(|s| !s.repeat)
        .find_map(|s| match s.key? {
            KeyCode::Enter | KeyCode::NumpadEnter => Some(true),
            KeyCode::Escape => Some(false),
            _ => None,
        })
}

/// The hot spots Tab walks through, in order.
pub const FOCUS_ORDER: [HotSpot; 7] = [
    HotSpot::Profile,
    HotSpot::Ship(1),
    HotSpot::ZoneText,
    HotSpot::Zones,
    HotSpot::Options,
    HotSpot::Play,
    HotSpot::Quit,
];

/// A click on `spot`; `zone_row` is the zone under the pointer when it is the list.
pub fn click(spot: &HotSpot, zone_row: Option<usize>, double: bool) -> Option<SkinAction> {
    use SkinAction as A;
    Some(match spot {
        HotSpot::Play => A::Play,
        HotSpot::Quit => A::Quit,
        HotSpot::Ship(n) => A::SelectShip(n.saturating_sub(1)),
        HotSpot::Spectate => A::SelectShip(oss_session::SPECTATOR),
        HotSpot::KeyDefs => A::OpenOptions(Some(Section::Keys)),
        HotSpot::Macros => A::OpenOptions(Some(Section::Macros)),
        HotSpot::Options | HotSpot::AdvOpt => A::OpenOptions(None),
        HotSpot::Zones => A::RefreshZones,
        HotSpot::Ping => A::TogglePing,
        HotSpot::Profile | HotSpot::ProfText | HotSpot::ProfBox => A::OpenProfile,
        HotSpot::ZoneUp => A::ScrollZones(-1),
        HotSpot::ZoneDown => A::ScrollZones(1),
        HotSpot::Trace => trace_action(IN_BROWSER),
        HotSpot::ZoneText => {
            let row = zone_row?;
            if double {
                A::PlayZone(row)
            } else {
                A::SelectZone(row)
            }
        }
        _ => return None,
    })
}

/// The mouse wheel over `spot`, `rows` notches (positive is down).
pub fn wheel(spot: &HotSpot, rows: i32) -> Option<SkinAction> {
    match spot {
        HotSpot::ZoneBox | HotSpot::ZoneText | HotSpot::ZoneUp | HotSpot::ZoneDown => {
            Some(SkinAction::ScrollZones(rows))
        }
        HotSpot::ShipBox | HotSpot::Ship(_) | HotSpot::Spectate => {
            Some(SkinAction::CycleShip(rows))
        }
        _ => None,
    }
}

/// A frame's keystrokes. `cmd` is Cmd or Ctrl held. Tab moves `focus` through
/// [`FOCUS_ORDER`]; Space presses the focused spot.
pub fn key(strokes: &[Stroke], cmd: bool, shift: bool, focus: &mut usize) -> Option<SkinAction> {
    use SkinAction as A;
    let n = FOCUS_ORDER.len();
    for s in strokes {
        let Some(k) = s.key else { continue };
        // Keys a person holds on purpose repeat; the rest act once.
        match k {
            KeyCode::ArrowLeft => return Some(A::CycleShip(-1)),
            KeyCode::ArrowRight => return Some(A::CycleShip(1)),
            KeyCode::ArrowUp => return Some(A::MoveSelection(-1)),
            KeyCode::ArrowDown => return Some(A::MoveSelection(1)),
            KeyCode::PageUp => return Some(A::ScrollZones(-10)),
            KeyCode::PageDown => return Some(A::ScrollZones(10)),
            _ => {}
        }
        if s.repeat {
            continue;
        }
        let act = if cmd {
            match k {
                KeyCode::KeyO | KeyCode::Comma => A::OpenOptions(None),
                KeyCode::KeyR => A::RefreshZones,
                KeyCode::KeyS => A::Sandbox,
                KeyCode::KeyQ => A::Quit,
                KeyCode::KeyK => A::NextSkin,
                KeyCode::KeyP => A::OpenProfile,
                _ => continue,
            }
        } else {
            match k {
                KeyCode::Enter | KeyCode::NumpadEnter => A::Play,
                KeyCode::Escape => A::Quit,
                KeyCode::F2 => A::OpenOptions(None),
                KeyCode::F3 => A::RefreshZones,
                KeyCode::F5 => A::Sandbox,
                KeyCode::F7 => A::NextSkin,
                KeyCode::KeyP => A::OpenProfile,
                KeyCode::Tab => {
                    *focus = if shift {
                        (*focus + n - 1) % n
                    } else {
                        (*focus + 1) % n
                    };
                    continue;
                }
                KeyCode::Space => match click(&FOCUS_ORDER[*focus % n], None, false) {
                    Some(a) => a,
                    None => continue,
                },
                _ => continue,
            }
        };
        return Some(act);
    }
    None
}

/// A scroll position that keeps the list's last page full.
pub fn clamp_scroll(scroll: usize, count: usize, rows: usize) -> usize {
    scroll.min(count.saturating_sub(rows))
}

/// The scroll position that shows `selected`, moving as little as possible.
pub fn scroll_to_show(selected: usize, scroll: usize, rows: usize) -> usize {
    let rows = rows.max(1);
    if selected < scroll {
        selected
    } else if selected >= scroll + rows {
        selected + 1 - rows
    } else {
        scroll
    }
}

/// Where the zone that was selected is now, after the list changed.
pub fn keep_selection(old_address: Option<&str>, zones: &[ZoneInfo]) -> Option<usize> {
    if zones.is_empty() {
        return None;
    }
    Some(
        old_address
            .and_then(|a| zones.iter().position(|z| z.address == a))
            .unwrap_or(0),
    )
}

/// The info box's text for a hot spot.
/// The help line for `spot` on this build's start screen.
pub fn screen_help(spot: &HotSpot) -> &'static str {
    help_for(spot, IN_BROWSER)
}

/// The help line for `spot`; `browser` gives the arena list's wording.
pub fn help_for(spot: &HotSpot, browser: bool) -> &'static str {
    if browser {
        match spot {
            HotSpot::Trace => return "+ Create: apply for your own Foundation arena.",
            HotSpot::Ping => return "Sort arenas by pilots or by name.",
            HotSpot::Zones => return "Refresh the arena list. (F3, Cmd-R)",
            HotSpot::ZoneText | HotSpot::ZoneBox => {
                return "Click an arena to pick it; double-click to play there. (Up/Down)";
            }
            HotSpot::ZoneUp | HotSpot::ZoneDown => {
                return "Scroll the arena list. (wheel, PgUp/PgDn)";
            }
            _ => {}
        }
    }
    help(spot)
}

/// The desktop help line for `spot`.
pub fn help(spot: &HotSpot) -> &'static str {
    match spot {
        HotSpot::Play => "Play: join the selected zone. (Enter)",
        HotSpot::Quit => "Quit the game. (Esc, Cmd-Q)",
        HotSpot::Profile | HotSpot::ProfText | HotSpot::ProfBox => {
            "Profile: your name, password, arena and zone address. (P)"
        }
        HotSpot::PrevProf | HotSpot::NextProf => {
            "Switch profile. More profiles are coming in a later update."
        }
        HotSpot::Ship(_) | HotSpot::ShipBox => "Pick the ship you enter in. (Left/Right, wheel)",
        HotSpot::Spectate => "Enter as a spectator, watching the game.",
        HotSpot::Zones => "Refresh the zone list from the directory servers. (F3, Cmd-R)",
        HotSpot::Trace => "Trace is coming in a later update.",
        HotSpot::ZoneText | HotSpot::ZoneBox => {
            "Click a zone to pick it; double-click to play. (Up/Down)"
        }
        HotSpot::ZoneUp | HotSpot::ZoneDown => "Scroll the zone list. (wheel, PgUp/PgDn)",
        HotSpot::Ping => "Show ping as a number or a bar.",
        HotSpot::Options | HotSpot::AdvOpt => "Options: video, sound, keys and more. (F2, Cmd-O)",
        HotSpot::KeyDefs => "Change your keys.",
        HotSpot::Macros => "Edit your chat macros.",
        HotSpot::InfoBox | HotSpot::Main | HotSpot::Other(_) => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use winit::keyboard::KeyCode;

    fn press(k: KeyCode) -> Vec<Stroke> {
        vec![Stroke::press(k)]
    }

    #[test]
    fn click_table() {
        assert_eq!(click(&HotSpot::Play, None, false), Some(SkinAction::Play));
        assert_eq!(click(&HotSpot::Quit, None, false), Some(SkinAction::Quit));
        assert_eq!(
            click(&HotSpot::Ship(4), None, false),
            Some(SkinAction::SelectShip(3))
        );
        assert_eq!(
            click(&HotSpot::Spectate, None, false),
            Some(SkinAction::SelectShip(oss_session::SPECTATOR))
        );
        assert_eq!(
            click(&HotSpot::KeyDefs, None, false),
            Some(SkinAction::OpenOptions(Some(Section::Keys)))
        );
        assert_eq!(
            click(&HotSpot::Macros, None, false),
            Some(SkinAction::OpenOptions(Some(Section::Macros)))
        );
        assert_eq!(
            click(&HotSpot::Options, None, false),
            Some(SkinAction::OpenOptions(None))
        );
        assert_eq!(
            click(&HotSpot::AdvOpt, None, false),
            Some(SkinAction::OpenOptions(None))
        );
        assert_eq!(
            click(&HotSpot::Zones, None, false),
            Some(SkinAction::RefreshZones)
        );
        assert_eq!(
            click(&HotSpot::Ping, None, false),
            Some(SkinAction::TogglePing)
        );
        assert_eq!(
            click(&HotSpot::Profile, None, false),
            Some(SkinAction::OpenProfile)
        );
        assert_eq!(
            click(&HotSpot::ProfText, None, false),
            Some(SkinAction::OpenProfile)
        );
        assert_eq!(
            click(&HotSpot::ZoneUp, None, false),
            Some(SkinAction::ScrollZones(-1))
        );
        assert_eq!(
            click(&HotSpot::ZoneDown, None, false),
            Some(SkinAction::ScrollZones(1))
        );
        assert!(matches!(
            click(&HotSpot::Trace, None, false),
            Some(SkinAction::Info(_))
        ));
        assert_eq!(click(&HotSpot::PrevProf, None, false), None);
        assert_eq!(click(&HotSpot::Other("user1".into()), None, false), None);
    }

    #[test]
    fn confirm_key_reads_enter_and_escape() {
        assert_eq!(confirm_key(&press(KeyCode::Enter)), Some(true));
        assert_eq!(confirm_key(&press(KeyCode::NumpadEnter)), Some(true));
        assert_eq!(confirm_key(&press(KeyCode::Escape)), Some(false));
        assert_eq!(confirm_key(&press(KeyCode::KeyX)), None);
        assert_eq!(confirm_key(&[]), None);
    }

    #[test]
    fn confirm_takes_enter_before_play() {
        // The same Enter that would play answers the confirmation; the caller
        // asks confirm_key first and does not pass the stroke on.
        let enter = press(KeyCode::Enter);
        let mut focus = 0;
        assert_eq!(
            key(&enter, false, false, &mut focus),
            Some(SkinAction::Play)
        );
        assert_eq!(confirm_key(&enter), Some(true));
    }

    #[test]
    fn trace_is_create_in_the_browser() {
        assert_eq!(trace_action(true), SkinAction::CreateArena);
        assert!(matches!(trace_action(false), SkinAction::Info(_)));
        assert_eq!(
            help_for(&HotSpot::Trace, true),
            "+ Create: apply for your own Foundation arena."
        );
        assert_eq!(
            help_for(&HotSpot::Ping, true),
            "Sort arenas by pilots or by name."
        );
        assert_eq!(
            help_for(&HotSpot::Zones, true),
            "Refresh the arena list. (F3, Cmd-R)"
        );
        assert_eq!(
            help_for(&HotSpot::ZoneText, true),
            "Click an arena to pick it; double-click to play there. (Up/Down)"
        );
        assert_eq!(help_for(&HotSpot::Ping, false), help(&HotSpot::Ping));
        assert_eq!(help_for(&HotSpot::Play, true), help(&HotSpot::Play));
    }

    #[test]
    fn double_click_plays_zone() {
        assert_eq!(
            click(&HotSpot::ZoneText, Some(5), false),
            Some(SkinAction::SelectZone(5))
        );
        assert_eq!(
            click(&HotSpot::ZoneText, Some(5), true),
            Some(SkinAction::PlayZone(5))
        );
        assert_eq!(click(&HotSpot::ZoneText, None, true), None);
    }

    #[test]
    fn wheel_table() {
        assert_eq!(
            wheel(&HotSpot::ZoneBox, 2),
            Some(SkinAction::ScrollZones(2))
        );
        assert_eq!(
            wheel(&HotSpot::ZoneText, -1),
            Some(SkinAction::ScrollZones(-1))
        );
        assert_eq!(wheel(&HotSpot::ShipBox, 1), Some(SkinAction::CycleShip(1)));
        assert_eq!(
            wheel(&HotSpot::Ship(2), -1),
            Some(SkinAction::CycleShip(-1))
        );
        assert_eq!(wheel(&HotSpot::Play, 1), None);
    }

    #[test]
    fn keys_table() {
        let mut f = 0;
        assert_eq!(
            key(&press(KeyCode::Enter), false, false, &mut f),
            Some(SkinAction::Play)
        );
        assert_eq!(
            key(&press(KeyCode::Escape), false, false, &mut f),
            Some(SkinAction::Quit)
        );
        assert_eq!(
            key(&press(KeyCode::KeyK), true, false, &mut f),
            Some(SkinAction::NextSkin)
        );
        assert_eq!(
            key(&press(KeyCode::F7), false, false, &mut f),
            Some(SkinAction::NextSkin)
        );
        assert_eq!(
            key(&press(KeyCode::KeyQ), true, false, &mut f),
            Some(SkinAction::Quit)
        );
        assert_eq!(
            key(&press(KeyCode::KeyO), true, false, &mut f),
            Some(SkinAction::OpenOptions(None))
        );
        assert_eq!(
            key(&press(KeyCode::F2), false, false, &mut f),
            Some(SkinAction::OpenOptions(None))
        );
        assert_eq!(
            key(&press(KeyCode::KeyR), true, false, &mut f),
            Some(SkinAction::RefreshZones)
        );
        assert_eq!(
            key(&press(KeyCode::F3), false, false, &mut f),
            Some(SkinAction::RefreshZones)
        );
        assert_eq!(
            key(&press(KeyCode::KeyS), true, false, &mut f),
            Some(SkinAction::Sandbox)
        );
        assert_eq!(
            key(&press(KeyCode::F5), false, false, &mut f),
            Some(SkinAction::Sandbox)
        );
        assert_eq!(
            key(&press(KeyCode::KeyP), true, false, &mut f),
            Some(SkinAction::OpenProfile)
        );
        assert_eq!(
            key(
                &[Stroke::press(KeyCode::KeyP).typing('p')],
                false,
                false,
                &mut f
            ),
            Some(SkinAction::OpenProfile)
        );
        assert_eq!(
            key(&press(KeyCode::ArrowRight), false, false, &mut f),
            Some(SkinAction::CycleShip(1))
        );
        assert_eq!(
            key(&press(KeyCode::ArrowUp), false, false, &mut f),
            Some(SkinAction::MoveSelection(-1))
        );
        assert_eq!(key(&press(KeyCode::KeyK), false, false, &mut f), None);
        // repeats of one-shot keys do nothing
        assert_eq!(
            key(
                &[Stroke::press(KeyCode::F7).repeated(true)],
                false,
                false,
                &mut f
            ),
            None
        );
    }

    #[test]
    fn tab_cycles_focus_and_space_activates() {
        let mut f = 0;
        assert_eq!(key(&press(KeyCode::Tab), false, false, &mut f), None);
        assert_eq!(f, 1);
        let mut g = 0;
        key(&press(KeyCode::Tab), false, true, &mut g);
        assert_eq!(g, FOCUS_ORDER.len() - 1);
        assert_eq!(
            key(&press(KeyCode::Space), false, false, &mut g),
            Some(SkinAction::Quit)
        );
    }

    #[test]
    fn scroll_clamps_after_refresh() {
        assert_eq!(clamp_scroll(30, 5, 15), 0);
        assert_eq!(clamp_scroll(30, 40, 15), 25);
        assert_eq!(clamp_scroll(3, 40, 15), 3);
        let z = |a: &str| ZoneInfo {
            name: a.into(),
            description: String::new(),
            address: a.into(),
            players: 0,
            ping_ms: None,
        };
        let list = vec![z("a:1"), z("b:2"), z("c:3")];
        assert_eq!(keep_selection(Some("c:3"), &list), Some(2));
        assert_eq!(keep_selection(Some("gone:9"), &list), Some(0));
        assert_eq!(keep_selection(None, &[]), None);
    }

    #[test]
    fn scroll_to_show_selection() {
        assert_eq!(scroll_to_show(20, 0, 15), 6);
        assert_eq!(scroll_to_show(2, 10, 15), 2);
        assert_eq!(scroll_to_show(12, 10, 15), 10);
    }

    #[test]
    fn every_spot_has_help() {
        for h in FOCUS_ORDER
            .iter()
            .chain([HotSpot::Trace, HotSpot::Ping, HotSpot::KeyDefs].iter())
        {
            assert!(!help(h).is_empty(), "{h:?}");
        }
    }
}
