#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
app="$root/packaging/build/mdvr.app"
dmg="$root/packaging/build/mdvr.dmg"
archive="$root/packaging/build/mdvr.zip"

if [ "${1:-}" = "--dry-run" ]; then
    echo "packaging: release requires SIGNING_IDENTITY and NOTARY_PROFILE"
    echo "packaging: builds universal app, signs, notarizes, staples, and verifies app and DMG"
    exit 0
fi
[ "$#" -eq 0 ] || {
    echo "Usage: SIGNING_IDENTITY=... NOTARY_PROFILE=... sh packaging/release.sh [--dry-run]" >&2
    exit 2
}
: "${SIGNING_IDENTITY:?SIGNING_IDENTITY is required}"
: "${NOTARY_PROFILE:?NOTARY_PROFILE is required}"
for command in bun codesign ditto spctl xcrun; do
    command -v "$command" >/dev/null 2>&1 || {
        echo "packaging: required command missing: $command" >&2
        exit 1
    }
done

(cd "$root/web" && bun install --frozen-lockfile && bun run build)
bun "$root/scripts/verify/generate-notices.mjs"
sh "$root/packaging/build-universal.sh"
sh "$root/packaging/build-app.sh"
codesign --force --options runtime --timestamp --sign "$SIGNING_IDENTITY" "$app/Contents/MacOS/mdvr"
codesign --force --options runtime --timestamp --sign "$SIGNING_IDENTITY" "$app"
codesign --verify --deep --strict --verbose=2 "$app"
rm -f "$archive"
ditto -c -k --keepParent "$app" "$archive"
xcrun notarytool submit "$archive" --keychain-profile "$NOTARY_PROFILE" --wait
xcrun stapler staple "$app"
xcrun stapler validate "$app"
sh "$root/packaging/build-dmg.sh" "$app" "$dmg"
codesign --force --timestamp --sign "$SIGNING_IDENTITY" "$dmg"
xcrun notarytool submit "$dmg" --keychain-profile "$NOTARY_PROFILE" --wait
xcrun stapler staple "$dmg"
xcrun stapler validate "$dmg"
spctl --assess --type execute --verbose=2 "$app"
spctl --assess --type open --context context:primary-signature --verbose=2 "$dmg"
echo "packaging: signed and notarized $dmg"
