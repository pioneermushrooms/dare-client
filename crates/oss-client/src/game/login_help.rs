//! Plain words for a refused login on the Foundation zone, and the page that fixes it.
//!
//! The session crate words each refusal code for any zone. A zone that gives every code
//! its own meaning can be told more plainly, with the next step. This module does that for
//! the Foundation zone only, which is recognised by the address the player connected to;
//! every other zone keeps the generic wording and never shows these links.
//!
//! Only the codes that zone sends are reworded. The zone sends: "no permission" for a name
//! that has no account, "bad password", "locked out" after too many wrong passwords and
//! "server busy" when it cannot check a login right now.

use oss_proto::s2c::login_code;

const SIGN_UP: &str = "https://subspacefoundation.com/account/signup";
const FORGOT: &str = "https://subspacefoundation.com/account/forgot";

/// Addresses of the Foundation zone: its site's domain and its game server.
const HOSTS: [&str; 2] = ["subspacefoundation.com", "5.161.238.245"];

/// A page the login screen can open for the player.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Link {
    /// The address that is opened.
    pub url: &'static str,
    /// What the button says.
    pub label: &'static str,
    /// The same on a foot line, with its key: "F4 = Register".
    pub key: &'static str,
}

/// Whether `server` (`host` or `host:port`) is the Foundation zone.
pub fn is_foundation(server: &str) -> bool {
    let host = server.trim();
    let host = host.rsplit_once(':').map_or(host, |(h, _)| h);
    HOSTS.iter().any(|h| {
        host.eq_ignore_ascii_case(h)
            || host
                .to_ascii_lowercase()
                .strip_suffix(h)
                .is_some_and(|rest| rest.ends_with('.'))
    })
}

/// The words for a refusal and, if a page helps, the button for it. `None` when
/// the generic wording should stand: another zone, or a code the Foundation zone
/// has no plainer words for.
pub fn explain(server: &str, code: u8) -> Option<(&'static str, Option<Link>)> {
    if !is_foundation(server) {
        return None;
    }
    match code {
        login_code::NO_PERMISSION => Some((
            "This name isn't registered. Register at subspacefoundation.com/account/signup",
            Some(Link {
                url: SIGN_UP,
                label: "Register",
                key: "F4 = Register",
            }),
        )),
        login_code::BAD_PASSWORD => Some((
            "Wrong password. Forgot it? subspacefoundation.com/account/forgot",
            Some(Link {
                url: FORGOT,
                label: "Reset password",
                key: "F4 = Reset password",
            }),
        )),
        login_code::LOCKED => Some(("Too many tries. Wait 10 minutes, then try again.", None)),
        login_code::SERVER_BUSY => Some(("Login is unavailable right now. Try again soon.", None)),
        _ => None,
    }
}

/// Opens `url` in the player's browser. Only the fixed addresses above are passed,
/// never anything that came from the network.
pub fn open_in_browser(url: &'static str) {
    #[cfg(target_os = "macos")]
    let spawned = std::process::Command::new("open").arg(url).spawn();
    #[cfg(target_os = "windows")]
    let spawned = std::process::Command::new("rundll32")
        .args(["url.dll,FileProtocolHandler", url])
        .spawn();
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let spawned = std::process::Command::new("xdg-open").arg(url).spawn();
    if let Err(e) = spawned {
        log::warn!("couldn't open {url}: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_only_the_foundation_zone() {
        assert!(is_foundation("5.161.238.245:5000"));
        assert!(is_foundation("play.subspacefoundation.com:5000"));
        assert!(is_foundation("SubspaceFoundation.com"));
        assert!(!is_foundation("notsubspacefoundation.com:5000"));
        assert!(!is_foundation("subspacefoundation.com.evil.example:5000"));
        assert!(!is_foundation("5.161.238.2450:5000"));
        assert!(!is_foundation("sscentral.trenchwars.org:5000"));
        assert!(!is_foundation(""));
    }

    #[test]
    fn other_zones_keep_the_generic_wording() {
        for code in 0u8..=0x13 {
            assert_eq!(explain("zone.example.org:5000", code), None, "{code:#04x}");
        }
    }

    #[test]
    fn foundation_refusals_say_what_to_do() {
        let host = "5.161.238.245:5000";
        let (text, link) = explain(host, login_code::NO_PERMISSION).unwrap();
        assert!(text.contains("isn't registered") && text.contains("account/signup"));
        assert_eq!(
            link.unwrap().url,
            "https://subspacefoundation.com/account/signup"
        );

        let (text, link) = explain(host, login_code::BAD_PASSWORD).unwrap();
        assert!(text.starts_with("Wrong password") && text.contains("account/forgot"));
        assert_eq!(
            link.unwrap().url,
            "https://subspacefoundation.com/account/forgot"
        );

        let (text, link) = explain(host, login_code::LOCKED).unwrap();
        assert!(text.contains("10 minutes"));
        assert_eq!(link, None);
    }

    #[test]
    fn codes_the_zone_does_not_use_stay_generic() {
        for code in [
            login_code::NO_PERMISSION2,
            login_code::FULL,
            login_code::BAD_NAME,
        ] {
            assert_eq!(explain("5.161.238.245:5000", code), None, "{code:#04x}");
        }
    }
}
