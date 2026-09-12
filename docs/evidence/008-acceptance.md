# 008 — Acceptance and release evidence

Status: **candidate checks pass; release gate remains open**.

## Automated checks

Run from the repository root on the documented arm64 macOS host:

```text
cargo fmt --check                                      passed
cargo test --locked                                    passed (72 tests)
cargo clippy --frozen --all-targets -- -D warnings     passed
sh scripts/verify/check-fixtures.sh                   passed
cd web && bun install --frozen-lockfile                passed (134 packages)
cd web && bun test tests                               passed (15 tests)
cd web && bun run build                                passed
cargo build --release --locked                         passed
sh scripts/verify/check-packaging.sh                  passed
```

The release binary is arm64 Mach-O and links WebKit. An unsigned arm64
`packaging/build/mdvr.app` bundle was created and inspected; its Markdown file
association, bundled web assets, metadata, and linked system frameworks pass
scaffold checks. Cargo reports only known
upstream future-incompatibility notices for `block` and `proc-macro-error2`.

## Black-window regression

- Real Orca desktop screenshot exposed black `mdvr` window.
- Earlier asset/CSP/frame explanations were hypotheses, not proven causes.
  Native diagnostics confirmed attachment aborted because runtime lookup of
  `WKScriptMessageHandler` protocol metadata returned absent. Callback classes
  now register required selectors without requiring optional protocol metadata.
- Fix: native loader now prefers packaged `Contents/Resources/web`, falls back
  to checkout `web/dist`, and calls SDK-verified
  `loadFileURL:allowingReadAccessToURL:`. Navigation allows only exact bundled
  `index.html`; packaging rejects symlinks and network references.
- Path-selection tests cover packaged precedence, dev fallback, and exact
  navigation allowlist. `bun run build`, native checks, and package inspection
  verify bundle presence and local production module selection.
- After callback registration was fixed, `target/debug/mdvr readme.md` showed
  styled `Loading document…` text in Orca. Navigation allow and finish callbacks
  were observed. The black surface is resolved, but document rendering is not:
  JavaScript evaluation reports `window.mdvrLoadDocument is not a function`.
  Renderer bundle initialization remains under investigation.
- Source is queued until `didFinishNavigation`. Regression tests cover callback
  class registration and latest-generation pending-source drain. The queue write
  now executes in release builds too, rather than only inside `debug_assert!`.
- Latest checkpoint verified debug build, Rust tests/clippy and web tests/build.
  Release/package checks above predate these latest native changes; existing
  generated package is not evidence for the current source.

## Launch checks

- `--help`, `--version`, unsupported input, multiple input, missing path, and
  explicit-file validation passed.
- Explicit-file launch remained running and printed `mdvr: accepted ...`.
- Frontmost/process launch was confirmed for the pre-fix build and the
  pre-fix window was observable through Orca.
- Post-fix window displays the loading sentinel through Orca. Document content,
  link navigation, action bridge, and reload remain unaccepted.

## Unverified desktop behavior

The following remain unverified against a live embedded WKWebView: resize and
clipping, keyboard/focus transitions, close/reopen and activation, picker
interaction, embedded selection and clipboard behavior, reload selection/locator
preservation, bridge callback execution, CSP enforcement, Mermaid async output,
resource revocation, and appearance propagation.

Unit and Bun tests cover the corresponding pure/core behavior but do not close
these app-evidence rows.

## Security and policy gaps

Local canonical-root/symlink policy and remote URL validation have automated
core tests. Embedded hostile-content execution, outside-root consent, remote
consent, DNS resolution and destination/connect-race protection, actual HTTP
fetch/redirect handling, and WebKit resource transport remain unverified.

## Performance and distribution

Performance distributions were not measured on the required 2020 M1 MacBook Air
8 GB baseline. Intel/x86_64 and universal builds are blocked by the available
arm64-only target/toolchain. The app bundle is scaffolded, but no DMG, icon,
clean-machine test, signing, notarization, or Gatekeeper evidence exists.

Signing/notarization credentials and packaging environment were not created or
assumed. These are release blockers, not passes.
