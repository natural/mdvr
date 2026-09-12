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
  buttons move wrapped literal matches, Match case controls comparison, and Escape
  closes and restores document focus.
  Live WKWebView found and selected `GPUI`; revision-1 search actions reached native
  bridge context. Evidence: [search](screenshots/search.png).
- Every fenced block exposes an accessible Copy button. Live WKWebView copy of
  first `readme.md` block produced exact source
  `xcodebuild -downloadComponent MetalToolchain` on pasteboard; fallback uses a
  temporary exact DOM range only after Clipboard API failure. Evidence:
  [code copy](screenshots/code-copy.png).
- Responsive Contents control builds links using safe text nodes from rendered
  heading IDs. At narrow widths CSS hides both outline and toggle, moves toolbar
  into an overflow-safe second row, and reserves reader space. Live outline expanded
  with `mdvr` and `Development`; selecting
  `Development` drove validated anchor navigation and changed scroll position.
  Evidence: [outline](screenshots/outline.png).
- Reload captures first visible stable block plus viewport offset and exact
  selection text before DOM replacement, restores matching block offset, and
  recreates unchanged selection across text nodes without moving focus. Renderer
  scroll/navigation now also emits closed generation-bound locators; native state
  updates current history and atomically persists them, while Dock startup,
  back/forward, and reload restore only after matching source mount. Live
  document stayed at paragraphs 26–Target after editing paragraph 0 and reload.
  Evidence: [reload position](screenshots/reload-position.png). Unchanged-selection
  restoration retains source regression coverage; dedicated live selection proof remains open.
- Directory launch now starts ignore-aware discovery off GPUI thread, applies
  bounded progressive batches to native picker state, and opens selected files
  into same window. Missing GPUI `font-kit` feature caused invisible text; enabling
  it fixed native text while keeping platform defaults disabled. Live directory
  picker listed four Markdown files and opened `first.md`. Evidence:
  [picker](screenshots/picker.png), [opened document](screenshots/picker-open.png).
  Native focus plus Down/Up and Return keyboard handling also passed: Down then
  Return opened `image.md`. Clicking the picker then typing `ban` changed the visible
  query and reduced three entries to `banana.md`. Evidence:
  [keyboard open](screenshots/picker-keyboard-open.png),
  [picker filter](screenshots/picker-filter.png). ⌘P now retains the current
  navigation/document state while showing the picker; Escape recreated the embedded
  viewer and restored the same `design.md` document rather than discarding it.
  Evidence: [picker over document](screenshots/picker-return-open.png),
  [picker Escape return](screenshots/picker-return-document.png). Native picker
  now routes ⌘O/⇧⌘O without inserting shortcut characters, toggles back to the
  document on ⌘P, and uses ⌘R to retry a failed startup path. Selecting
  `docs/design.md` from that picker preserved prior `readme.md` as enabled Back
  history even though WKWebView was recreated; readiness queuing retained control
  state. Evidence: [picker history](screenshots/picker-history.png).
  While picker remains open, one-second background rescans atomically replace its
  entries while retaining selected path when present. Live removal of `a.md` and
  addition of `c.md` produced `b.md`/`c.md` without blocking. Evidence:
  [picker watch](screenshots/picker-watch.png).
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
  Bundled CLI now validates path/type/readability/UTF-8/size then delegates through
  LaunchServices. Two CLI opens acknowledged absolute paths, reused one PID/window,
  and second visibly replaced first; invalid paths exit 2 before dispatch. Launcher
  now sends a private bounded request file, waits up to 10 seconds, and prints accepted
  only after app consumes the request and acknowledges successful load. Live bundled
  CLI returned `mdvr: accepted /private/tmp/mdvr-local-link/doc.md` only after current
  app opened that document; private request/ack cleanup and timeout are fail-closed.
- Hostile live fixture strips scripts, event handlers, CSS, frames, forms and
  orphan form inputs. Local SVG bytes now pass renderer SVG sanitizer before Blob
  creation, removing scripts, handlers and every href/xlink external reference.
  Live WKWebView showed only safe text/image output. Evidence:
  [hostile content](screenshots/hostile-content.png).
