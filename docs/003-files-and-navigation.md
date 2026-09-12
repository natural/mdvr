# 003 — Files, discovery and navigation (lane B)

**Gate:** 002 acknowledged native interfaces and fixtures. Runs beside 004–006.
**Writes:** `src/files/`, `src/navigation.rs`, colocated tests.
**Reads:** A's app/platform/contracts; F's fixtures. No shared manifest edits.
**Requirements:** D1–D2, N1–N2, N4, N6; launch validation and U4 error data.

## Implementation slices

1. Path/load API: resolve CLI paths against caller cwd; distinguish directories,
   explicit files, missing/unreadable paths and empty content. Explicit files bypass
   discovery exclusions, not resource policy. Return structured results to A's
   launch acknowledgment flow. Above 10 MB requires confirmation, never truncation.
2. Background discovery: `.md`/`.markdown` case-insensitive, `.gitignore`, hidden
   exclusions, no symlink-directory traversal. Prefer an existing ignore-aware walker.
   Publish bounded progressive batches tagged with root/scan ID; cancel obsolete work.
   Directory watching updates additions/removals without blocking GPUI.
3. Navigation state: one current document; history captures reading locator before
   transition, supports forward/back and anchors. Resolve local Markdown links from
   current document, allow explicit outside-root navigation, commit only successful
   loads. Non-Markdown/remote links go to A's policy, never a shell.
4. Watch current file and parent as needed for atomic replacement; debounce save
   bursts, document chosen duration separately from refresh latency. Re-read only
   on file change/manual reload. Cancel reads on navigation and reject stale results.
   Keep last good source/view on deletion or errors; retry when file returns.
   Empty file is a successful new empty document.

## Regression checks

Use temporary directories for nested ignores, uppercase extensions, hidden files,
symlink directories and explicit excluded opens. Assert progressive batches,
root replacement cancellation, add/remove updates and unreadable-root distinction.
Test relative paths, missing explicit file errors, failed navigation preserving
history, anchors, history truncation after a new branch, deletion/reappearance,
atomic rename saves, rapid navigation/save races and zero-byte content.

Run `cargo test --locked` plus fmt/clippy once integrated by A. F verifies actual
CLI acknowledgment and visible reload/history behavior in 007/008; module tests
alone do not satisfy N5. Request dependency additions through A.

**Handoff:** exact diff/files, requirement IDs, test commands/results, debounce,
contract revision and unverified app behavior. Stop on ambiguous root/grant or
history ordering semantics; never infer resource authority from path traversal.
