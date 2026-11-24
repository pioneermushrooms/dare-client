//! Remembering the password: the start-up screen's side of `oss-credstore`.
//!
//! Every call to a password store is made away from the thread that draws. On
//! Windows and macOS they are quick; on Linux each one is a question put to the
//! desktop's keyring over its message bus, which can be slow, or can put a
//! prompt up and wait for the player. A screen that waited with it would freeze.
//!
//! What happens, and when:
//!
//! * **At start**, [`Vault::probe`] asks whether this machine has a store. Until
//!   it answers, and for good if it says no, the form has no Remember line.
//! * **With a name and a zone filled in and the password box empty**,
//!   [`Recall`] asks the store for a saved password, and puts it in the box and
//!   ticks Remember if there is one. It never writes over anything typed.
//! * **When the zone accepts the login** -- not before, so a mistyped password
//!   is never kept -- [`settle`] saves the password if Remember is ticked, and
//!   deletes any saved one if it is not.
//! * **Unticking** deletes the saved password at once.
//!
//! **The store is only touched for a password the player asked to keep.** Reading
//! or deleting an entry can make the operating system put up its own prompt (on
//! a Mac, one asking for the Mac's login password), so the client keeps a short
//! [`Ledger`] of the accounts it has saved a password for, in a plain file beside
//! the login file, and asks the store about those and no others. A player who has
//! never ticked Remember never sees the prompt, and the first time they do, the
//! screen says what the prompt will be before it appears.
//!
//! Log lines name the entry (`pilot@zone:5000`) and never the password.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::Sender;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use oss_credstore::{OsStore, PasswordStore, account};

use super::login::Login;

/// How long the store has to say whether it is there before the client stops
/// waiting and carries on without it.
const PROBE_DEADLINE: Duration = Duration::from_secs(5);
/// How long the name and zone must stay as they are before the store is asked
/// about them, so that typing a name does not ask once per letter.
const SETTLE: Duration = Duration::from_millis(400);

/// This machine's password store, once it has said whether there is one.
pub enum Vault {
    /// Still asking, since `started`.
    Asking(Receiver<Option<Arc<dyn PasswordStore>>>, Instant),
    Ready(Arc<dyn PasswordStore>),
    /// No store, or none answered in time.
    None,
}

impl Vault {
    /// Asks, on a thread of its own, whether this machine has a password store.
    pub fn probe() -> Self {
        let (tx, rx) = mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("password-store".into())
            .spawn(move || {
                // A store that opens is offered even if it turns out to be
                // locked when a password is read or saved: the store asks the
                // player then, and saying no is their choice, not a missing
                // feature. One that will not even open is not offered.
                let store: Option<Arc<dyn PasswordStore>> = match OsStore::open() {
                    Ok(Some(s)) => Some(Arc::new(s)),
                    Ok(None) => None,
                    Err(e) => {
                        log::info!("password store: {e}");
                        None
                    }
                };
                let _ = tx.send(store);
            });
        match spawned {
            Ok(_) => Vault::Asking(rx, Instant::now()),
            Err(e) => {
                log::warn!("couldn't ask for a password store: {e}");
                Vault::None
            }
        }
    }

    /// Takes the answer, if it has come, or gives up once the deadline passes.
    pub fn poll(&mut self) {
        let Vault::Asking(rx, started) = self else {
            return;
        };
        *self = match rx.try_recv() {
            Ok(Some(store)) => {
                log::info!("password store found: Remember password is offered");
                Vault::Ready(store)
            }
            Ok(None) | Err(TryRecvError::Disconnected) => Vault::None,
            Err(TryRecvError::Empty) if started.elapsed() > PROBE_DEADLINE => {
                log::info!("the password store did not answer in time; not offering to remember");
                Vault::None
            }
            Err(TryRecvError::Empty) => return,
        };
    }

    pub fn store(&self) -> Option<Arc<dyn PasswordStore>> {
        match self {
            Vault::Ready(s) => Some(Arc::clone(s)),
            _ => None,
        }
    }
}