- Files above 10 MiB now stop on visible full-load confirmation, including
  navigation/Finder/CLI replacement of an existing document; confirmed files
  retain permission across reload while all source paths enforce a 20 MiB hard
  ceiling. Live 10,485,765-byte code-heavy fixture prompted, then rendered full
  heading/code content without truncation. Evidence:
  [large confirmation](screenshots/large-confirm.png),
  [large rendered](screenshots/large-rendered.png).
- Local GIF transport is now enabled but renderer decodes only first frame through
  `createImageBitmap` and converts it to PNG before display, preventing animation.
  Full rendering fixture exposed PNG/JPEG/GIF/WebP/SVG, KaTeX, and asynchronously
  completed Mermaid with themed readable labels in live WKWebView. Mermaid output
  keeps bundled strict-mode executable/href stripping while local SVG uses stricter
  DOM allowlist. Evidence: [rendering formats](screenshots/rendering-formats.png).
- Reader theme chooser offers closed System/Light/Dark choices only. Native state
  persists override, reapplies validated tokens, and leaves System following live
  window appearance. Isolated live choice switched to Light and saved `"light"`.
  Evidence: [light theme](screenshots/theme-light.png). Import option opens native
  JSON picker; validated Zed families add safe named choices while arbitrary CSS/
  script values stay blocked. Imported `Safe dark` path/name persisted and restored
  after restart. Evidence: [imported theme](screenshots/imported-theme.png).
- Copy Markdown writes exact current source, with fallback restoring prior DOM
  ranges and focus. Live `readme.md` copy pasted beginning `# mdvr` plus original
  following prose. Evidence: [source copy](screenshots/source-copy.png).
- Open File/Open Folder controls and ⌘O/⇧⌘O request native `NSOpenPanel`; renderer
  supplies no path authority. Folder choice tears down current WebView/watcher and
  starts progressive picker for new root; file choice queues transactional open.
  Native folder-only dialog opened live. Evidence:
  [open controls](screenshots/open-controls.png).
- ⇧⌘P opens keyboard-focused command palette with native open/picker, copy,
  outline, and theme actions; Escape restores renderer focus. Plain ⌘P returns to
  current-root picker. Live palette exposed all fixed actions. Evidence:
  [command palette](screenshots/command-palette.png).
- Copy Rendered serializes rendered block text without toolbar labels; live fixture
  pasted `First\n\nOpen second`. ⌘A selects document root only unless input/select
  owns focus, then preserves native control behavior. Evidence:
  [rendered copy and select all](screenshots/rendered-copy-select-all.png).
- Renderer posts closed `render.ready` only after synchronous DOM mount; native
  bridge rejects stale document/generation and records commit-to-ready latency.
- Latest local verification passed 84 Rust tests/clippy, 32 web tests/build,
  restricted-file WKWebView probe, current release build, app inspection, and DMG
  inspection.

## Launch checks

- `--help`, `--version`, unsupported input, multiple input, missing path, and
  explicit-file validation passed.
- Explicit-file launch remained running and printed `mdvr: accepted ...`.
- Frontmost/process launch was confirmed. Packaged Finder-style file-open events
  reuse active process and window.
- Post-fix window visibly renders `readme.md` through Orca. Relative navigation,
  reload, controls, history, picker, clipboard, themes, Finder/Dock lifecycle,
  local resources, and hostile-content scenarios have live evidence above.
- Full-screen transition retained reader layout, all toolbar controls, document
  content, and scrolling without visible clipping. Evidence:
  [full screen](screenshots/full-screen.png).

- Recoverable-state integration now keeps failed startup paths and exposes Retry,
  Choose file, and Browse folder; empty/no-match picker states expose Choose folder.
  Native runtime failures retain the current document and use a bounded dismissible
  text-only alert. A broken Markdown link kept “Stable view,” showed the exact failed
  path, and offered Dismiss. Evidence: [recoverable error](screenshots/recoverable-error.png).

