#!/bin/sh
set -eu

export LC_ALL=C
export TZ=UTC
umask 022

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
binary="$root/target/release/mdvr"
web_dist="$root/web/dist"
plist="$root/packaging/Info.plist"
app="$root/packaging/build/mdvr.app"
dry_run=false

fail() {
    printf 'packaging: %s\n' "$1" >&2
    exit 1
}

usage() {
    printf '%s\n' "Usage: sh packaging/build-app.sh [--dry-run]"
}

if [ "$#" -gt 1 ]; then
    usage >&2
    exit 2
fi
if [ "$#" -eq 1 ]; then
    case "$1" in
    --dry-run) dry_run=true ;;
    -h | --help)
        usage
        exit 0
        ;;
    *)
        usage >&2
        exit 2
        ;;
    esac
fi

[ "$(uname -s)" = Darwin ] || fail "macOS is required"
for command in cp find file grep iconutil lipo mkdir otool plutil rm sips touch; do
    command -v "$command" >/dev/null 2>&1 || fail "required command missing: $command"
done
[ -f "$binary" ] || fail "release binary missing: $binary"
[ -x "$binary" ] || fail "release binary is not executable: $binary"
[ -d "$web_dist" ] || fail "built web assets missing: $web_dist"
[ -f "$web_dist/index.html" ] || fail "built web entrypoint missing: $web_dist/index.html"
grep -Eq 'src="\./[^"/]+\.js"' "$web_dist/index.html" || fail "built web entrypoint has no local production module"
! grep -Eq 'https?://' "$web_dist/index.html" || fail "built web entrypoint references network content"
[ -f "$plist" ] || fail "bundle metadata missing: $plist"

symlink=$(find "$web_dist" -type l -print -quit)
[ -z "$symlink" ] || fail "built web assets contain symlink: $symlink"

plutil -lint "$plist" >/dev/null || fail "invalid bundle metadata: $plist"

kind=$(file -b "$binary")
case "$kind" in
*Mach-O*) ;;
*) fail "release binary is not Mach-O: $kind" ;;
esac
archs=$(lipo -archs "$binary")
[ "$archs" = arm64 ] || fail "release binary must be arm64-only; found: $archs"

if $dry_run; then
    printf 'packaging: dry run passed\n'
    printf '  binary: %s (%s)\n' "$binary" "$archs"
    printf '  web: %s\n' "$web_dist"
    printf '  output: %s\n' "$app"
    exit 0
fi

rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources/web" "$app/Contents/Resources/bin"
sh "$root/packaging/build-icon.sh" "$root/packaging/build/AppIcon.icns"
cp "$binary" "$app/Contents/MacOS/mdvr"
cp "$plist" "$app/Contents/Info.plist"
cp "$root/packaging/build/AppIcon.icns" "$app/Contents/Resources/AppIcon.icns"
cp "$root/license" "$app/Contents/Resources/LICENSE"
cp "$root/THIRD_PARTY_NOTICES.md" "$app/Contents/Resources/THIRD_PARTY_NOTICES.md"
cp "$root/packaging/mdvr-cli" "$app/Contents/Resources/bin/mdvr"
cp -R "$web_dist/." "$app/Contents/Resources/web/"
find "$app/Contents" -type d -exec chmod 755 {} +
find "$app/Contents/Resources" -type f -exec chmod 644 {} +
chmod 755 "$app/Contents/MacOS/mdvr" "$app/Contents/Resources/bin/mdvr"
find "$app" -exec touch -t 200001010000 {} +

sh "$root/packaging/inspect-app.sh" "$app"
printf 'packaging: created %s\n' "$app"
