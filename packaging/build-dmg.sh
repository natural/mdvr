#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
app=${1:-$root/packaging/build/mdvr.app}
dmg=${2:-$root/packaging/build/mdvr.dmg}

[ -d "$app" ] || { printf 'packaging: app missing: %s\n' "$app" >&2; exit 1; }
command -v hdiutil >/dev/null 2>&1 || { printf 'packaging: hdiutil missing\n' >&2; exit 1; }
rm -f "$dmg"
hdiutil create -quiet -fs HFS+ -volname mdvr -srcfolder "$app" -format UDZO "$dmg"
hdiutil imageinfo "$dmg" >/dev/null
printf 'packaging: created %s\n' "$dmg"
