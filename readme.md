# mdvr

Read-only macOS Markdown viewer. GPUI + embedded WKWebView design; currently only
build bootstrap, not a functional viewer. See [design](docs/design.md) and
[implementation sequence](docs/001-foundation-and-feasibility.md).

## Development

Verified tools: macOS arm64, Xcode 26.5 with Metal Toolchain, Rust/Cargo 1.98.1
(Homebrew), Bun 1.4.0. These are development versions, **not** a published minimum
macOS target. Full Xcode is required for GPUI's Metal shaders. If Metal is missing:

```sh
xcodebuild -downloadComponent MetalToolchain
```

Build/check from repository root:

```sh
cargo build --locked
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
(cd web && bun install --frozen-lockfile && bun run build)
```

`cargo run --locked` opens the GPUI bootstrap window. Web output in `web/dist/`
is currently separate; WKWebView embedding is plan 001. No application tests yet;
Cargo's zero-test result is only a build check. No web dependencies means Bun
currently emits no lockfile; commit it when dependencies are added.

Keep `Cargo.lock` tracked. Only integration owner changes dependencies and shared
build files. Rustup is not required for the installed host toolchain; universal
build setup must add/prove Intel target support in plans 001/008. Signing and
notarization credentials remain unconfirmed. See numbered plans for parallel
ownership, contract gates, acceptance and release requirements.
