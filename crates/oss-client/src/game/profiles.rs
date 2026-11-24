//! Profiles: the names a player logs in with, and the password last used with each.
//!
//! A profile is a name on a zone, with the password that worked there last time.
//! They live in one small text file in the client's own settings folder
//! (`profiles.txt`), most recently used first. The client never uses the
//! operating system's password store, so nothing here can make the system put up
//! a prompt.
//!
//! **The passwords are stored as plain text.** The file is created readable by its
//! owner only (mode 0600 on Unix; on Windows it inherits the settings folder's
//! permissions), but anyone who can read that user account's files can read it,
//! and obfuscating it would not change that, so it is not obfuscated. This is how
//! Continuum keeps its profiles too.
//!
//! A password is written only after the zone has accepted the login (see
//! [`super::remember`]), so a mistyped one is never kept.
//!
//! File format, one profile per line, fields separated by a tab:
//! `name <TAB> zone <TAB> password`. A name, zone or password with a tab or a
//! line break in it is refused rather than saved, since it could not be read back.

use std::path::{Path, PathBuf};

/// One profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    pub name: String,
    /// `host:port`.
    pub zone: String,
    /// Empty when the player chose not to keep it.
    pub password: String,
}

impl Profile {
    /// Whether this is the profile `name` on `zone`: the name without regard to
    /// case, as zones do, and the zone address exactly.
    pub fn is(&self, name: &str, zone: &str) -> bool {
        self.name.trim().eq_ignore_ascii_case(name.trim()) && self.zone.trim() == zone.trim()
    }
}

/// The profiles file. Without a path (no settings folder) it keeps nothing.
#[derive(Debug, Clone, Default)]
pub struct Profiles {
    path: Option<PathBuf>,
}

fn clean(s: &str) -> bool {
    !s.chars().any(|c| c == '\t' || c == '\n' || c == '\r')
}

impl Profiles {
    pub fn new(path: Option<PathBuf>) -> Self {
        Self { path }
    }

    #[cfg(test)]
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Every profile, most recently used first. A missing or unreadable file is
    /// no profiles; a line that does not parse is skipped.
    pub fn list(&self) -> Vec<Profile> {
        let Some(text) = self.path.as_deref().and_then(|p| std::fs::read(p).ok()) else {
            return Vec::new();
        };
        String::from_utf8_lossy(&text)
            .lines()
            .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
            .filter_map(|l| {
                let mut f = l.splitn(3, '\t');
                let (name, zone, password) = (f.next()?, f.next()?, f.next()?);
                (!name.trim().is_empty()).then(|| Profile {
                    name: name.to_string(),
                    zone: zone.to_string(),
                    password: password.to_string(),
                })
            })
            .collect()
    }

    /// The profile `name` on `zone`, if there is one.
    pub fn find(&self, name: &str, zone: &str) -> Option<Profile> {
        self.list().into_iter().find(|p| p.is(name, zone))
    }

    /// Records a login that the zone accepted: the profile moves to the front,
    /// with `password` (empty to keep none), and the name as just typed.
    pub fn remember(&self, name: &str, zone: &str, password: &str) -> Result<(), String> {
        let name = name.trim();
        let zone = zone.trim();
        if name.is_empty() {
            return Err("a profile needs a name".into());
        }
        if ![name, zone, password].iter().all(|s| clean(s)) {
            return Err("tabs and line breaks cannot be kept in a profile".into());
        }
        let mut all: Vec<Profile> = self
            .list()
            .into_iter()
            .filter(|p| !p.is(name, zone))
            .collect();
        all.insert(
            0,
            Profile {
                name: name.into(),
                zone: zone.into(),
                password: password.into(),
            },
        );
        self.write(&all)
    }

    /// Deletes only the saved password of a profile, keeping the profile.
    pub fn forget_password(&self, name: &str, zone: &str) -> Result<(), String> {
        let mut all = self.list();
        let Some(p) = all.iter_mut().find(|p| p.is(name, zone)) else {
            return Ok(());
        };
        if p.password.is_empty() {
            return Ok(());
        }
        p.password.clear();
        self.write(&all)
    }

    /// Deletes a profile altogether.
    pub fn delete(&self, name: &str, zone: &str) -> Result<(), String> {
        let all = self.list();
        let kept: Vec<Profile> = all.iter().filter(|p| !p.is(name, zone)).cloned().collect();
        if kept.len() == all.len() {
            return Ok(());
        }
        self.write(&kept)
    }

