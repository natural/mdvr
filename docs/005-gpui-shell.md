# 005 — GPUI shell and commands (lane D)

**Gate:** 002 frozen state/actions/focus contracts; fixture adapters allowed.
**Writes:** `src/ui/` and colocated tests. **Reads:** A/B state, C outline/search
messages, E preferences. A owns composition and shared dependencies.
**Requirements:** D3–D4, U3–U5; controls for R7–R10, N1/N3 and U1.
Runs beside B, C and E; never builds a second document renderer.

## Implementation slices

1. Progressive picker with fuzzy relative-path matching, alphabetical empty query,
   files only, keyboard selection/Enter, Escape to current document, Choose folder
   on empty/no-match states. Preserve usable selection as batches arrive.
2. Minimal native chrome, file/folder actions, back/forward availability, responsive
   outline, search controls, theme chooser and permission/error surfaces.
   Non-Markdown local opens require confirmation through A; no direct OS dispatch.
3. Command palette and all fixed shortcuts: ⌘O, ⇧⌘O, ⌘P, ⌘F, ⌘[/⌘], ⌘R, ⌘W,
   ⇧⌘P, ⌘+/⌘-/⌘0. Palette includes theme family switching, outline visibility,
   heading navigation and Copy Markdown. No configurable keymap.
4. Focus ownership: picker/search/palette temporarily own focus and restore prior
   owner on close. ⌘A/⌘C operate on focused inputs normally; otherwise target
   rendered document only. Visible focus and keyboard operation are required.
5. Recoverable startup error shows exact failed path with Retry, Choose file and
   Browse folder. Preserve root on missing restored file. Render/discovery/theme/
   resource errors retain valid view and offer appropriate retry/consent actions.
   Wire remote consent and outside-root grants through native authority, not JS flags.

## Checks and handoff

Test fuzzy ordering, empty/no-match transitions, selection through progressive
updates, command dispatch, focus owner restoration, disabled history actions and
error recovery using frozen fixtures. Run Rust fmt/clippy/tests. F verifies each
shortcut and focus transition in real GPUI/WKWebView, including closing/reopening
window and ⌘A/⌘C inside text fields versus document.

Deliver diff/files, IDs, contract revision, commands/results and remaining native
interaction checks. Stop if an overlay/shortcut cannot coexist with embedded view;
request A's platform fix rather than replacing GPUI or swallowing document input.
