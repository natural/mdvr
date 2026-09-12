#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
dmg=${1:-$root/packaging/build/mdvr.dmg}
mount=
cleanup() { [ -z "$mount" ] || hdiutil detach -quiet "$mount" >/dev/null 2>&1 || true; }
trap cleanup EXIT INT TERM

[ -f "$dmg" ] || { printf 'packaging: DMG missing: %s\n' "$dmg" >&2; exit 1; }
mount=$(hdiutil attach -readonly -nobrowse -plist "$dmg" | plutil -extract system-entities xml1 -o - - | awk -F'[<>]' '/mount-point/{getline; print $3; exit}')
[ -n "$mount" ] || { printf 'packaging: DMG did not mount\n' >&2; exit 1; }
sh "$root/packaging/inspect-app.sh" "$mount/mdvr.app"
printf 'packaging: DMG verified\n'
