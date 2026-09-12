# 004 — Web document renderer (lane C)

**Gate:** 002 bridge/resource/appearance contracts; M0 approved dependencies.
**Writes:** `web/src/document/`, `web/src/reader.css`, colocated web tests.
**Reads:** A's web entry point/bridge; F fixtures. A owns package manifest/lockfile.
**Requirements:** R1–R10, N5, renderer side of S1/S6, U1 and recoverable U5 states.
Runs beside B, D and E; no native policy or duplicate Rust parser.

## Implementation slices

1. Bundled parser/sanitizer pipeline: CommonMark, tables, read-only tasks,
   strikethrough, autolinks, footnotes; soft breaks become spaces. GitHub-compatible
   heading IDs include duplicate suffixes and mixed Unicode/HTML fixtures.
   Allow required sanitized HTML and details/summary; strip scripts, handlers,
   document CSS, forms, frames and objects. Native broker remains authoritative.
2. Bundled syntax highlighting with unknown fences plain. Verify every baseline
   language and common aliases from design R6, including JSX/TSX, C#, Swift,
   Kotlin, TOML and Markdown. Preserve exact original code separately for copy.
3. Bounded lazy Mermaid and inline/display TeX, useful invalid-input states,
   cancellation/generation checks, no CDN or runtime grammar fetches. Route all
   generated URLs through policy; sanitizer and renderer hooks cannot bypass it.
4. Brokered PNG/JPEG/GIF/WebP/SVG with placeholders/retry. GIF uses first frame;
   do not accidentally enable animation. SVG external references remain untrusted.
   Native host owns resource and remote-consent decisions.
5. Reader layout: proportional prose at roughly 80 characters, monospace code,
   horizontal table/code overflow, text scaling, responsive collapsible outline.
   Theme updates affect prose, syntax, Mermaid and math without source reparse.
6. Selection/copy and rendered-text search: cross-block selection, document-only
   select-all, exact code copy; literal case-insensitive default with case toggle,
   highlight/next/previous/wrap, Enter/Shift-Enter, Escape. Include code text.
7. Full reparse on source changes, reuse current view between changes. Preserve
   heading/block locator and local offset; restore unchanged selected content,
   clear affected selection only, never steal focus. Use M0's proven minimal
   DOM update strategy, not a generic diff framework. Reject stale async work.

## Evidence

Runnable web tests cover parser/anchor fixtures, every language alias, code fidelity,
malicious HTML/SVG/Mermaid, invalid math, search behavior, locator fallback,
selection unchanged/affected, stale completions and finite budgets. Pure DOM tests
are not WebKit evidence: F must run copy, focus, reload and security against the
embedded view. A supplies the build/test entry points after dependency selection.

Run `cd web && bun install --frozen-lockfile && bun run build` and the frozen web
test command. Record first readable prose separately from lazy completion.

**Handoff:** diff/files, IDs, exact tests, budgets, contract revision and WebKit gaps.
Stop if WebKit cannot preserve required selection or renderer dependencies cannot
be sandboxed; do not weaken N5/S1–S6 to make a browser fixture pass.
