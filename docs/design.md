# mdvr design and parallel implementation plan

Status: approved product decisions; implementation contracts proposed below.

This document consolidates the design interview. No application implementation
exists yet. Proposed paths and interfaces below are coordination boundaries, not
claims about existing code.

## 1. Product and fixed constraints

`mdvr` is a read-only GUI Markdown viewer inspired by Glow's local-file
discovery and reading workflow.

- macOS only. The minimum OS version follows the selected dependencies; record
  their actual requirements before publishing a deployment target.
- Rust application with a **GPUI shell and a Wry-hosted WKWebView document
  surface**. No Electron. GPUI is fixed, not a candidate to replace silently.
- All features identified as v1 below are required. A feasibility spike is a
  milestone, not a reduced release scope.
- Only AI coding agents write or edit source code. Humans may review, test,
  approve, and request changes. Dependencies and generated code are allowed.
- Reuse Zed code where useful and legally compatible. Check individual crate,
  asset, grammar, and theme licenses at pinned revisions. Do not assume Zed or
  community themes are uniformly MIT-licensed.
- The user accepts applicable Zed crate license obligations. The repository
  currently has an MIT `LICENSE`; distribution licensing still requires an
  audit. Do not silently relabel imported code or assume the current license
  covers it.
- Native macOS chrome, minimal toolbar, proportional prose, monospace code,
  generous spacing. No editor gutters or line numbers.
- VoiceOver support is not a v1 requirement. Keyboard operation, visible focus,
  legibility, and selection/copy remain required.

### Superseded scraps

Do not implement a custom serializable Rust `Document` tree, duplicate Rust
Markdown parser, GPUI widget-per-block document renderer, mandatory tree
diffing, or Tree-sitter integration merely because the scraps proposed them.
WKWebView owns the whole document, including HTML, diagrams, math, selection,
and search highlights. Full reparse on file change is the starting point;
benchmark before introducing incremental parsing.

## 2. Launch and window behavior

| Entry point                 | Required behavior                                                                                                                   |
| --------------------------- | ----------------------------------------------------------------------------------------------------------------------------------- |
| `mdvr`                      | Show a progressively populated Markdown picker rooted at the invoking shell's current directory. Do not restore a document instead. |
| `mdvr FILE`                 | Open that file, even if discovery would exclude it.                                                                                 |
| `mdvr DIR`                  | Show the picker rooted at that directory.                                                                                           |
| Finder opens a file         | Open that file; browsing root is its parent directory.                                                                              |
| Dock launch                 | Restore the previous document/root when available. First launch asks for a folder.                                                  |
| Explicit open while running | Reuse the existing application window and replace the active document.                                                              |

There is one window and one active document, not tabs or multiple document
windows. Closing/reopening the window must remain compatible with Dock and CLI
activation.

CLI v1 supports `--help` and `--version`. A missing or unreadable explicit path
produces stderr and a nonzero exit. The launcher must receive an acknowledgment
of explicit-open success/failure rather than claiming success merely because the
GUI process started. No stdin, remote URL arguments, multiple paths, or general
Glow flag compatibility.

**Implementation default:** a CLI file open starts browsing at the file's parent
directory, matching Finder. Relative arguments resolve against the invoking
shell's directory, never the GUI process's incidental working directory.

## 3. Required v1 behavior

Requirement IDs are stable references for tasks and verification. Splitting
ownership does not weaken any requirement.

### Discovery and picker

| ID  | Requirement                                                                                                                                                                                                                              |
| --- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| D1  | Recursively discover `.md` and `.markdown`, case-insensitively. Honor `.gitignore`; skip hidden files/directories by default. Do not traverse symlink directories. Explicit file opens bypass discovery exclusions, not security checks. |
| D2  | Scan off the UI thread and publish progressive results. Cancel obsolete scans when the root changes. Update the list when files appear/disappear.                                                                                        |
| D3  | Fuzzy-match relative paths, not contents. Empty query sorts alphabetically by path. Directories are not selectable file entries.                                                                                                         |
| D4  | Arrow keys select, Enter opens, Escape returns to the document when one exists. `⌘P` opens the picker. Folder action changes the browsing root. Empty/no-match states offer “Choose folder.”                                             |

### Document rendering and reading

