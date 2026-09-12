# 008 — Independent verification and release (lane F, M3–M4)

**Start now:** fixture and license preparation beside 001. **App gate:** 007 M2.
**Writes:** `tests/e2e/`, `tests/fixtures/`, `scripts/verify/`, `packaging/`,
`docs/evidence/008-acceptance.md`, `docs/evidence/licenses.md`.
**Reads:** all implementation/contracts; no competing production implementations.
**Requirements:** independent coverage of D1–D4, R1–R10, N1–N6, U1–U5, S1–S6,
design §2 launch behavior and §9 distribution.

## Early parallel work

- Create deterministic fixtures: duplicate/Unicode headings, mixed Markdown/HTML,
  nested lists/code, every language/alias, relative links, all image formats,
  malformed math/Mermaid, hostile HTML/SVG, outside-root/symlink resources,
  atomic-save/delete/reappear scenarios and stale navigation races.
- Build requirement matrix: ID → implementation owner → automated check → real-app
  evidence → status/blocker. Split shared IDs by responsibility, never count an
  unintegrated unit test as whole-feature acceptance.
- Audit exact locked crates/packages/assets/grammars/themes, license texts,
  notices and source obligations. GPUI's Apache-2.0 declaration does not license
  every Zed component. Preserve original MIT license; escalate distribution conflicts.
- Inventory actual hardware, SDKs, Intel target availability and signing access.
  Do not print secrets, create credentials or treat their absence as a pass.

## Integrated acceptance

Run actual GPUI/WKWebView, not only a standalone browser. Verify CLI/Finder/Dock,
ACK failures, one-window reuse/reopen, picker, history/anchors, selection, rendered/
source/code copy, document-only select-all, all shortcuts and focus restoration,
search/outline, HTML/images/Mermaid/math, themes, persistence, reload and recoverable
errors. Exercise unchanged and affected selection, heading deletion, atomic saves,
missing-file recovery and empty source. Record observations, not only screenshots.

Adversarial app tests verify blocked document scripts/CSS, frames/forms/navigation,
SVG references, Mermaid hooks, schemes, forged/stale bridge messages, resource
revocation, symlink escapes, outside-root consent, remote consent reset, DNS/private
addresses/redirects, credential absence and all finite limit boundaries. A/C fix
security failures in their owned areas. Never weaken tests to pass implementation.

## Performance

Release builds; record commit, OS, dependencies, hardware, fixture bytes and content
mix. Baseline target: 2020 M1 MacBook Air, 8 GB. Different hardware gives provisional
results only. Use repeated cold/warm runs and report distributions for prose,
code-heavy, table-heavy and diagram/math-heavy fixtures.

- 1 MB cold launch → readable prose <500 ms; report lazy completion separately.
- Refresh after debounce <200 ms; report debounce and total save-to-visible too.
- Smooth 60 fps reading/scrolling; collect frame evidence, not subjective claims.
- 10 MB remains usable; switching/cancellation stay responsive.
- Above 10 MB asks before loading; no silent truncation.

Record unmet targets and request decisions; never claim baseline results without
baseline hardware. Optimize measured bottlenecks only.

## Packaging and release gate

1. Build arm64 and x86_64 with pinned tools and resolved macOS minimum; combine
   Mach-O slices, inspect every embedded binary/library and bundle offline assets.
2. Create .app metadata/file associations, icons with provenance and bundled CLI.
   Provide explicit launcher installation action; no silent PATH/shell edits.
3. Produce DMG; verify clean-machine file/CLI/Dock launch, offline assets and Intel/
   Apple Silicon operation. Manual updates only.
4. With available user credentials, sign nested components, enable appropriate
   hardened runtime settings, notarize, staple and validate Gatekeeper acceptance.
   Record signing/notarization blockers if credentials are unavailable; unsigned
   development artifact is not release completion.

M4 requires all matrix rows passing with no unapproved gaps, license obligations
fulfilled, architecture/package checks, real-app/security/performance evidence and
signed/notarized universal DMG. Handoff exact artifacts/checks/results and remaining
blockers. Owners repair failures; F independently reruns failed acceptance cases.
