//! The skinned start screen's profile dialog: name, password, arena and zone.
//!
//! Continuum keeps these behind its `profile` button rather than on the start
//! screen itself. The fields and their rules are [`Login`]'s; this only draws
//! them, in the game's own panel, over the skin.

use crate::game::login::{Field, Login};
use crate::game::start_spots::Rect;
use crate::game::view::{Canvas, color, ui_color};
use crate::gfx::Renderer;

/// Width of the label column and the whole panel, in characters.
const LABEL_COLS: usize = 10;
const COLS: usize = 46;

fn label(f: Field) -> &'static str {
    match f {
        Field::Name => "Name",
        Field::Password => "Password",
        Field::Remember => "Remember",
        Field::Arena => "Arena",
        Field::Server => "Zone",
        Field::Ship | Field::Zones => "",
    }
}

/// Draws the dialog centred on the canvas; returns each line's clickable rectangle.
pub fn draw_profile_dialog(
    c: &Canvas,
    r: &mut Renderer,
    login: &Login,
    cursor: Option<(f32, f32)>,
) -> Vec<(Rect, Field)> {
    let fields = login.fields();
    let w = COLS as f32 * 8.0;
    let h = 20.0 + fields.len() as f32 * 14.0 + 8.0 + 14.0 + 10.0 + 14.0 + 14.0;
    let x = ((c.view_w - w) / 2.0).floor();
    let top = ((c.view_h - h) / 2.0).floor().max(4.0);
    c.panel(r, x, top, w, h);
    c.text_center(r, x + w / 2.0, top + 1.0, color::GREEN, "Profile");
    c.separator(r, x, top + 13.0, w);

    let mut spots = Vec::new();
    let focus = login.focused();
    let value_x = x + 4.0 + LABEL_COLS as f32 * 8.0;
    let room = COLS - LABEL_COLS - 2;
    let mut y = top + 20.0;
    for &f in fields {
        let row = Rect::new(x + 2.0, y - 1.0, w - 4.0, 13.0);
        spots.push((row, f));
        if f == focus || cursor.is_some_and(|(mx, my)| row.holds(mx, my)) {
            c.color_rect(r, ui_color::HIGHLIGHT, row.x, row.y, row.w, row.h);
        }
        let lc = if f == focus {
            color::YELLOW
        } else {
            color::RED
        };
        c.text(r, x + 4.0, y, lc, label(f));
        let value: String = match f {
            Field::Password => "*".repeat(login.password.chars().count()),
            Field::Remember => if login.remember {
                "[x] keep password in this profile"
            } else {
                "[ ] keep password in this profile"
            }
            .into(),
            Field::Name => login.name.clone(),
            Field::Arena if login.arena.is_empty() && f != focus => "(any public arena)".into(),
            Field::Arena => login.arena.clone(),
            Field::Server => login.server.clone(),
            Field::Ship | Field::Zones => String::new(),
        };
        let caret = f == focus && f != Field::Remember;
        let mut shown: String = value
            .chars()
            .rev()
            .take(room - usize::from(caret))
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        if caret {
            shown.push('_');
        }
        c.text(r, value_x, y, color::WHITE, &shown);
        y += 14.0;
    }
    y += 6.0;
    if let Some(err) = &login.error {
        let shown: String = err.chars().take(COLS - 1).collect();
        c.text(r, x + 4.0, y, color::RED, &shown);
    }
    y += 16.0;
    c.separator(r, x, y, w);
    c.text_center(
        r,
        x + w / 2.0,
        y + 6.0,
        color::YELLOW,
        "Enter = Play   Tab = Next   Esc = Close",
    );
    c.text_center(
        r,
        x + w / 2.0,
        y + 20.0,
        color::YELLOW,
        "PgUp/PgDn = Switch   F6 = New   F8 = Delete",
    );
    spots
}