/// Which accounts have a saved password, and whether the player has been told
/// what the system's prompt will be. Names and zones only, never a password.
/// Kept in a small text file; without a path it lives for the run only.
#[derive(Debug, Default)]
pub struct Ledger {
    path: Option<PathBuf>,
    accounts: BTreeSet<String>,
    notice_seen: bool,
}

/// The line that records the notice has been shown. An account line always has
/// an `@`, so the two cannot be mistaken.
const NOTICE_LINE: &str = "#notice-shown";

impl Ledger {
    pub fn load(path: Option<PathBuf>) -> Self {
        let mut l = Ledger {
            path,
            ..Ledger::default()
        };
        if let Some(text) = l
            .path
            .as_deref()
            .and_then(|p| std::fs::read_to_string(p).ok())
        {
            for line in text.lines().map(str::trim).filter(|s| !s.is_empty()) {
                if line == NOTICE_LINE {
                    l.notice_seen = true;
                } else if line.contains('@') {
                    l.accounts.insert(line.to_string());
                }
            }
        }
        l
    }

    /// Whether a password was saved for this account (see [`account`]).
    pub fn has(&self, who: &str) -> bool {
        self.accounts.contains(who)
    }

    pub fn add(&mut self, who: &str) {
        if self.accounts.insert(who.to_string()) {
            self.write();
        }
    }

    pub fn remove(&mut self, who: &str) {
        if self.accounts.remove(who) {
            self.write();
        }
    }

    /// Whether the player has been told what the system's prompt is.
    pub fn notice_seen(&self) -> bool {
        self.notice_seen
    }

    pub fn mark_notice_seen(&mut self) {
        if !self.notice_seen {
            self.notice_seen = true;
            self.write();
        }
    }

    fn write(&self) {
        let Some(path) = &self.path else { return };
        let mut text = String::new();
        if self.notice_seen {
            text.push_str(NOTICE_LINE);
            text.push('\n');
        }
        for a in &self.accounts {
            text.push_str(a);
            text.push('\n');
        }
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Err(e) = std::fs::write(path, text) {
            log::warn!("couldn't write {}: {e}", path.display());
        }
    }
}

/// What to tell a player before the system asks them anything about the saved
/// password, or `None` where the store asks nothing worth explaining.
pub fn keychain_notice() -> Option<&'static str> {
    cfg!(target_os = "macos").then_some(
        "Dare keeps your Foundation password in your Mac's Keychain. macOS may now ask \
         for your Mac login password: that is your Mac's password, not your Foundation \
         one, and it only lets Dare use the Keychain. Choose \"Always Allow\" so it \
         does not ask again.",
    )
}

/// A job for the store, run off the drawing thread.
type Job = Box<dyn FnOnce() + Send>;

/// The one thread every store job runs on, in the order they were asked for.
///
/// One queue rather than a thread per job, because order matters: untick
/// (delete), then tick again and play (save), must not end with the delete
/// arriving last and the password gone. On Linux, where each call is a question
/// to the desktop's keyring that may take a while, two threads could finish
/// either way round.
fn queue() -> Option<&'static Mutex<Sender<Job>>> {
    static QUEUE: OnceLock<Option<Mutex<Sender<Job>>>> = OnceLock::new();
    QUEUE
        .get_or_init(|| {
            let (tx, rx) = mpsc::channel::<Job>();
            std::thread::Builder::new()
                .name("password-store".into())
                .spawn(move || {
                    while let Ok(job) = rx.recv() {
                        job();
                    }
                })
                .map_err(|e| log::warn!("couldn't start the password store's thread: {e}"))
                .ok()?;
            Some(Mutex::new(tx))
        })
        .as_ref()
}