| ID  | Requirement                                                                                                                                                                                                                                                                                               |
| --- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| R1  | CommonMark plus tables, task lists, strikethrough, autolinks, and footnotes. Task checkboxes are read-only. Soft line breaks render as spaces.                                                                                                                                                            |
| R2  | GitHub-style heading anchors, including duplicate-heading suffixes; anchor behavior needs explicit compatibility fixtures.                                                                                                                                                                                |
| R3  | Render sanitized HTML, including headings, tables, lists, links, images, formatting, and `<details>/<summary>`. Strip document scripts, event handlers, iframes, forms, embedded objects, and document CSS.                                                                                               |
| R4  | Render Mermaid diagrams and `$…$` / `$$…$$` TeX math using bundled assets. Invalid inputs produce useful rendering errors without hanging the application. No CDN dependencies.                                                                                                                           |
| R5  | Render PNG, JPEG, GIF, WebP, and SVG. Relative image paths resolve from the document directory. Unsupported/failed images show a placeholder. Static-first-frame GIF is the documented v1 default; animation was not separately approved. SVG is untrusted content, not a script or network escape hatch. |
| R6  | Highlight popular programming languages using a bundled highlighter; a web highlighter is allowed. Unknown languages remain plain code. See the initial language set below. No runtime grammar downloads.                                                                                                 |
| R7  | Center prose at roughly 80 characters maximum width. Persist adjustable text size (`⌘+`, `⌘-`, `⌘0`). Wide tables/code blocks scroll horizontally.                                                                                                                                                        |
| R8  | Include a collapsible heading outline with click-to-jump. Hide it automatically on narrow windows.                                                                                                                                                                                                        |
| R9  | Support cross-block selection. `⌘C` copies selected rendered text with useful line breaks; `⌘A` selects the full rendered document, excluding app chrome. Separate “Copy Markdown” copies original source. Code-block copy copies exact code without fences.                                              |
| R10 | Search rendered document text, including code: literal, case-insensitive by default, case-sensitive toggle, highlights, next/previous, wraparound. Enter/Shift-Enter navigate matches; Escape closes. No regex or cross-file content search.                                                              |

Initial highlighting baseline from the interview: Python, JavaScript/TypeScript,
JSX/TSX, Bash, SQL, Rust, Go, C/C++, Java, C#, Ruby, PHP, Swift, Kotlin,
HTML/CSS, JSON, YAML, TOML, and Markdown. Confirm support and common fence
aliases in the selected library; do not silently drop languages.

### Navigation and reload

| ID  | Requirement                                                                                                                                                                                                                                                                             |
| --- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| N1  | Picker opens and local Markdown links replace the current document. Maintain back/forward history with reading positions. Local `#anchor` links jump within the document.                                                                                                               |
| N2  | Allow explicit local Markdown navigation outside the browsing root. Broken links leave the current document intact and show an error.                                                                                                                                                   |
| N3  | Explicit HTTP/HTTPS clicks open the default browser; `mailto:` opens the default mail app. Image links open the default image viewer. Other local files require confirmation before opening with macOS. Block executable files and unapproved URL schemes; never invoke shell commands. |
| N4  | Watch the current file, debounce save bursts, re-read/reparse on change, and handle atomic replacement saves. Never reparse merely because a frame rendered or the window resized.                                                                                                      |
| N5  | Preserve reading position by nearby heading/block and offset, not scroll percentage alone. Never steal focus on reload. Preserve selection if selected content is unchanged; clear it only when affected. This must be demonstrated, not assumed from WebKit reload behavior.           |
| N6  | Missing/unreadable file after a successful render: keep the last good render, display error, and retry when the file returns. An empty file renders the empty-document state, not stale content.                                                                                        |

### Themes, state, and commands

| ID  | Requirement                                                                                                                                                                                                                                                                                                   |
| --- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| U1  | Ship light/dark defaults; follow macOS appearance unless overridden. Import local Zed theme JSON and switch between themes in a family. Map reader colors and syntax tokens; ignore editor-only fields. Apply themes to HTML, Mermaid, and math. Invalid themes preserve the current theme and show an error. |
| U2  | Persist browsing root, last document, reading position, theme, text size, and window size/position. Explicit launch intent overrides restoration. History and search query are session-only.                                                                                                                  |
| U3  | Provide the shortcuts below, a command palette, and predictable focus restoration. No configurable keymap in v1.                                                                                                                                                                                              |
| U4  | Startup open failures show the failed path with Retry, Choose file, and Browse folder. Never silently substitute another document. Preserve the previous root when a restored file is missing.                                                                                                                |
| U5  | Empty/unreadable directories, parse/render failures, invalid themes, denied resources, and failed images have explicit recoverable states. A failed operation does not destroy the current valid view.                                                                                                        |