    /// Writes the whole file to a new one next to it, then renames it into
    /// place, so a crash never leaves half a file and a reader never sees one.
    fn write(&self, all: &[Profile]) -> Result<(), String> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let mut text = String::from(
            "# Dare profiles: name, zone, password, separated by tabs.\n\
             # Passwords are plain text: anyone who can read this file can read them.\n",
        );
        for p in all {
            text.push_str(&format!("{}\t{}\t{}\n", p.name, p.zone, p.password));
        }
        write_private(path, text.as_bytes()).map_err(|e| format!("{}: {e}", path.display()))
    }
}

/// Writes `data` to `path` so that only its owner can read it, replacing any file
/// there. The new file is private from the moment it exists.
fn write_private(path: &Path, data: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    let _ = std::fs::remove_file(&tmp);
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts.open(&tmp)?;
    f.write_all(data)?;
    f.sync_all()?;
    drop(f);
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ZONE: &str = "zone.example:5000";

    fn store(tag: &str) -> (Profiles, PathBuf) {
        let dir = std::env::temp_dir().join(format!("oss-profiles-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        (Profiles::new(Some(dir.join("profiles.txt"))), dir)
    }

    #[test]
    fn saves_loads_and_keeps_several_most_recent_first() {
        let (p, dir) = store("several");
        assert!(p.list().is_empty(), "no file, no profiles");
        p.remember("Ace", ZONE, "one").unwrap();
        p.remember("Bee", ZONE, "two").unwrap();
        p.remember("Ace", "other.example:5000", "three").unwrap();
        let names: Vec<_> = p
            .list()
            .iter()
            .map(|p| (p.name.clone(), p.zone.clone()))
            .collect();
        assert_eq!(
            names,
            [
                ("Ace".to_string(), "other.example:5000".to_string()),
                ("Bee".to_string(), ZONE.to_string()),
                ("Ace".to_string(), ZONE.to_string()),
            ]
        );
        assert_eq!(
            p.find("ace", ZONE).unwrap().password,
            "one",
            "name case does not matter"
        );
        // Logging in again with another password replaces it and moves it up.
        p.remember("ACE", ZONE, "changed").unwrap();
        assert_eq!(p.list()[0].password, "changed");
        assert_eq!(p.list()[0].name, "ACE");
        assert_eq!(p.list().len(), 3);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn forgetting_a_password_keeps_the_profile_and_delete_removes_it() {
        let (p, dir) = store("forget");
        p.remember("Ace", ZONE, "one").unwrap();
        p.remember("Bee", ZONE, "two").unwrap();
        p.forget_password("ace", ZONE).unwrap();
        assert_eq!(p.find("Ace", ZONE).unwrap().password, "");
        assert_eq!(p.find("Bee", ZONE).unwrap().password, "two");
        p.delete("Ace", ZONE).unwrap();
        assert!(p.find("Ace", ZONE).is_none());
        assert_eq!(p.list().len(), 1);
        p.delete("Nobody", ZONE).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn refuses_what_could_not_be_read_back() {
        let (p, dir) = store("refuse");
        assert!(p.remember("", ZONE, "x").is_err());
        assert!(p.remember("A\tce", ZONE, "x").is_err());
        assert!(p.remember("Ace", ZONE, "pa\nss").is_err());
        assert!(p.list().is_empty());
        // Odd but legal characters round-trip.
        p.remember("Ace", ZONE, "p=a ss#é\"'").unwrap();
        assert_eq!(p.find("Ace", ZONE).unwrap().password, "p=a ss#é\"'");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_damaged_file_is_not_a_crash() {
        let (p, dir) = store("damaged");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            p.path().unwrap(),
            b"junk line\n\xff\xfe\n\tonly\tthree\nok\tz:1\tpw\n",
        )
        .unwrap();
        let all = p.list();
        assert!(all.iter().any(|x| x.name == "ok" && x.password == "pw"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn the_file_is_private_to_its_owner_even_after_a_rewrite() {
        use std::os::unix::fs::PermissionsExt;
        let (p, dir) = store("perm");
        p.remember("Ace", ZONE, "one").unwrap();
        let mode = |p: &Profiles| {
            std::fs::metadata(p.path().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777
        };
        assert_eq!(mode(&p), 0o600);
        p.remember("Bee", ZONE, "two").unwrap();
        assert_eq!(mode(&p), 0o600);
        // A looser file left by something else is replaced by a private one.
        std::fs::set_permissions(p.path().unwrap(), std::fs::Permissions::from_mode(0o644))
            .unwrap();
        p.remember("Cee", ZONE, "three").unwrap();
        assert_eq!(mode(&p), 0o600);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn without_a_settings_folder_nothing_is_kept_and_nothing_fails() {
        let p = Profiles::new(None);
        p.remember("Ace", ZONE, "one").unwrap();
        assert!(p.list().is_empty());
    }
}
