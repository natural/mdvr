# mdvr

Read-only macOS Markdown viewer built with GPUI and embedded WKWebView. Markdown,
GFM, syntax highlighting, Mermaid, KaTeX, local images, document search, outline,
exact code copy, relative navigation, live reload, appearance, and bounded native
resource policy run fully offline.

## Development

Verified tools: macOS arm64, Xcode 26.5 with Metal Toolchain, Rust/Cargo 1.98.1
(Homebrew), Bun 1.4.0. These are development versions, **not** a published minimum
macOS target. Full Xcode is required for GPUI's Metal shaders. If Metal is missing:

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
scripts/verify/check-packaging.sh
```

`Cargo.lock` and `web/bun.lock` stay tracked. Current packaging creates an unsigned
arm64 development app. `packaging/build-universal.sh` and `packaging/release.sh`
fail closed until Intel Rust target and Developer ID/notary credentials are available.
See [design](docs/design.md),
[numbered implementation plans](docs/001-foundation-and-feasibility.md), and
[acceptance evidence](docs/evidence/008-acceptance.md).
