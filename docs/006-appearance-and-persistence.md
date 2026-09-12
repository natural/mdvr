# 006 — Appearance and persistence (lane E)

**Gate:** 002 appearance/position/state types. Runs beside B, C and D.
**Writes:** `src/preferences.rs`, `src/theme.rs`, `assets/themes/`, colocated tests.
**Reads:** contracts; A launch precedence; C token consumption; D controls.
**Requirements:** U1–U2, R7 preference persistence; invalid-state recovery in U5.

## Implementation slices

1. Small validated preferences format in conventional macOS Application Support
   location. Store browsing root, last document, reading locator, theme choice,
   text size, window size/position. Use atomic safe writes; preserve prior usable
   settings on failures. Handle malformed/older settings without crashing.
2. Restore only on Dock intent. Explicit CLI picker/file/directory and Finder opens
   override stored document/root; never have preferences silently reopen a document
   for bare `mdvr`. History/search and document resource grants stay session-only.
3. Light/dark defaults following system appearance unless overridden. Import local
   Zed theme JSON, enumerate family members, map reader colors/syntax tokens and
   ignore editor-only fields. Reject invalid inputs without changing active theme.
   Send validated tokens only, never imported CSS/JavaScript.
4. Persist chosen text scale and window geometry with safe bounds; recover windows
   on removed displays. Notify shell and renderer consistently, including Mermaid
   and math. No theme marketplace or generic configuration framework.
5. Bundle only license-audited assets; prefer original minimal defaults. Supply F
   exact source, revision, license and attribution for every imported theme.

## Checks and handoff

Round-trip settings, interrupted/failed writes, corrupt settings, explicit launch
overrides, missing restored document preserving root, transient history/search/
consent exclusion, invalid theme preserving current theme, family switching,
malicious theme values and window/text bounds. Run Rust fmt/clippy/tests.
F verifies restart, appearance changes and imported themes across native and web UI.

Deliver diff/files, IDs, settings location/schema, token revision, tests/results,
asset provenance and remaining app evidence. Request shared type/dependency changes
through A. Stop on uncertain asset rights or missing token semantics; no silent
license rewrite or arbitrary style injection.
