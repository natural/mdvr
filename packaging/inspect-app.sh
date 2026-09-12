#!/bin/sh
set -eu

export LC_ALL=C
export TZ=UTC

app=${1:-}

fail() {
    printf 'packaging inspect: %s\n' "$1" >&2
    exit 1
}

[ "$#" -eq 1 ] || fail "usage: sh packaging/inspect-app.sh PATH_TO_APP"
[ "$(uname -s)" = Darwin ] || fail "macOS is required"
for command in file find grep lipo otool plutil; do
    command -v "$command" >/dev/null 2>&1 || fail "required command missing: $command"
done
[ -d "$app" ] || fail "app bundle missing: $app"
case "$app" in
*.app) ;;
*) fail "not an app bundle: $app" ;;
esac

contents="$app/Contents"
main="$contents/MacOS/mdvr"
plist="$contents/Info.plist"
[ -d "$contents" ] || fail "Contents directory missing"
[ -f "$main" ] || fail "main executable missing: $main"
[ -x "$main" ] || fail "main executable is not executable: $main"
[ -f "$plist" ] || fail "Info.plist missing: $plist"

symlink=$(find "$app" -type l -print -quit)
[ -z "$symlink" ] || fail "bundle contains symlink: $symlink"
plutil -lint "$plist" >/dev/null || fail "invalid Info.plist"

bundle_executable=$(plutil -extract CFBundleExecutable raw -o - "$plist")
[ "$bundle_executable" = mdvr ] || fail "CFBundleExecutable is not mdvr: $bundle_executable"
bundle_type=$(plutil -extract CFBundlePackageType raw -o - "$plist")
[ "$bundle_type" = APPL ] || fail "CFBundlePackageType is not APPL: $bundle_type"
plutil -extract CFBundleDocumentTypes xml1 -o /dev/null "$plist" >/dev/null 2>&1 || fail "Markdown file association missing"
if plutil -extract CFBundleURLTypes xml1 -o /dev/null "$plist" >/dev/null 2>&1; then
    fail "custom URL scheme is not supported by current design"
fi

web="$contents/Resources/web"
[ -d "$web" ] || fail "bundled web assets missing: $web"
[ -f "$web/index.html" ] || fail "bundled web entrypoint missing: $web/index.html"
grep -Eq 'src="\./[^"/]+\.js"' "$web/index.html" || fail "bundled web entrypoint has no local production module"
! grep -Eq 'https?://' "$web/index.html" || fail "bundled web entrypoint references network content"

printf 'packaging inspect: %s\n' "$app"
printf '  metadata: valid APPL, executable=%s, Markdown association=present, custom URL scheme=absent\n' "$bundle_executable"

macho_count=0
inspect_macho() {
    path=$1
    kind=$(file -b "$path")
    case "$kind" in
    *Mach-O*)
        macho_count=$((macho_count + 1))
        archs=$(lipo -archs "$path") || fail "cannot inspect architectures: $path"
        [ "$archs" = arm64 ] || fail "embedded Mach-O must be arm64-only: $path ($archs)"
        printf '  Mach-O: %s (%s)\n' "${path#"$app/"}" "$archs"
        otool -L "$path" || fail "cannot inspect linked libraries: $path"
        ;;
    esac
}

# Generated bundles must contain no symlinks.
while IFS= read -r path; do
    inspect_macho "$path"
done <<EOF
$(find "$contents" -type f -print)
EOF

[ "$macho_count" -ge 1 ] || fail "no Mach-O executable or library found"
printf '  framework inspection: Contents/Frameworks checked; embedded Mach-O count=%s\n' "$macho_count"
printf 'packaging inspect: passed\n'
