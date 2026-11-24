//! Remembering the password: the start-up screen's side of [`super::profiles`].
//!
//! The password is kept in the client's own profiles file, per name and zone, and
//! nowhere else: the client never uses the operating system's password store, so
//! it can never make the system put up a prompt. **The file is plain text**
//! readable by whoever can read this user account's files (see
//! [`super::profiles`]); that is how Continuum keeps its profiles too.
//!
//! What happens, and when:
//!
//! * **With a name and a zone filled in and the password box empty**, [`Recall`]
//!   looks for that profile and puts its password in the box. It never writes
//!   over anything typed.
//! * **When the zone accepts the login** -- not before, so a mistyped password
//!   is never kept, and a refused one leaves the stored one alone -- [`settle`]
//!   saves the profile, with its password if Remember is ticked.
//! * **Unticking** deletes the profile's saved password at once.
//! * **Switching, adding and deleting** profiles is [`apply`], driven by the keys
//!   [`super::login::Login`] reads.

use std::time::{Duration, Instant};

use super::login::{Field, Login, ProfileCmd};
use super::profiles::Profiles;

/// How long the name and zone must stay as they are before the profile is
/// looked up, so that typing a name does not look up once per letter.
const SETTLE: Duration = Duration::from_millis(400);

/// The zone accepted the login made from `login`: record the profile, with its
/// password if Remember is ticked and one was typed. A password is never
/// replaced by an empty one.
pub fn settle(profiles: &Profiles, login: &Login) {
    if login.name.trim().is_empty() {
        return;
    }
    let result = if login.remember && !login.password.is_empty() {
        profiles.remember(&login.name, &login.server, &login.password)
    } else if login.remember {
        // Nothing typed (a blank password zone): keep what is stored.
        let kept = profiles
            .find(&login.name, &login.server)
            .map(|p| p.password)
            .unwrap_or_default();
        profiles.remember(&login.name, &login.server, &kept)
    } else {
        profiles.remember(&login.name, &login.server, "")
    };
    match result {
        Ok(()) => log::info!("saved the profile for {}", login.name.trim()),
        Err(e) => log::warn!("couldn't save the profile: {e}"),
    }
}

/// Unticking Remember: deletes the saved password of the profile, keeping the profile.
pub fn forget(profiles: &Profiles, name: &str, zone: &str) {
    if let Err(e) = profiles.forget_password(name, zone) {
        log::warn!("couldn't forget the saved password: {e}");
    }
}

/// Switches to, adds or deletes a profile as the player asked.
pub fn apply(cmd: ProfileCmd, profiles: &Profiles, login: &mut Login) {
    let all = profiles.list();
    let at = all.iter().position(|p| p.is(&login.name, &login.server));
    let target = match cmd {
        ProfileCmd::New => {
            login.name.clear();
            login.password.clear();
            login.remember = true;
            login.error = None;
            login.focus_on(Field::Name);
            return;
        }
        ProfileCmd::Next => at.map_or(0, |i| (i + 1) % all.len().max(1)),
        ProfileCmd::Prev => at.map_or(all.len().saturating_sub(1), |i| {
            (i + all.len() - 1) % all.len()
        }),
        ProfileCmd::Delete => {
            let Some(i) = at else {
                login.error =
                    Some("This profile is not saved yet, so there is nothing to delete.".into());
                return;
            };
            if let Err(e) = profiles.delete(&login.name, &login.server) {
                login.error = Some(format!("Couldn't delete the profile: {e}"));
                return;
            }
            let rest = profiles.list();
            if rest.is_empty() {
                apply(ProfileCmd::New, profiles, login);
            } else {
                fill(login, &rest[i.min(rest.len() - 1)]);
            }
            return;
        }
    };
    match all.get(target) {
        Some(p) => fill(login, p),
        None => {
            login.error =
                Some("No saved profiles yet: a profile is saved when a login works.".into());
        }
    }
}

fn fill(login: &mut Login, p: &super::profiles::Profile) {
    login.name.clone_from(&p.name);
    login.server.clone_from(&p.zone);
    login.password.clone_from(&p.password);
    login.remember = true;
    login.error = None;
}

/// Fills in a saved password on the start-up screen.
#[derive(Default)]
pub struct Recall {
    /// The name and zone as last seen, and since when.
    watching: Option<(String, Instant)>,
    /// The profile last looked up, so it is looked up once.
    asked: Option<String>,
    /// The profile whose saved password is in the box, put there by us rather
    /// than typed: if the name or zone then changes, it is taken out again.
    filled_for: Option<String>,
}

fn key(login: &Login) -> Option<String> {
    (!login.name.trim().is_empty() && login.server.contains(':')).then(|| {
        format!(
            "{}@{}",
            login.name.trim().to_lowercase(),
            login.server.trim()
        )
    })
}

impl Recall {
    /// One frame's worth: notices a new name or zone, looks the profile up once it
    /// has stopped changing, and fills the box.
    pub fn tick(&mut self, login: &mut Login, profiles: &Profiles, now: Instant) {
        let who = key(login);

        // A password we filled in belongs to the profile it was saved for.
        if let Some(filled) = &self.filled_for
            && who.as_ref() != Some(filled)
        {
            login.password.clear();
            self.filled_for = None;
        }

        let Some(who) = who else {
            self.watching = None;
            return;
        };
        let since = match &self.watching {
            Some((w, since)) if *w == who => *since,
            _ => {
                self.watching = Some((who, now));
                return;
            }
        };
        if now.duration_since(since) < SETTLE
            || self.asked.as_ref() == Some(&who)
            || !login.password.is_empty()
        {
            return;
        }
        self.asked = Some(who.clone());
        if let Some(p) = profiles.find(&login.name, &login.server)
            && !p.password.is_empty()
        {
            login.password = p.password;
            login.remember = true;
            self.filled_for = Some(who);
        }
    }