## Unverified desktop behavior

The following remain unverified against a live embedded WKWebView: arbitrary
small-window resize clipping, broader focus transitions, live
reload selection preservation, integrated remote-image completion after consent,
and automated dynamic system-appearance switching. Hostile-content and restricted-file startup probes
cover CSP-sensitive script/network paths, but no independent CSP report capture exists.

Unit and Bun tests cover the corresponding pure/core behavior but do not close
these app-evidence rows.

## Security and policy gaps

Local canonical-root/symlink policy has automated tests and live same-root
resource transport evidence. Embedded hostile HTML/SVG sanitization passed live
fixture plus source regressions. Outside-root reference resolved canonically,
showed exact native path consent, then granted only that resource for current
context; approval rendered it. Evidence:
[outside consent](screenshots/outside-resource-consent.png),
[approved resource](screenshots/outside-resource-approved.png).
Remote images are blocked until a per-document native alert is approved; live
packaged-app evidence confirms that alert: [remote consent](screenshots/remote-consent.png).
Native fetching rejects private, loopback, link-local, CGNAT, documentation,
benchmark, multicast, reserved, and transition destinations before creating a client
pinned to validated addresses; it disables proxy/cookies/automatic redirects,
revalidates manual redirects, limits time/body/count/concurrency, and accepts only
static-image MIME types. Automated policy/fetch tests pass; integrated fetched-image
rendering remains without live evidence. Local non-Markdown routing canonicalizes
before dispatch: images open through `NSWorkspace`, other regular files require
exact-path native confirmation, and executable-mode files/directories are denied.
Automated executable-policy coverage passes. Live text-file link showed canonical
exact path in native confirmation before dispatch; external-app completion remains
open. Evidence: [local file consent](screenshots/local-file-consent.png).

## Performance and distribution

Required 2020 M1 MacBook Air 8 GB baseline remains unavailable. Provisional
release measurement on MacBook Pro Mac14,10, M2 Pro, 16 GB used a 1,048,576-byte
single-code-block fixture across five cold process launches: 411.2, 426.2, 435.1,
482.4, and 505.6 ms from native document commit to validated `render.ready`
(median 435.1 ms; one run exceeded 500 ms). Lazy completion excluded. Measured
renderer cleanup removed per-block duplicate sanitization and linear scans while
retaining one full-document sanitization. The checked `scripts/verify/measure-reload.py` harness generated a 1,048,595-byte
prose fixture with 2,198 blocks and measured warm reload commit-to-ready times of
188.8, 179.6, 183.8, 179.8, and 181.0 ms (median 181.0 ms); observed save-to-ready
totals including poll/debounce were 331.9, 327.8, 318.0, 312.9, and 321.7 ms
(median 321.7 ms). This meets the <200 ms post-debounce target by median on the provisional M2 Pro,
not the unavailable M1 baseline. Intel/x86_64 and universal builds are blocked by available arm64-only target/toolchain. Unsigned arm64 app and compressed DMG were built;
read-only DMG mount passed full bundle/Mach-O/framework/icon inspection. Original
project icon is generated into ICNS. Bundle minimum macOS 11.0 matches release
Mach-O `LC_BUILD_VERSION` and is enforced by inspection. Deterministic notices
include license text for all 124 installed web packages plus 96 remote-fetch Cargo
packages and are bundled with project license; full native GPUI dependency review
and final legal review remain external. No clean-machine test,
signing, notarization, or Gatekeeper evidence exists.

`packaging/build-universal.sh` now fails closed unless both pinned Rust targets are
installed, then combines and verifies both Mach-O slices. `packaging/release.sh`
builds, hardened-runtime signs, notarizes, staples, and Gatekeeper-checks app and
DMG using explicit keychain identity/profile inputs. Dry-run and shell validation
pass. Execution remains blocked: x86_64 Rust std is absent, keychain has only an
Apple Development identity, and no `notarytool` profile exists. No credentials were
created or assumed; universal/signing/notarization remain release blockers, not passes.
