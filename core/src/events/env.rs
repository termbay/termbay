//! How `termbay-hook` finds the app (decision E1 in `docs/FASE2-PLANO.md`).
//!
//! The app puts three variables in the environment of every shell it opens.
//! `claude`, started in that shell, passes them on to its hooks. Without
//! them, `termbay-hook` does nothing, so `claude` outside termbay is not
//! affected and no discovery file can leak or go stale.

use std::ffi::OsString;
use std::fmt;
use std::net::SocketAddr;

/// Address of the event server, `127.0.0.1:<port>`.
pub const EVENTS_VAR: &str = "TERMBAY_EVENTS";
/// Session token of the event server.
pub const TOKEN_VAR: &str = "TERMBAY_TOKEN";
/// Id of the pty the shell runs in, so the office can tell which pane a
/// session belongs to.
pub const PANE_VAR: &str = "TERMBAY_PANE";
/// All of them. Shells never inherit these from termbay's own environment
/// (a termbay started inside another one must not point at the other app).
pub const HOOK_VARS: [&str; 3] = [EVENTS_VAR, TOKEN_VAR, PANE_VAR];

/// Length of a valid token: 32 bytes in hex.
const TOKEN_LEN: usize = 64;

/// Where to send events and with which token.
#[derive(Clone, PartialEq, Eq)]
pub struct HookEnv {
    addr: SocketAddr,
    token: String,
}

impl HookEnv {
    /// Address and token of a running event server.
    pub fn new(addr: SocketAddr, token: impl Into<String>) -> Self {
        Self {
            addr,
            token: token.into(),
        }
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    pub fn token(&self) -> &str {
        &self.token
    }

    /// The variables for a shell running in pty `pane`.
    pub fn vars(&self, pane: u64) -> Vec<(OsString, OsString)> {
        vec![
            (EVENTS_VAR.into(), self.addr.to_string().into()),
            (TOKEN_VAR.into(), self.token.clone().into()),
            (PANE_VAR.into(), pane.to_string().into()),
        ]
    }

    /// Reads the variables back, as `termbay-hook` does. `None` when any of
    /// address or token is missing or invalid: the address must be loopback
    /// (events never leave the machine, whatever the environment says) and
    /// the token 64 hex characters. The pane is optional.
    pub fn from_vars(get: impl Fn(&str) -> Option<String>) -> Option<(Self, Option<String>)> {
        let addr: SocketAddr = get(EVENTS_VAR)?.trim().parse().ok()?;
        if !addr.ip().is_loopback() || addr.port() == 0 {
            return None;
        }
        let token = get(TOKEN_VAR)?;
        if token.len() != TOKEN_LEN || !token.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let pane = get(PANE_VAR)
            .filter(|p| !p.is_empty() && p.len() <= 32 && p.bytes().all(|b| b.is_ascii_digit()));
        Some((Self { addr, token }, pane))
    }
}

/// Never prints the token.
impl fmt::Debug for HookEnv {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HookEnv")
            .field("addr", &self.addr)
            .field("token", &"<redacted>")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn lookup(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| ((*k).into(), (*v).into()))
            .collect();
        move |key| map.get(key).cloned()
    }

    #[test]
    fn vars_round_trip() {
        let env = HookEnv::new("127.0.0.1:4242".parse().unwrap(), TOKEN);
        let vars = env.vars(7);
        let pairs: Vec<(String, String)> = vars
            .iter()
            .map(|(k, v)| {
                (
                    k.to_string_lossy().into_owned(),
                    v.to_string_lossy().into_owned(),
                )
            })
            .collect();
        let borrowed: Vec<(&str, &str)> = pairs
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        let (back, pane) = HookEnv::from_vars(lookup(&borrowed)).unwrap();
        assert_eq!(back, env);
        assert_eq!(pane.as_deref(), Some("7"));
    }

    #[test]
    fn missing_or_invalid_values_mean_no_app() {
        let ok = [(EVENTS_VAR, "127.0.0.1:4242"), (TOKEN_VAR, TOKEN)];
        assert!(HookEnv::from_vars(lookup(&ok)).is_some());
        assert!(HookEnv::from_vars(lookup(&[])).is_none());
        assert!(HookEnv::from_vars(lookup(&[(EVENTS_VAR, "127.0.0.1:4242")])).is_none());
        assert!(HookEnv::from_vars(lookup(&[(TOKEN_VAR, TOKEN)])).is_none());
        for addr in [
            "10.0.0.5:4242",
            "example.com:80",
            "127.0.0.1",
            "127.0.0.1:0",
            "",
        ] {
            assert!(
                HookEnv::from_vars(lookup(&[(EVENTS_VAR, addr), (TOKEN_VAR, TOKEN)])).is_none(),
                "{addr}"
            );
        }
        for token in ["short", &TOKEN.replace('a', "z"), &format!("{TOKEN}00")] {
            assert!(
                HookEnv::from_vars(lookup(&[(EVENTS_VAR, "127.0.0.1:1"), (TOKEN_VAR, token)]))
                    .is_none()
            );
        }
        let bad_pane = [
            (EVENTS_VAR, "127.0.0.1:4242"),
            (TOKEN_VAR, TOKEN),
            (PANE_VAR, "../x"),
        ];
        assert_eq!(HookEnv::from_vars(lookup(&bad_pane)).unwrap().1, None);
    }

    #[test]
    fn debug_hides_the_token() {
        let env = HookEnv::new("127.0.0.1:4242".parse().unwrap(), TOKEN);
        let text = format!("{env:?}");
        assert!(!text.contains(TOKEN), "{text}");
        assert!(text.contains("127.0.0.1:4242"));
    }
}