    /// A profile was just switched to: its password counts as filled in by us,
    /// so editing the name takes it out again rather than keeping it for the
    /// wrong profile.
    pub fn adopt(&mut self, login: &Login) {
        self.asked = key(login);
        self.filled_for = key(login).filter(|_| !login.password.is_empty());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &str = "s3cret-for-tests";
    const ZONE: &str = "zone.example:5000";

    fn form() -> Login {
        let mut l = Login::default();
        l.name = "Pilot".into();
        l.server = ZONE.into();
        l.remember = true;
        l
    }

    fn profiles(tag: &str) -> (Profiles, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("oss-remember-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        (Profiles::new(Some(dir.join("profiles.txt"))), dir)
    }

    /// `settle` is only ever called once the zone has accepted, so a refused
    /// login -- a mistyped password -- is never kept, and does not disturb the
    /// stored one.
    #[test]
    fn the_zone_accepting_is_what_saves_it() {
        let (p, dir) = profiles("accept");
        let mut l = form();
        l.password = SECRET.into();
        assert!(p.list().is_empty(), "typing and trying alone saves nothing");
        settle(&p, &l);
        assert_eq!(p.find("Pilot", ZONE).unwrap().password, SECRET);

        // A later refused attempt never reaches `settle`: the stored one stays.
        l.password = "mistyped".into();
        assert_eq!(p.find("Pilot", ZONE).unwrap().password, SECRET);

        // Unticked and accepted: the profile stays, the password goes.
        l.remember = false;
        settle(&p, &l);
        let kept = p.find("Pilot", ZONE).unwrap();
        assert_eq!(kept.password, "");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_empty_password_never_replaces_a_kept_one() {
        let (p, dir) = profiles("empty");
        let mut l = form();
        l.password = SECRET.into();
        settle(&p, &l);
        l.password.clear();
        settle(&p, &l);
        assert_eq!(p.find("Pilot", ZONE).unwrap().password, SECRET);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unticking_forgets_the_password_at_once() {
        let (p, dir) = profiles("untick");
        p.remember("Pilot", ZONE, SECRET).unwrap();
        let mut l = form();
        l.set_vault(true);
        l.toggle_remember();
        assert!(!l.remember);
        let asked = l.take_forget().expect("asked to forget");
        forget(&p, &asked.0, &asked.1);
        assert_eq!(p.find("Pilot", ZONE).unwrap().password, "");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A saved password fills an empty box; it never writes over a password
    /// being typed; and it leaves again if the name changes.
    #[test]
    fn a_saved_password_fills_an_empty_box_only() {
        let (p, dir) = profiles("fill");
        p.remember("Pilot", ZONE, SECRET).unwrap();

        let mut l = form();
        let mut recall = Recall::default();
        let start = Instant::now();
        recall.tick(&mut l, &p, start);
        recall.tick(&mut l, &p, start + SETTLE);
        assert_eq!(l.password, SECRET);
        assert!(l.remember);

        // The name changes: that password was someone else's.
        l.name = "Another".into();
        recall.tick(&mut l, &p, start + SETTLE * 2);
        assert!(l.password.is_empty());

        // Typed first: left alone.
        let mut l = form();
        l.password = "typed".into();
        let mut recall = Recall::default();
        for i in 1..5u32 {
            recall.tick(&mut l, &p, start + SETTLE * i);
        }
        assert_eq!(l.password, "typed");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn switching_adding_and_deleting_profiles() {
        let (p, dir) = profiles("switch");
        p.remember("Ace", ZONE, "one").unwrap();
        p.remember("Bee", ZONE, "two").unwrap();
        // Most recent first: Bee, Ace.
        let mut l = form();
        l.name = "Bee".into();
        apply(ProfileCmd::Next, &p, &mut l);
        assert_eq!((l.name.as_str(), l.password.as_str()), ("Ace", "one"));
        apply(ProfileCmd::Next, &p, &mut l);
        assert_eq!((l.name.as_str(), l.password.as_str()), ("Bee", "two"));
        apply(ProfileCmd::Prev, &p, &mut l);
        assert_eq!(l.name, "Ace");

        apply(ProfileCmd::New, &p, &mut l);
        assert!(l.name.is_empty() && l.password.is_empty());
        assert_eq!(l.focused(), Field::Name);
        // From a blank form, Next starts at the front and Prev at the end.
        apply(ProfileCmd::Next, &p, &mut l);
        assert_eq!(l.name, "Bee");
        apply(ProfileCmd::New, &p, &mut l);
        apply(ProfileCmd::Prev, &p, &mut l);
        assert_eq!(l.name, "Ace");

        apply(ProfileCmd::Delete, &p, &mut l);
        assert!(p.find("Ace", ZONE).is_none());
        assert_eq!(l.name, "Bee", "falls to a neighbour");
        apply(ProfileCmd::Delete, &p, &mut l);
        assert!(p.list().is_empty());
        assert!(l.name.is_empty(), "nothing left: a blank profile");
        apply(ProfileCmd::Next, &p, &mut l);
        assert!(l.error.is_some(), "says there is nothing to switch to");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