/// Runs `job` with the store on the store's own thread, after any job asked for
/// before it, so the screen never waits. The answer says when it has finished:
/// the screen ignores it, and a test waits on it.
fn in_background(
    store: &Arc<dyn PasswordStore>,
    job: impl FnOnce(&dyn PasswordStore) + Send + 'static,
) -> Receiver<()> {
    let (done, finished) = mpsc::channel();
    let store = Arc::clone(store);
    let job: Job = Box::new(move || {
        job(store.as_ref());
        let _ = done.send(());
    });
    match queue().map(|q| q.lock().map(|tx| tx.send(job))) {
        Some(Ok(Ok(()))) => {}
        _ => log::warn!("couldn't reach the password store"),
    }
    finished
}

/// Unticking Remember: deletes the saved password, if one was ever saved. A
/// player who never kept one never has the store touched.
pub fn forget_if_kept(
    store: &Arc<dyn PasswordStore>,
    ledger: &mut Ledger,
    who: String,
) -> Option<Receiver<()>> {
    if !ledger.has(&who) {
        return None;
    }
    ledger.remove(&who);
    Some(forget(store, who))
}

/// Deletes the saved password for `who`, as unticking Remember asks.
fn forget(store: &Arc<dyn PasswordStore>, who: String) -> Receiver<()> {
    in_background(store, move |s| match s.forget(&who) {
        Ok(()) => log::info!("forgot the saved password for {who}"),
        Err(e) => log::warn!("couldn't forget the saved password for {who}: {e}"),
    })
}

/// The zone has accepted the login made from `login`: keep the password if
/// Remember is ticked, and delete any kept one if it is not.
pub fn settle(
    store: &Arc<dyn PasswordStore>,
    ledger: &mut Ledger,
    login: &Login,
) -> Option<Receiver<()>> {
    if !login.vault_shown() || login.name.trim().is_empty() {
        return None;
    }
    let who = account(&login.name, &login.server);
    if login.remember && !login.password.is_empty() {
        let password = login.password.clone();
        ledger.add(&who);
        Some(in_background(store, move |s| {
            match s.save(&who, &password) {
                Ok(()) => log::info!("saved the password for {who}"),
                Err(e) => log::warn!("couldn't save the password for {who}: {e}"),
            }
        }))
    } else {
        forget_if_kept(store, ledger, who)
    }
}

/// Fills in a saved password on the start-up screen.
#[derive(Default)]
pub struct Recall {
    /// The account the name and zone add up to now, and since when.
    watching: Option<(String, Instant)>,
    /// The account last asked about, so it is asked once.
    asked: Option<String>,
    /// An answer on its way.
    pending: Option<(String, Receiver<Option<String>>)>,
    /// The account whose saved password is in the box, put there by us rather
    /// than typed: if the name or zone then changes, it is taken out again.
    filled_for: Option<String>,
}

