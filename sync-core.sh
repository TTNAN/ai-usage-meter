#!/bin/sh
# ai-usage-core lives inside each app's src-tauri/crates/ because every project
# in this repo is shipped as a standalone zip that has to build on its own.
# This script keeps the two copies byte-identical: it copies THIS project's core
# into the sibling project, then compares both.
#
#   ./sync-core.sh          copy into the sibling, then verify
#   ./sync-core.sh --check  only verify, change nothing
#
# Run it after editing anything under src-tauri/crates/ai-usage-core/.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
core="$here/src-tauri/crates/ai-usage-core"
siblings="../ai-usage-meter ../ai-usage-meter-dock"
check_only=false
[ "${1:-}" = "--check" ] && check_only=true

[ -d "$core" ] || { echo "sync-core: $core not found" >&2; exit 1; }

targets=""
for other in $siblings; do
  [ -d "$other/src-tauri/crates" ] || continue
  other=$(cd "$other" && pwd)
  [ "$other" = "$here" ] && continue
  targets="$targets $other"
done

if [ -z "$targets" ]; then
  echo "sync-core: no sibling project next to $here — nothing to compare"
  exit 0
fi

for other in $targets; do
  dst="$other/src-tauri/crates/ai-usage-core"
  if [ "$check_only" = false ]; then
    rm -rf "$dst"
    mkdir -p "$(dirname "$dst")"
    cp -R "$core" "$dst"
    echo "sync-core: copied -> $other"
  fi
  if diff -r -q "$core" "$dst" >/dev/null 2>&1; then
    echo "sync-core: $other matches"
  else
    echo "sync-core: $other DIFFERS from $here" >&2
    diff -r "$core" "$dst" >&2 || true
    exit 1
  fi
done
