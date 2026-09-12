#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
source="$root/assets/icon.svg"
out=${1:-$root/packaging/build/AppIcon.icns}
temporary=$(mktemp -d "${TMPDIR:-/tmp}/mdvr-icon.XXXXXX")
iconset="$temporary/AppIcon.iconset"
cleanup() { rm -rf "$temporary"; }
trap cleanup EXIT INT TERM
mkdir -p "$iconset" "$(dirname -- "$out")"
base="$iconset/base.png"
sips -s format png "$source" --out "$base" >/dev/null
for spec in '16 icon_16x16.png' '32 icon_16x16@2x.png' '32 icon_32x32.png' '64 icon_32x32@2x.png' '128 icon_128x128.png' '256 icon_128x128@2x.png' '256 icon_256x256.png' '512 icon_256x256@2x.png' '512 icon_512x512.png' '1024 icon_512x512@2x.png'; do
    set -- $spec
    sips -z "$1" "$1" "$base" --out "$iconset/$2" >/dev/null
done
rm "$base"
iconutil -c icns "$iconset" -o "$out"
printf 'packaging: created %s\n' "$out"