| Shortcut           | Action                                |
| ------------------ | ------------------------------------- |
| `⌘O`               | Choose file                           |
| `⇧⌘O`              | Choose browsing folder                |
| `⌘P`               | File picker                           |
| `⌘F`               | Document search                       |
| `⌘[` / `⌘]`        | Back / forward                        |
| `⌘R`               | Reload                                |
| `⌘W`               | Close window                          |
| `⇧⌘P`              | Command palette                       |
| `⌘+` / `⌘-` / `⌘0` | Increase / decrease / reset text size |

Picker, search, and palette temporarily own focus; closing them restores
previous focus. `⌘A`/`⌘C` retain ordinary text-field behavior while a
picker/search input has focus. Palette covers remaining actions, including theme
switching, outline visibility, Copy Markdown, and heading navigation.

## 4. Security and resource policy

The document is untrusted. “Local file” does not imply permission to execute
code or read arbitrary paths. All paths to resources—including HTML, SVG,
Mermaid, math output, redirects, and the message bridge—must enforce the same
policy.

| ID  | Requirement                                                                                                                                                                                                                                                                                                                                          |
| --- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| S1  | Only bundled application scripts run. Sanitize document HTML and renderer-generated content as appropriate. Prevent script/event-handler injection, document CSS, automatic navigation, subframes, forms, embedded objects, and uncontrolled network loads. Use defense in depth, including WebKit policy and a restrictive content security policy. |
| S2  | Automatically load local images only within the browsing root. Resolve relative paths from the document directory and resolve symlinks before checking root membership. Outside-root images require explicit permission. Merely opening an outside-root Markdown document is not permission to auto-read all adjacent files.                         |
| S3  | Remote images are blocked by default. Offer explicit per-document “Load remote images.” Consent lasts only for that document session and never survives app restart. Consent does not authorize scripts or other remote content.                                                                                                                     |
| S4  | Fetch remote images only over HTTP/HTTPS, without browser cookies or stored credentials. Block loopback, private-network, and link-local destinations, including IPv6 equivalents, DNS results, and redirects. Enforce finite download-size, redirect, and timeout limits. Failed images offer retry.                                                |
| S5  | Web content cannot read arbitrary files, launch arbitrary applications, or invoke shell commands through the bridge. Validate message type, payload, current document generation, and navigation target in the native host. A supplied path or claimed “user gesture” is not authority by itself.                                                    |
| S6  | Route explicit links through the native policy from N2/N3. Do not grant `file://` directory-wide access or permit WebKit's default navigation to bypass the resource broker. Treat SVG external references and renderer extension hooks as resource requests too.                                                                                    |

**Implementation defaults to freeze in the spike:** scope outside-root
permission to an explicitly approved resource for the current document session;
revoke document consent when navigating away; use a nonpersistent WebKit data
store. These narrow defaults implement the agreed policy without creating a
permission-management product.

The platform/resource owner selects a bounded local-resource mechanism and
numeric network limits during the spike, records them here or in the shared
contract, and provides adversarial tests. Do not use sanitization alone as a
network sandbox. If the selected APIs cannot enforce this policy, stop and
report.

## 5. Architecture and data flow

```text
CLI / Finder / Dock
        |
        v
Rust application state + GPUI shell
  |       |           |
  |       |           +-- persistence / themes
  |       +-------------- file discovery / watching / navigation
  |
validated, narrow message bridge
  |
WKWebView document surface
  +-- bundled Markdown parser + sanitizer
  +-- bundled syntax highlighter / Mermaid / TeX renderer
  +-- document layout / selection / search / outline / reading position
  |
resource requests --> native resource policy --> authorized bytes or denial
```

### Ownership of state

- **Rust:** launch intent, browsing root, current path and source revision,
  navigation history, file watching, preferences, theme selection, resource
  grants, OS link dispatch.
- **Web renderer:** Markdown parsing, DOM, heading index, text selection, search
  matches, block/heading reading locator, diagram/math rendering.
- **GPUI shell:** application chrome, picker, dialogs, palette, action routing,
  focus handoff, progress/error surfaces.
- **Shared only as messages:** source, document identity/revision, theme tokens,
  reading position, outline entries, validated user intents, render status. No
  shared mutable document tree.