impl Recall {
    /// One frame's worth: notices a new name or zone, asks once it has stopped
    /// changing, and fills the box when the answer comes.
    pub fn tick(
        &mut self,
        login: &mut Login,
        store: &Arc<dyn PasswordStore>,
        ledger: &Ledger,
        now: Instant,
    ) {
        let who = (!login.name.trim().is_empty() && login.server.contains(':'))
            .then(|| account(&login.name, &login.server));

        // A password we filled in belongs to the account it was saved for.
        if let Some(filled) = &self.filled_for
            && who.as_ref() != Some(filled)
        {
            login.password.clear();
            login.remember = false;
            self.filled_for = None;
        }

        if let Some((asked_for, rx)) = &self.pending {
            match rx.try_recv() {
                Ok(found) => {
                    if let Some(password) = found
                        && who.as_ref() == Some(asked_for)
                        && login.password.is_empty()
                    {
                        login.password = password;
                        login.remember = true;
                        self.filled_for = Some(asked_for.clone());
                    }
                    self.pending = None;
                }
                Err(TryRecvError::Disconnected) => self.pending = None,
                Err(TryRecvError::Empty) => {}
            }
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
            || self.pending.is_some()
            || !login.password.is_empty()
            // Nothing was ever kept for this account: do not ask the store, which
            // could put the system's prompt up for nothing.
            || !ledger.has(&who)
        {
            return;
        }
        self.asked = Some(who.clone());
        let (tx, rx) = mpsc::channel();
        let asking = who.clone();
        in_background(store, move |s| {
            let found = match s.load(&asking) {
                Ok(found) => found,
                Err(e) => {
                    log::info!("couldn't read a saved password for {asking}: {e}");
                    None
                }
            };
            let _ = tx.send(found);
        });
        self.pending = Some((who, rx));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oss_credstore::MemoryStore;

    const SECRET: &str = "s3cret-for-tests";

    fn form(store: bool) -> Login {
        let mut l = Login::default();
        l.name = "Pilot".into();
        l.server = "zone.example:5000".into();
        l.set_vault(store);
        l
    }

    /// Waits for the background threads, ticking as the screen would.
    fn tick_until(
        recall: &mut Recall,
        l: &mut Login,
        store: &Arc<dyn PasswordStore>,
        ledger: &Ledger,
        done: impl Fn(&Login) -> bool,
    ) {
        let start = Instant::now();
        let mut now = start;
        while !done(l) && start.elapsed() < Duration::from_secs(5) {
            now += SETTLE;
            recall.tick(l, store, ledger, now);
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Waits for a background job to finish.
    fn join(job: Option<Receiver<()>>) {
        job.expect("a job was asked for")
            .recv_timeout(Duration::from_secs(5))
            .expect("and finished");
    }

    fn kept(store: &MemoryStore, who: &str) -> Option<String> {
        store.load(who).expect("readable")
    }

    /// Ticked and accepted: saved. Unticked and accepted: deleted. And `settle`
    /// is only ever called once the zone has accepted, so a refused login --
    /// a mistyped password -- is never kept.
    #[test]
    fn the_zone_accepting_is_what_saves_it() {
        let mem = Arc::new(MemoryStore::default());
        let store: Arc<dyn PasswordStore> = mem.clone();
        let who = account("Pilot", "zone.example:5000");
        let mut ledger = Ledger::default();

        let mut l = form(true);
        l.password = SECRET.into();
        assert!(!l.remember, "unticked to begin with");
        assert!(
            settle(&store, &mut ledger, &l).is_none(),
            "nothing was kept, so nothing is deleted"
        );
        assert_eq!(kept(&mem, &who), None);

        l.remember = true;
        join(settle(&store, &mut ledger, &l));
        assert_eq!(kept(&mem, &who).as_deref(), Some(SECRET));

        l.remember = false;
        join(settle(&store, &mut ledger, &l));
        assert_eq!(kept(&mem, &who), None);
    }

    /// Unticking deletes the saved password at once, without waiting to play.
    #[test]
    fn unticking_forgets_at_once() {
        let mem = Arc::new(MemoryStore::default());
        let store: Arc<dyn PasswordStore> = mem.clone();
        let who = account("Pilot", "zone.example:5000");
        mem.save(&who, SECRET).expect("saved");
        let mut l = form(true);
        l.remember = true;
        l.toggle_remember();
        let asked = l.take_forget().expect("asked to forget");
        assert_eq!(asked, who);
        let mut ledger = Ledger::default();
        ledger.add(&who);
        join(forget_if_kept(&store, &mut ledger, asked));
        assert_eq!(kept(&mem, &who), None);
        assert!(!ledger.has(&who));
    }

    /// A saved password fills an empty box and ticks Remember; it never writes
    /// over a password being typed; and it leaves again if the name changes.
    #[test]
    fn a_saved_password_fills_an_empty_box_only() {
        let mem = Arc::new(MemoryStore::default());
        let store: Arc<dyn PasswordStore> = mem.clone();
        mem.save(&account("Pilot", "zone.example:5000"), SECRET)
            .expect("saved");
        let mut ledger = Ledger::default();
        ledger.add(&account("Pilot", "zone.example:5000"));

        let mut l = form(true);
        let mut recall = Recall::default();
        tick_until(&mut recall, &mut l, &store, &ledger, |l| {
            !l.password.is_empty()
        });
        assert_eq!(l.password, SECRET);
        assert!(l.remember);

        // The name changes: that password was someone else's.
        l.name = "Another".into();
        recall.tick(&mut l, &store, &ledger, Instant::now());
        assert!(l.password.is_empty());
        assert!(!l.remember);

        // Typed first: left alone.
        let mut l = form(true);
        l.password = "typed".into();
        let mut recall = Recall::default();
        let start = Instant::now();
        for i in 1..20u32 {
            recall.tick(&mut l, &store, &ledger, start + SETTLE * i);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(l.password, "typed");
    }

    /// No store, no line: the form is the six lines it always was, and Tab never
    /// lands on a Remember that is not drawn.
    #[test]
    fn no_store_no_line() {
        use super::super::login::Field;
        let mut l = form(false);
        assert!(!l.fields().contains(&Field::Remember));
        for _ in 0..20 {
            l.focus += 1;
            assert_ne!(l.focused(), Field::Remember);
        }
        l.toggle_remember();
        assert!(!l.remember, "nothing to tick");

        let l = form(true);
        let fields = l.fields();
        let at = fields.iter().position(|f| *f == Field::Remember);
        assert_eq!(at, Some(2), "under the password");
    }

    /// A store that counts how often it is touched.
    #[derive(Default)]
    struct Counting(std::sync::atomic::AtomicUsize);

    impl PasswordStore for Counting {
        fn save(&self, _: &str, _: &str) -> Result<(), oss_credstore::StoreError> {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        }
        fn load(&self, _: &str) -> Result<Option<String>, oss_credstore::StoreError> {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(None)
        }
        fn forget(&self, _: &str) -> Result<(), oss_credstore::StoreError> {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        }
    }

    /// A player who never asked to remember a password never has the store
    /// touched: no read while typing a name, no delete after a login.
    #[test]
    fn the_store_is_not_touched_unless_a_password_was_kept() {
        let counting = Arc::new(Counting::default());
        let store: Arc<dyn PasswordStore> = counting.clone();
        let mut ledger = Ledger::default();

        let mut l = form(true);
        let mut recall = Recall::default();
        let start = Instant::now();
        for i in 1..20u32 {
            recall.tick(&mut l, &store, &ledger, start + SETTLE * i);
            std::thread::sleep(Duration::from_millis(2));
        }
        l.password = SECRET.into();
        assert!(
            settle(&store, &mut ledger, &l).is_none(),
            "unticked: no delete"
        );
        l.remember = true;
        l.toggle_remember();
        let asked = l.take_forget().expect("asked to forget");
        assert!(forget_if_kept(&store, &mut ledger, asked).is_none());
        assert_eq!(counting.0.load(std::sync::atomic::Ordering::SeqCst), 0);

        // Ticked and accepted: now it is touched, once, and remembered as kept.
        l.remember = true;
        join(settle(&store, &mut ledger, &l));
        assert_eq!(counting.0.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert!(ledger.has(&account("Pilot", "zone.example:5000")));
    }

    /// The ledger survives a restart, holds names and never a password, and
    /// remembers that the notice was shown.
    #[test]
    fn the_ledger_round_trips_without_secrets() {
        let dir = std::env::temp_dir().join(format!("oss-ledger-{}", std::process::id()));
        let path = dir.join("remembered.txt");
        let mut a = Ledger::load(Some(path.clone()));
        assert!(!a.notice_seen());
        a.add("pilot@zone.example:5000");
        a.mark_notice_seen();
        let b = Ledger::load(Some(path.clone()));
        assert!(b.has("pilot@zone.example:5000"));
        assert!(b.notice_seen());
        let text = std::fs::read_to_string(&path).expect("written");
        assert!(!text.contains(SECRET));
        let mut b = b;
        b.remove("pilot@zone.example:5000");
        assert!(!Ledger::load(Some(path)).has("pilot@zone.example:5000"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_notice_says_whose_password_the_system_asks_for() {
        if let Some(text) = keychain_notice() {
            assert!(text.contains("Keychain"));
            assert!(text.contains("not your Foundation"));
            assert!(text.contains("Always Allow"));
        }
    }
}
