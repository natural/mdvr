# mdvr

Read-only macOS Markdown viewer built with GPUI and embedded WKWebView. Markdown,
GFM, syntax highlighting, Mermaid, KaTeX, local images, document search, outline,
exact code copy, relative navigation, live reload, appearance, and bounded native
resource policy work offline. Remote images remain blocked until per-document consent,
then use credential-free pinned-address native fetches.

## Development

Verified tools: macOS arm64, Xcode 26.5 with Metal Toolchain, Rust/Cargo 1.98.1
(Homebrew), Bun 1.4.0. Packaged minimum is macOS 11.0 and is checked against
Mach-O deployment metadata. Full Xcode is required for GPUI's Metal shaders. If Metal is missing:

```sh
xcodebuild -downloadComponent MetalToolchain
```

Build and run:

```sh
(cd web && bun install --frozen-lockfile && bun run build)
cargo run --locked -- readme.md
```

Verify:

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
(cd web && bun test tests && bun run build)
swift scripts/verify/check-renderer.swift web/dist/index.html
python3 scripts/verify/measure-reload.py
sh scripts/verify/check-packaging.sh
sh packaging/build-dmg.sh
sh scripts/verify/check-dmg.sh packaging/build/mdvr.dmg
```

`Cargo.lock` and `web/bun.lock` stay tracked. Current packaging creates an unsigned
universal (`x86_64 arm64`) development app and DMG. `packaging/release.sh` fails
closed until Developer ID/notary credentials are available; Intel runtime still
needs Intel hardware.
See [design](docs/design.md),
[numbered implementation plans](docs/001-foundation-and-feasibility.md), and
[acceptance evidence](docs/evidence/008-acceptance.md).