Parse on source changes, cache between changes, and cancel obsolete work. Prefer
a maintained parser/sanitizer/highlighter over custom implementations. Choose an
existing worker-capable parsing path where possible; DOM/layout work necessarily
remains on the web view's thread. Lazy-render diagrams/math and avoid stale
completions after navigation.

Full reparse does **not** require destroying the whole web view or losing
selection. The spike must establish a render-update strategy that satisfies N5.
Introduce targeted DOM reconciliation only where needed for correctness or
demonstrated performance; do not build a generic diff framework.

## 6. Shared contracts: freeze before parallel integration

These are semantic contracts, not a prescribed wire format. The integration
owner publishes one concrete bridge definition and example payloads after the
spike. Do not maintain independently invented Rust and JavaScript protocols. Use
one small checked source of truth plus serialization tests; no general-purpose
RPC framework.

| Contract          | Producer → consumer      | Minimum contents and guarantees                                                                                                                             |
| ----------------- | ------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Launch request    | CLI/platform → app       | Explicit intent (picker/file/restore), caller-resolved absolute path/root; acknowledgment or structured error.                                              |
| Discovery update  | Files → shell            | Root/scan identity, discovered relative paths, completion/error; old-root batches are discarded.                                                            |
| Load document     | App → renderer           | Document identity, monotonic generation, Markdown source, optional anchor/reading locator. No unrestricted filesystem capability.                           |
| Render result     | Renderer → app           | Matching identity/generation, ready/error, heading index. Stale results cannot replace the active view or history state.                                    |
| Reading locator   | Renderer ↔ app           | Heading/block identity and local offset, with a documented fallback when content disappeared. Selection restoration is renderer-owned and transient.        |
| Navigation intent | Renderer → app           | Link target and source generation; native host resolves and authorizes it. No shell command strings.                                                        |
| Resource exchange | Renderer ↔ native broker | Opaque document-scoped resource identity, permitted request kind, bounded response or denial. Renderer never chooses arbitrary native file-read operations. |
| Appearance        | App → renderer           | Resolved reader/syntax tokens and text scale, not arbitrary imported CSS/JavaScript.                                                                        |
| Actions           | Shell ↔ renderer         | Closed set of actions for search, copy, select-all, outline/anchor navigation, and position capture/restore; focus ownership explicit.                      |

Use acknowledgments where ordering matters: capture position before navigation,
reject stale reloads, restore after matching render is ready. Pending reads,
parses, and resource loads must not outlive their authority when the
document/root changes.

Required shared fixtures: duplicate headings, mixed Markdown/HTML, nested lists
and code, cross-block selection, relative links, atomic-save reload, denied
resources, malicious HTML/SVG/Mermaid, and malformed math. Contract changes
require consumer acknowledgment before merge.

## 7. Parallel agent work plan

### Coordination rules

1. **One owner per file area.** Agents may read everything but write only their
   assigned area. Paths below are proposed; the spike establishes the actual
   layout once. Use modules first, not one crate per agent.
2. **One integration owner controls shared files:** `Cargo.toml`, dependency
   lockfiles, web package manifest/lockfile, application entry
   point/composition, shared bridge contract, `docs/design.md`, and root
   build/CI configuration. Other agents request changes rather than racing to
   edit them.
3. **Independent tasks use separate branches/worktrees when available.** Do not
   copy unrelated changes or rewrite another agent's work. Merge small, tested
   increments.
4. **Dependencies are explicit gates.** A task can build against agreed fixtures
   after contract freeze, but cannot claim end-to-end completion until its
   producer/consumer is integrated.
5. **No silent scope/stack changes.** Report blockers with reproduction, failed
   requirement, options, and required decision. Especially stop on a
   GPUI/WKWebView blocker.
6. **Each handoff includes:** commit/diff, requirement IDs delivered, exact
   files changed, commands/results, manual evidence where necessary,
   contract/dependency requests, and known gaps. “Implemented” is not
   verification evidence.
7. **Tests live with their owner** or in an explicitly assigned test file. A
   test agent should not create competing production implementations.

### Exclusive implementation lanes

