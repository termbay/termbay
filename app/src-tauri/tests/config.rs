//! Guards settings in `tauri.conf.json` and the capabilities that are easy
//! to flip by accident.

const CONFIG: &str = include_str!("../tauri.conf.json");
const MAIN_CAPABILITY: &str = include_str!("../capabilities/main.json");

/// `freezePrototype` freezes `Object.prototype` before the UI loads. xterm.js
/// does not start under it, which leaves an empty grey window with no
/// terminal, so it must stay off.
#[test]
fn prototype_freezing_stays_off() {
    let compact: String = CONFIG.split_whitespace().collect();
    assert!(
        compact.contains(r#""freezePrototype":false"#),
        "tauri.conf.json must set \"freezePrototype\": false (xterm.js breaks with frozen prototypes)"
    );
}

/// The main window may call the pty commands (with `pty_complete` and
/// `pty_info`), `get_config`, listen to events (for `pty-exit`) and close
/// itself (D19), and nothing else: no shell, fs or http plugins.
#[test]
fn main_capability_grants_only_the_pty_commands_and_event_listening() {
    let compact: String = MAIN_CAPABILITY.split_whitespace().collect();
    let start = compact
        .find(r#""permissions":["#)
        .expect("capabilities/main.json must have a permissions list");
    let list = &compact[start + r#""permissions":["#.len()..];
    let list = &list[..list.find(']').expect("unterminated permissions list")];
    let mut permissions: Vec<&str> = list
        .split(',')
        .map(|p| p.trim_matches('"'))
        .filter(|p| !p.is_empty())
        .collect();
    permissions.sort_unstable();
    assert_eq!(
        permissions,
        [
            "allow-get-config",
            "allow-pty-ack",
            "allow-pty-close",
            "allow-pty-complete",
            "allow-pty-info",
            "allow-pty-open",
            "allow-pty-resize",
            "allow-pty-write",
            "core:event:allow-listen",
            "core:event:allow-unlisten",
            "core:window:allow-close",
        ]
    );
    assert!(compact.contains(r#""windows":["main"]"#));
}
