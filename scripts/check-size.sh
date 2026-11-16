#!/usr/bin/env bash
# Report the size of release artifacts and fail when any of them is above the limit.
#
# Usage: scripts/check-size.sh [--title TEXT] PATH...
#
# Each PATH is a file or a directory (a directory, such as a macOS .app, is
# measured as the sum of its regular files). A PATH that does not exist is an
# error: a missing artifact must never pass silently.
#
# Environment:
#   SIZE_LIMIT_MB         limit per artifact, in megabytes (1 MB = 1,000,000 bytes). Default: 20.
#   GITHUB_STEP_SUMMARY   when set (GitHub Actions), the Markdown table is appended to it.
#
# Exit codes: 0 all artifacts within the limit, 1 at least one above it,
#             2 usage error or missing artifact.
#
# Portable: plain bash + find + wc + awk, so it runs on macOS, Linux and the
# Git Bash shipped with the Windows runners.

set -euo pipefail

limit_mb="${SIZE_LIMIT_MB:-20}"
title="Artifact sizes"

if [[ "${1:-}" == "--title" ]]; then
  title="${2:?--title needs a value}"
  shift 2
fi

if [[ $# -eq 0 ]]; then
  echo "usage: $0 [--title TEXT] PATH..." >&2
  exit 2
fi

if ! [[ "$limit_mb" =~ ^[0-9]+$ ]] || [[ "$limit_mb" -eq 0 ]]; then
  echo "error: SIZE_LIMIT_MB must be a positive integer, got '$limit_mb'" >&2
  exit 2
fi

limit_bytes=$((limit_mb * 1000000))

# Size in bytes of a file, or the sum of the regular files inside a directory.
size_of() {
  local path="$1" total=0 n f
  if [[ -d "$path" ]]; then
    while IFS= read -r -d '' f; do
      n=$(wc -c <"$f")
      total=$((total + n))
    done < <(find "$path" -type f -print0)
  else
    n=$(wc -c <"$path")
    total=$((n + 0))
  fi
  echo "$total"
}

human() {
  awk -v b="$1" 'BEGIN { printf "%.2f MB", b / 1000000 }'
}

status=0
table="### ${title}"$'\n\n'"Limit: ${limit_mb} MB per artifact."$'\n\n'
table+="| Artifact | Size | Bytes | Result |"$'\n'
table+="|---|---:|---:|---|"$'\n'

for path in "$@"; do
  name="${path##*/}"
  if [[ ! -e "$path" ]]; then
    table+="| \`${path}\` | - | - | missing |"$'\n'
    echo "error: artifact not found: $path" >&2
    [[ $status -eq 0 ]] && status=2
    continue
  fi
  bytes=$(size_of "$path")
  if [[ "$bytes" -gt "$limit_bytes" ]]; then
    result="above the limit"
    echo "error: $name is $(human "$bytes"), above the ${limit_mb} MB limit" >&2
    status=1
  else
    result="ok"
  fi
  table+="| \`${name}\` | $(human "$bytes") | ${bytes} | ${result} |"$'\n'
done

printf '%s\n' "$table"
if [[ -n "${GITHUB_STEP_SUMMARY:-}" ]]; then
  printf '%s\n' "$table" >>"$GITHUB_STEP_SUMMARY"
fi

exit "$status"
