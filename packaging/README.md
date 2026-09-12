# macOS packaging scaffolding

Scripts here build and inspect one unsigned, arm64-only `.app` and compressed
DMG. They do not build Rust or web inputs, sign, notarize, install anything, or
modify `PATH`.

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

Safe prerequisite check without writing the bundle:

```sh
sh packaging/build-app.sh --dry-run
```

Explicit CLI installation remains manual and does not edit shell configuration:

```sh
mkdir -p "$HOME/.local/bin"
ln -s /Applications/mdvr.app/Contents/Resources/bin/mdvr "$HOME/.local/bin/mdvr"
```

Bundled CLI delegates through macOS LaunchServices, so repeated CLI/Finder/Dock
opens reuse running app. It validates argument count, path existence/readability,
Markdown extension, UTF-8, and 20 MiB hard ceiling before reporting acceptance. Symlink it only after placing
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
- Current release artifact is arm64-only. No x86_64 target or universal binary
  is claimed.
- Bundle is unsigned and unnotarized. Developer credentials, hardened-runtime
  settings, notarization, stapling, Gatekeeper, clean-machine launch, and Intel
  launch remain release blockers.
- `LSMinimumSystemVersion` is 11.0, matching release Mach-O `LC_BUILD_VERSION`;
  inspection fails on metadata/binary drift.
- Native code resolves production assets from `Contents/Resources/web` in the
  app bundle, with `web/dist` as dev-checkout fallback. `WKWebView::loadFileURL`
  grants read access only to that canonical asset directory; navigation allows
  only its `index.html`. Packaging rejects symlinks and network references.
