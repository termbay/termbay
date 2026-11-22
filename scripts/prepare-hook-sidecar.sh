#!/usr/bin/env bash
# Builds termbay-hook in release and puts it where `tauri build` looks for
# sidecars, so the packages ship it next to `termbay` (task D2 in
# docs/FASE2-PLANO.md, decision E11).
#
# Usage (from the repository root):
#   bash scripts/prepare-hook-sidecar.sh
#   ui/node_modules/.bin/tauri build --config app/src-tauri/tauri.hook.conf.json
#
# The sidecar is only listed in tauri.hook.conf.json, not in tauri.conf.json:
# Tauri checks the file exists whenever the app crate builds, and plain
# `cargo build`/`cargo test` must keep working without this step.

set -euo pipefail

cd "$(dirname "$0")/.."

triple="$(rustc -vV | sed -n 's/^host: //p')"
if [[ -z "$triple" ]]; then
  echo "error: could not read the host target triple from rustc -vV" >&2
  exit 2
fi
ext=""
[[ "$triple" == *windows* ]] && ext=".exe"

cargo build --release --locked -p termbay-hook

mkdir -p app/src-tauri/binaries
cp "target/release/termbay-hook$ext" "app/src-tauri/binaries/termbay-hook-$triple$ext"
echo "sidecar ready: app/src-tauri/binaries/termbay-hook-$triple$ext"
