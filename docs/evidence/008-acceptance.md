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
  were observed. Subsequent renderer startup failed because WebKit rejected
  external ES modules over `file://`. `web/build.mjs` now changes the generated
  self-contained script to classic `defer`, preserving CSP and restricted read
  access. The built bundle initializes and renders Markdown.
- Runtime regression command:
  `swift scripts/verify/check-renderer.swift web/dist/index.html` passed.
  This probes actual WKWebView loading and asserts rendered heading content.
- Orca confirmed `target/debug/mdvr readme.md` visibly renders headings, links,
  paragraphs, and code blocks on macOS 26.7 (25G229), arm64. Evidence:
  [rendered readme](screenshots/readme-rendered.png). This uses native code from
  checkpoint `1d3b443` plus the classic-script build fix.
- Source is queued until `didFinishNavigation`. Regression tests cover callback
  class registration and latest-generation pending-source drain. The queue write
  now executes in release builds too, rather than only inside `debug_assert!`.
- Live revision-1 navigation bridge acceptance passed: clicking a relative
  Markdown link changed `First` to `Second`; editing that second file then changed
  the visible heading to `Second Reloaded`. This also verifies bridge polling and
  reload-worker transfer to the newly navigated document. Evidence:
  [navigation and reload](screenshots/navigation-reload.png).
- Context-bound local image requests now cross the validated revision-1 bridge,
  resolve only through native canonical-root policy, and return bounded bytes as
  object URLs. A same-root SVG rendered in live WKWebView; unsupported formats,
  outside-root paths, symlink escapes, and stale grants remain denied. Evidence:
  [local resource](screenshots/local-resource.png).
- Valid persisted text scale is loaded from conventional Application Support,
  applied through validated appearance tokens, and retained while current root and
  document are atomically saved. Isolated `HOME` runtime at 150% visibly enlarged
  reader text and preserved scale while saving paths. Evidence:
  [preferences appearance](screenshots/preferences-appearance.png).
- Embedded ⌘F opens a keyboard-accessible search surface; Enter/Shift-Enter and
  buttons move wrapped literal matches, Escape closes and restores document focus.
  Live WKWebView found and selected `GPUI`; revision-1 search actions reached native
  bridge context. Evidence: [search](screenshots/search.png).
- Every fenced block exposes an accessible Copy button. Live WKWebView copy of
  first `readme.md` block produced exact source
  `xcodebuild -downloadComponent MetalToolchain` on pasteboard; fallback uses a
  temporary exact DOM range only after Clipboard API failure. Evidence:
  [code copy](screenshots/code-copy.png).
- Responsive Contents control builds links using safe text nodes from rendered
  heading IDs. Live outline expanded with `mdvr` and `Development`; selecting
  `Development` drove validated anchor navigation and changed scroll position.
  Evidence: [outline](screenshots/outline.png).
- Reload now captures first visible stable block plus viewport offset and exact
  selection text before DOM replacement, restores matching block offset, and
  recreates unchanged selection across text nodes without moving focus. Source
  regression check exists; live scrolled/selection evidence remains open.
- Directory launch now starts ignore-aware discovery off GPUI thread, applies
  bounded progressive batches to native picker state, and opens selected files
  into same window. Missing GPUI `font-kit` feature caused invisible text; enabling
  it fixed native text while keeping platform defaults disabled. Live directory
  picker listed four Markdown files and opened `first.md`. Evidence:
  [picker](screenshots/picker.png), [opened document](screenshots/picker-open.png).
  Native focus plus Down/Up and Return keyboard handling also passed: Down then
  Return opened `image.md`. Evidence: [keyboard open](screenshots/picker-keyboard-open.png).
- WKWebView becomes first responder after direct or picker launch; live direct
  launch accepted ⌘F without a prerequisite document click. HTTP(S) targets now
  pass bounded credential/private-address policy before native `NSWorkspace`
  dispatch; bounded control-free mailto targets use same shell-free dispatch.
- ⌘+/⌘-/⌘0 now send context-bound text-scale actions to native state; native
  bounds 50–300%, reapplies validated appearance tokens, and atomically persists
  scale. Isolated runtime ⌘= changed visible scale and saved 110%. Evidence:
  [text scale](screenshots/text-scale.png).
- Visible Back/Forward/Reload controls and ⌘[/⌘]/⌘R route through context-bound
  native history actions rather than browser history. Live sequence opened
  `second.md`, returned to `first.md`, then moved forward to `second.md`, retaining
  same window and transferring reload watcher. Evidence: [history](screenshots/history.png).
