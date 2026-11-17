// Declaring the app commands makes Tauri check them against the
// capabilities (`capabilities/main.json`): a command that is not allowed
// there cannot be called from any window. It also generates the
// `allow-<command>` permissions used by that file.
const COMMANDS: &[&str] = &[
    "pty_open",
    "pty_write",
    "pty_resize",
    "pty_close",
    "pty_ack",
    "pty_complete",
    "pty_info",
    "get_config",
];

fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
    )
    .expect("failed to run the Tauri build script");
}
