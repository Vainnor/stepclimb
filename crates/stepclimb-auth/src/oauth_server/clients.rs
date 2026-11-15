// SPDX-License-Identifier: AGPL-3.0-or-later
//! The registry of OAuth 2.1 clients (docs/specs/m0/08-sessions-and-csrf.md B30, Q10 (a)).
//!
//! Every client is public (no secret: an app on a user's device cannot keep one) and is one of
//! Stepclimb's own apps, listed here in code. A client names the redirect URIs it may use, and a
//! presented `redirect_uri` must equal one of them character for character, with one exception
//! that RFC 8252 section 7.3 and OAuth 2.1 require for native apps: a loopback redirect URI
//! (`http://127.0.0.1:<port>/...` or `http://[::1]:<port>/...`) may carry any port, since the
//! desktop app listens on whatever port the operating system gives it. The scheme, host and path
//! are still compared exactly, and a loopback URI must carry a port and no query or fragment.
//! No other normalization happens: `HTTP://127.0.0.1:...`, `localhost` or a trailing `/` is a
//! mismatch.

/// One redirect URI a client registered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Redirect {
    /// A URI matched character for character, such as the app's private-use scheme
    /// (RFC 8252 section 7.1).
    Exact(&'static str),
    /// A loopback URI (RFC 8252 section 7.3): `origin` is `http://127.0.0.1` or `http://[::1]`
    /// and `path` the exact path; any port from 1 to 65535 matches.
    Loopback {
        origin: &'static str,
        path: &'static str,
    },
}

impl Redirect {
    /// Whether `presented` is this registered URI.
    #[must_use]
    pub fn accepts(&self, presented: &str) -> bool {
        match *self {
            Self::Exact(uri) => presented == uri,
            Self::Loopback { origin, path } => presented
                .strip_prefix(origin)
                .and_then(|rest| rest.strip_prefix(':'))
                .and_then(|rest| rest.split_once('/'))
                .is_some_and(|(port, tail)| is_port(port) && tail == &path[1..]),
        }
    }
}

/// A decimal port from 1 to 65535 without a leading zero, as a URL serializer writes one.
fn is_port(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 5
        && text.bytes().all(|byte| byte.is_ascii_digit())
        && !text.starts_with('0')
        && text.parse::<u32>().is_ok_and(|port| port <= 65_535)
}

/// A public client of the OAuth 2.1 server.
#[derive(Debug, PartialEq, Eq)]
pub struct Client {
    /// The `client_id` the app sends.
    pub id: &'static str,
    /// The name the confirmation page shows the user.
    pub name: &'static str,
    /// The redirect URIs the client may name.
    pub redirects: &'static [Redirect],
}

impl Client {
    /// Whether `presented` is one of the client's redirect URIs.
    #[must_use]
    pub fn accepts_redirect(&self, presented: &str) -> bool {
        self.redirects
            .iter()
            .any(|redirect| redirect.accepts(presented))
    }
}

/// The path every redirect URI of the Stepclimb app ends in.
const APP_CALLBACK_PATH: &str = "/oauth/callback";

/// The Stepclimb app (`apps/app`): its deep link on the private-use scheme of its bundle
/// identifier `dev.stepclimb.shell` (RFC 8252 section 7.1, one `/` after the scheme), and a
/// loopback listener on either address family for desktop.
pub const STEPCLIMB_APP: Client = Client {
    id: "stepclimb-app",
    name: "the Stepclimb app",
    redirects: &[
        Redirect::Exact("dev.stepclimb.shell:/oauth/callback"),
        Redirect::Loopback {
            origin: "http://127.0.0.1",
            path: APP_CALLBACK_PATH,
        },
        Redirect::Loopback {
            origin: "http://[::1]",
            path: APP_CALLBACK_PATH,
        },
    ],
};

/// Every client the server accepts.
pub const ALL: &[Client] = &[STEPCLIMB_APP];

/// The client whose `client_id` is `id`, compared exactly.
#[must_use]
pub fn find(id: &str) -> Option<&'static Client> {
    ALL.iter().find(|client| client.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_client_is_found_by_its_exact_id() {
        assert_eq!(find("stepclimb-app"), Some(&STEPCLIMB_APP));
        for other in [
            "",
            "Stepclimb-app",
            "stepclimb-app ",
            "stepclimb-ap",
            "stepclimb-appx",
        ] {
            assert_eq!(find(other), None, "{other:?}");
        }
    }

    #[test]
    fn the_deep_link_matches_only_character_for_character() {
        let app = &STEPCLIMB_APP;
        assert!(app.accepts_redirect("dev.stepclimb.shell:/oauth/callback"));
        for other in [
            "dev.stepclimb.shell:/oauth/callbacK",
            "dev.stepclimb.shell:/oauth/callback/",
            "dev.stepclimb.shell:/oauth/callback?x=1",
            "dev.stepclimb.shell:/oauth/callback#x",
            "dev.stepclimb.shell://oauth/callback",
            "DEV.stepclimb.shell:/oauth/callback",
            "dev.stepclimb.shel:/oauth/callback",
            "dev.stepclimb.shell:/oauth/%63allback",
            " dev.stepclimb.shell:/oauth/callback",
        ] {
            assert!(!app.accepts_redirect(other), "{other:?}");
        }
    }

    #[test]
    fn a_loopback_uri_matches_with_any_port() {
        let app = &STEPCLIMB_APP;
        for uri in [
            "http://127.0.0.1:1/oauth/callback",
            "http://127.0.0.1:49152/oauth/callback",
            "http://127.0.0.1:65535/oauth/callback",
            "http://[::1]:8080/oauth/callback",
        ] {
            assert!(app.accepts_redirect(uri), "{uri:?}");
        }
    }

    #[test]
    fn a_loopback_uri_differing_in_anything_but_the_port_is_refused() {
        let app = &STEPCLIMB_APP;
        for other in [
            "http://127.0.0.1/oauth/callback",
            "http://127.0.0.1:/oauth/callback",
            "http://127.0.0.1:0/oauth/callback",
            "http://127.0.0.1:080/oauth/callback",
            "http://127.0.0.1:65536/oauth/callback",
            "http://127.0.0.1:123456/oauth/callback",
            "http://127.0.0.1:+80/oauth/callback",
            "http://127.0.0.1:8080/oauth/callback/",
            "http://127.0.0.1:8080/oauth/callback?x=1",
            "http://127.0.0.1:8080/oauth/callback#x",
            "http://127.0.0.1:8080/oauth/callbac",
            "http://127.0.0.1:8080//oauth/callback",
            "http://127.0.0.1:8080@evil.example/oauth/callback",
            "http://127.0.0.1.evil.example:8080/oauth/callback",
            "https://127.0.0.1:8080/oauth/callback",
            "HTTP://127.0.0.1:8080/oauth/callback",
            "http://localhost:8080/oauth/callback",
            "http://127.0.0.2:8080/oauth/callback",
            "http://[::1]/oauth/callback",
            "http://[0:0:0:0:0:0:0:1]:8080/oauth/callback",
        ] {
            assert!(!app.accepts_redirect(other), "{other:?}");
        }
    }

    #[test]
    fn every_client_has_an_id_a_name_and_a_redirect() {
        for client in ALL {
            assert!(!client.id.is_empty() && !client.name.is_empty());
            assert!(!client.redirects.is_empty());
            for redirect in client.redirects {
                if let Redirect::Loopback { path, .. } = redirect {
                    assert!(path.starts_with('/'), "{path}");
                }
            }
        }
    }
}
