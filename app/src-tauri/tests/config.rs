//! Guards settings in `tauri.conf.json` that are easy to flip by accident.

const CONFIG: &str = include_str!("../tauri.conf.json");

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
