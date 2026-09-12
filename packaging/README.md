# macOS packaging scaffolding

Scripts here build and inspect one unsigned, arm64-only `.app`. They do not
build Rust or web inputs, sign, notarize, create a DMG, install anything, or
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
open packaging/build/mdvr.app
```

Safe prerequisite check without writing the bundle:

```sh
sh packaging/build-app.sh --dry-run
```

Explicit CLI installation remains manual and does not edit shell configuration:

```sh
mkdir -p "$HOME/.local/bin"
install -m 755 packaging/build/mdvr.app/Contents/MacOS/mdvr "$HOME/.local/bin/mdvr"
```

`build-app.sh` fails closed unless macOS tools, the release `target/release/mdvr`
binary, `web/dist/index.html`, and valid `Info.plist` exist. It rejects non-Mach-O
or non-arm64 binaries and web-asset symlinks. It replaces only
`packaging/build/mdvr.app`, then normalizes copied timestamps to `2000-01-01
00:00:00 UTC`.

`inspect-app.sh` validates bundle metadata, Markdown file associations, absence
of custom URL schemes, bundled web assets, symlinks, and every embedded Mach-O
file with `file`, `lipo`, and `otool -L`. Current bundle has no embedded
frameworks; linked system frameworks are printed for inspection.

## Design limits and blockers

- Bundle contains `Contents/MacOS/mdvr` and `Contents/Resources/web`.
- Finder Markdown associations are included because design §2 requires Finder
  file opens. No custom URL scheme is included: design defines HTTP/HTTPS and
  `mailto:` external link handling, not an `mdvr:` scheme; design also rejects
  remote URL CLI inputs.
- No icon is bundled because repository has no approved icon asset or
  provenance. Release packaging remains blocked until one is supplied.
- Current release artifact is arm64-only. No x86_64 target or universal binary
  is claimed.
- Bundle is unsigned and unnotarized. Developer credentials, hardened-runtime
  settings, notarization, stapling, Gatekeeper, DMG creation, clean-machine
  launch, and Intel launch remain release blockers.
- `LSMinimumSystemVersion` is omitted until pinned dependency and WebKit minimum
  support is resolved; no deployment target is claimed.
- Native code resolves production assets from `Contents/Resources/web` in the
  app bundle, with `web/dist` as dev-checkout fallback. `WKWebView::loadFileURL`
  grants read access only to that canonical asset directory; navigation allows
  only its `index.html`. Packaging rejects symlinks and network references.