- Saved validated window origin and content size now initialize GPUI bounds,
  recover onto available displays, and update atomically from live viewport size
  without titlebar-growth drift. Isolated 800×600 at (200,200) reopened there;
  observed outer window was 800×632 including titlebar. Evidence:
  [window geometry](screenshots/window-geometry.png).
- macOS reopen callback recreates one window only when none remain, using retained
  launch state and latest safe geometry. Packaged app window was closed; process
  stayed alive; reopening bundle restored same document in new window under same
  PID. Evidence: [close and reopen](screenshots/reopen.png).
- macOS file-open events decode native file URLs, queue paths into active app,
  and transactionally navigate existing window; multiple events remain FIFO.
  Packaged `open -a mdvr` changed `first.md` to `second.md`, then decoded and
  opened `space doc.md`, with one PID/window throughout. Evidence:
  [Finder reuse](screenshots/finder-reuse.png). After closing that window, Dock
  reopen loaded latest persisted `space doc.md`, not original process launch path.
- Hostile live fixture strips scripts, event handlers, CSS, frames, forms and
  orphan form inputs. Local SVG bytes now pass renderer SVG sanitizer before Blob
  creation, removing scripts, handlers and every href/xlink external reference.
  Live WKWebView showed only safe text/image output. Evidence:
  [hostile content](screenshots/hostile-content.png).
- Files above 10 MiB now stop on visible full-load confirmation; confirmed files
  retain permission across reload while all source paths enforce a 20 MiB hard
  ceiling. Live 10,485,765-byte code-heavy fixture prompted, then rendered full
  heading/code content without truncation. Evidence:
  [large confirmation](screenshots/large-confirm.png),
  [large rendered](screenshots/large-rendered.png).
- Local GIF transport is now enabled but renderer decodes only first frame through
  `createImageBitmap` and converts it to PNG before display, preventing animation.
  Full rendering fixture exposed PNG/JPEG/GIF/WebP/SVG entries in live WKWebView;
  first-frame conversion has source regression coverage.
- Reader theme chooser offers closed System/Light/Dark choices only. Native state
  persists override, reapplies validated tokens, and leaves System following live
  window appearance. Isolated live choice switched to Light and saved `"light"`.
  Evidence: [light theme](screenshots/theme-light.png). Import option opens native
  JSON picker; validated Zed families add safe named choices while arbitrary CSS/
  script values stay blocked. Imported `Safe dark` path/name persisted and restored
  after restart. Evidence: [imported theme](screenshots/imported-theme.png).
- Latest checkpoint verified debug build, 78 Rust tests/clippy and 25 web tests/build.
  Release/package checks above predate these latest native changes; existing
  generated package is not evidence for the current source.

## Launch checks

- `--help`, `--version`, unsupported input, multiple input, missing path, and
  explicit-file validation passed.
- Explicit-file launch remained running and printed `mdvr: accepted ...`.
- Frontmost/process launch was confirmed. Packaged Finder-style file-open events
  reuse active process and window.
- Post-fix window visibly renders `readme.md` through Orca. Relative Markdown
  navigation and reload after navigation passed. Action controls, history, and
  other desktop scenarios remain unaccepted.

## Unverified desktop behavior

The following remain unverified against a live embedded WKWebView: resize and
clipping, automated picker text filtering and broader focus transitions,
cross-block selection and rendered/source clipboard behavior, reload selection/locator
preservation, non-search action bridge execution, CSP enforcement, Mermaid async output,
remote resource consent and automated dynamic system-appearance switching.

Unit and Bun tests cover the corresponding pure/core behavior but do not close
these app-evidence rows.

## Security and policy gaps

Local canonical-root/symlink policy has automated tests and live same-root
resource transport evidence. Embedded hostile HTML/SVG sanitization passed live
fixture plus source regressions. Outside-root consent, remote consent, DNS
resolution and destination/connect-race protection, and actual HTTP fetch/redirect
handling remain unverified.

## Performance and distribution

Performance distributions were not measured on the required 2020 M1 MacBook Air
8 GB baseline. Intel/x86_64 and universal builds are blocked by the available
arm64-only target/toolchain. Unsigned arm64 app and compressed DMG were built;
read-only DMG mount passed full bundle/Mach-O/framework/icon inspection. Original
project icon is generated into ICNS. Bundle minimum macOS 11.0 matches release
Mach-O `LC_BUILD_VERSION` and is enforced by inspection. No clean-machine test,
signing, notarization, or Gatekeeper evidence exists.

Signing/notarization credentials and packaging environment were not created or
assumed. These are release blockers, not passes.