| Lane                             | Proposed owned area                                                                      | Deliverables / requirements                                                                                                                    | Prerequisites                                                               |
| -------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------- |
| A: Integration + native platform | `src/main.rs`, `src/app.rs`, `src/platform/`, `contracts/`, shared manifests/build files | GPUI/WKWebView host, CLI/platform handoff, validated bridge, resource broker, S1–S6 native enforcement, shared contract and integration wiring | Spike first                                                                 |
| B: Files + navigation            | `src/files/`, `src/navigation.rs`                                                        | D1–D2; N1–N2, N4, N6 state/IO; launch-path validation; cancellation and watcher tests                                                          | Frozen native interfaces                                                    |
| C: Web document                  | `web/src/document/`, `web/src/reader.css`                                                | R1–R10, N5 renderer behavior, sanitizer/renderer security tests, reading locators and render status                                            | Frozen bridge/resource contract; web dependencies approved by A             |
| D: GPUI shell                    | `src/ui/`                                                                                | D3–D4, U3–U5 shell surfaces; picker/palette/dialogs; outline/search controls; focus and shortcut routing                                       | Frozen action/state interfaces; may use fixtures                            |
| E: Appearance + persistence      | `src/preferences.rs`, `src/theme.rs`, `assets/themes/`                                   | U1–U2, theme-token mapping, settings validation and safe writes; license provenance for bundled themes                                         | Frozen theme/position types                                                 |
| F: Verification + release        | `tests/e2e/`, `tests/fixtures/`, `scripts/verify/`, `packaging/`                         | Independent app-level acceptance checks, performance harness/evidence, universal DMG/signing plan, dependency/license audit                    | Fixture work can start immediately; app verification needs integrated build |

Cross-lane ownership is intentional: for example, B owns reload events, C owns
position/selection restoration, and A wires them. Assign a single integration
test owner (F) rather than letting every lane claim the feature complete
independently. C does not edit native policy; D does not invent a second parser;
E does not inject CSS into the reader.

### Dependency sequence

```text
M0: feasibility spike + actual dependency/license inspection (A; F can audit)
  |
  +-- blocked --> STOP + report; no toolkit substitution
  |
M1: freeze paths, bridge/actions, theme tokens, fixtures, dependency versions
  |
  +--> B: files/navigation --------+
  +--> C: web rendering -----------+
  +--> D: shell -------------------+--> M2: integrated feature-complete build
  +--> E: appearance/persistence --+          |
  +--> F: fixtures/release prep ---+          v
                                   M3: acceptance + security + performance
                                              |
                                              v
                                   M4: signed/notarized universal release
```

B–E can proceed simultaneously after M1. F prepares fixtures and licensing
evidence without waiting for them. Integration is continuous: merge individual
contract-compatible slices, not six unfinished branches at the end.

### M0: mandatory stop/go spike

Prove in a real macOS application:

- GPUI window embeds WKWebView with working layout, resize, focus, and keyboard
  input.
- Bundled Markdown/HTML, Mermaid, and TeX rendering works without CDN access.
- Cross-block selection, rendered copy, select-all, and exact code copy work.
- File reload preserves reading position and unchanged selection without
  stealing focus.
- Native link/resource policy and bridge validation block representative
  malicious inputs.
- Required platform APIs and dependency licenses are identified; minimum macOS
  and universal-build feasibility are documented.

Use the smallest end-to-end example; no speculative framework or full shell
buildout. If blocked, report the exact failing API/behavior, reproduction, and
options. **Do not replace GPUI, weaken security, or drop a requirement without
approval.**

## 8. Verification and performance gates

### Performance baseline

Baseline recommendation accepted: **2020 MacBook Air, Apple M1, 8 GB RAM**,
release build on a supported macOS version. Record actual OS, build revision,
dependency versions, and fixture characteristics. This is a benchmark target,
not a claim that the hardware is available.

| Scenario                                   | Target                                                        |
| ------------------------------------------ | ------------------------------------------------------------- |
| Cold launch → readable 1 MB Markdown file  | Under 500 ms                                                  |
| File refresh after debounce → updated view | Under 200 ms                                                  |
| Reading/scrolling                          | Smooth 60 fps                                                 |
| Stress document                            | 10 MB remains usable; switching/cancellation stays responsive |
| Above 10 MB                                | Warn before loading; never silently truncate                  |

Report debounce duration separately so it cannot hide slow save-to-visible
latency. Distinguish first readable prose from completion of lazy diagrams/math.
Measure representative prose, code-heavy, table-heavy, and diagram/math-heavy
fixtures; bytes alone do not characterize rendering cost. Capture multiple runs
rather than one favorable sample. Targets remain provisional until measured on
the baseline hardware; no unmeasured performance claims.

