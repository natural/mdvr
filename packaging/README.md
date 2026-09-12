# macOS packaging scaffolding

Scripts here build and inspect unsigned arm64 or universal development artifacts.
`release.sh` performs credential-gated universal build, hardened-runtime signing,
notarization, stapling, and Gatekeeper checks. Nothing installs files or modifies
`PATH`.

## Exact commands

From repository root:

```sh
cd web
bun install --frozen-lockfile
bun run build
cd ..
cargo build --release --locked
sh packaging/build-app.sh
sh scripts/verify/check-packaging.sh
sh packaging/build-dmg.sh
sh scripts/verify/check-dmg.sh
open packaging/build/mdvr.dmg
```

Safe prerequisite checks without writing release artifacts:

```sh
sh packaging/build-app.sh --dry-run
sh packaging/release.sh --dry-run
```

Universal signed release, after installing both Rust targets and storing a
`notarytool` keychain profile:

```sh
SIGNING_IDENTITY='Developer ID Application: …' \
NOTARY_PROFILE=mdvr \
sh packaging/release.sh
```

Explicit CLI installation remains manual and does not edit shell configuration:

```sh
mkdir -p "$HOME/.local/bin"
ln -s /Applications/mdvr.app/Contents/Resources/bin/mdvr "$HOME/.local/bin/mdvr"
```

Bundled CLI delegates through macOS LaunchServices, so repeated CLI/Finder/Dock
opens reuse running app. It validates argument count, path existence/readability,
Markdown extension, UTF-8, and 20 MiB hard ceiling, then writes a private bounded
request under `$TMPDIR`, waits up to 10 seconds for app consumption/load, and reports
acceptance only after app writes its derived acknowledgment. Requests and acknowledgments
are removed on success, rejection, signal, or timeout. Symlink it only after placing
`mdvr.app` in `/Applications`; no shell profile is edited.

`build-app.sh` fails closed unless macOS tools, the release `target/release/mdvr`
binary, `web/dist/index.html`, and valid `Info.plist` exist. It rejects non-Mach-O
or non-arm64 binaries and web-asset symlinks. It replaces only
`packaging/build/mdvr.app`, then normalizes copied timestamps to `2000-01-01
00:00:00 UTC`.

`inspect-app.sh` validates bundle metadata, Markdown file associations, project
license and complete generated third-party notices, absence of custom URL schemes,
bundled web assets, symlinks, and every embedded Mach-O
file with `file`, `lipo`, and `otool -L`. Current bundle has no embedded
frameworks; linked system frameworks are printed for inspection.
`build-dmg.sh` creates a compressed HFS+ image; `check-dmg.sh` mounts it read-only
and reruns full app inspection before detaching.

## Design limits and blockers

- Bundle contains `Contents/MacOS/mdvr` and `Contents/Resources/web`.
- Finder Markdown associations are included because design §2 requires Finder
  file opens. No custom URL scheme is included: design defines HTTP/HTTPS and
  `mailto:` external link handling, not an `mdvr:` scheme; design also rejects
  remote URL CLI inputs.
- `assets/icon.svg` is an original repository-owned design. `build-icon.sh`
  generates required raster sizes and `AppIcon.icns`; bundle inspection validates it.
- `build-universal.sh` builds pinned arm64/x86_64 targets and combines them with
  `lipo`; current machine still lacks the x86_64 Rust target, so current artifact
  remains arm64-only.
- `release.sh` fails closed without `SIGNING_IDENTITY` and `NOTARY_PROFILE`, signs
  with hardened runtime and timestamping, notarizes/staples app and DMG, then runs
  `codesign`, `stapler`, and Gatekeeper checks. Current keychain lacks Developer ID
  Application/notary credentials, so no signed artifact is claimed.
- `LSMinimumSystemVersion` is 11.0, matching release Mach-O `LC_BUILD_VERSION`;
  inspection fails on metadata/binary drift.
- Native code resolves production assets from `Contents/Resources/web` in the
  app bundle, with `web/dist` as dev-checkout fallback. `WKWebView::loadFileURL`
  grants read access only to that canonical asset directory; navigation allows
  only its `index.html`. Packaging rejects symlinks and network references.