Parsing must run off the UI thread where feasible; expensive diagram/math work
must be lazy and bounded. Set explicit rendering/resource budgets and surface
pathological content failures instead of freezing. Optimize measured bottlenecks
only. If approved targets cannot be met with the fixed stack, report evidence
and request a decision.

### Required automated evidence

- Discovery exclusions, extensions, symlink behavior, incremental results, stale
  scan cancellation.
- Launch precedence, path validation, explicit CLI failure acknowledgment,
  restored state, history/anchors.
- Atomic-save watching, deletion/reappearance, empty files, rapid
  navigation/reload stale-result rejection.
- Parser fixtures, heading IDs, code aliases/copy fidelity, invalid
  HTML/Mermaid/math handling.
- Canonical-path boundaries and outside-root grants; malicious HTML/SVG; blocked
  scripts/schemes/subresources; remote consent, DNS/redirect/private-address
  rules and limits.
- Invalid themes/settings preserve usable state; imported themes cannot
  introduce executable content.
- Bridge payload validation and serialization compatibility.

Use the smallest suitable test setup for the selected stack. Every nontrivial
production behavior needs a runnable regression check; avoid redundant
per-function ceremony.

### Required real-app evidence

Run the actual GPUI/WKWebView app, not only a standalone browser harness. Verify
file/Dock/CLI handoff, picker, navigation, cross-block copy, code/source copy,
full-document select-all, focus restoration, search, outline, all image formats,
HTML, Mermaid, math, theme switching, reload position/selection, and error
recovery. Screenshots alone do not prove interaction behavior or security.
Record commands, observations, and failures; missing hardware or credentials are
blockers, not passes.

Release completion requires all v1 requirement IDs accounted for, no unapproved
gaps, passing integrated checks, performance evidence, license audit, and
packaging verification. F reports findings; implementation owners fix their own
areas unless ownership is explicitly reassigned.

## 9. Distribution and explicit exclusions

Required distribution: signed, notarized, universal macOS application via DMG,
with a bundled `mdvr` CLI and an explicit installation action. Do not modify
shell configuration or install the launcher silently. Manual updates only.

Not in v1: Electron; alternate toolkit fallback; tabs/multiple document windows;
editing/task toggling; VoiceOver work; terminal-style gutters; configurable
keymaps; regex/cross-file content search; theme marketplace/download service;
runtime grammar downloads; remote URL CLI inputs; stdin; multiple CLI paths;
general Glow flag compatibility; automatic updates; telemetry; accounts;
mandatory incremental parsing; custom Rust document AST.

## 10. Open facts and bounded implementation decisions

These are not permission to drop required features:

| Item                                                    | Owner / resolution                                                                                                                                    |
| ------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------- |
| Apple Developer signing/notarization credentials        | User availability unconfirmed. A local unsigned build can validate development, but does not satisfy release delivery. F reports any signing blocker. |
| Actual minimum macOS and universal architecture support | A records requirements of pinned GPUI, WebKit integration, and other dependencies during M0.                                                          |
| Concrete web libraries and Zed reuse                    | A and C select maintained compatible libraries; F audits licenses. No presumed Zed API or blanket license claim.                                      |
| Exact bridge encoding/resource transport                | A freezes during M1 after proving platform behavior; consumers acknowledge.                                                                           |
| Network/render budgets and debounce                     | A/C/B respectively choose finite values, document them, and test boundary behavior.                                                                   |
| Settings storage format/location                        | E uses a small native or conventional macOS settings mechanism with safe writes; no configuration framework needed.                                   |
| GIF animation                                           | Static-first-frame default; revisit only on explicit request.                                                                                         |
| Benchmark hardware availability                         | F records actual hardware and gaps relative to the accepted M1 baseline.                                                                              |
| Distribution licensing versus existing MIT file         | F supplies per-component evidence and required notices/source obligations before release; no silent license rewrite.                                  |

## 11. Agent task template

Copy this into an implementation assignment; fill concrete paths after M1:

```text
Lane / owner:
Requirement IDs:
Allowed write paths:
Read-only dependencies:
Shared contract revision:
Prerequisite gate and evidence:
Deliverable:
Required automated checks:
Required real-app checks:
Out of scope:
Stop/report conditions:
Handoff: diff/commit, exact checks/results, evidence, unresolved gaps.
```

Shared-file or contract changes go through A. Product decisions go back to the
user. Keep this document authoritative by having its owner record approved
changes rather than allowing parallel agents to rewrite requirements
independently.
