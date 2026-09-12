# 002 — Freeze contracts and release parallel lanes (M1)

**Owner:** A. **Depends on:** 001 real-app feasibility evidence. **Writes:**
`contracts/`, `src/app.rs`, shared entry points/manifests/lockfiles/build config,
`docs/design.md`, `docs/evidence/002-contract-freeze.md`.
**Consumers:** B, C, D, E; F owns shared acceptance fixtures.

## Deliverable

One small checked protocol source plus serialization fixtures. Choose concrete
encoding during M0, not independently in each lane. Generated bindings are fine;
a general RPC framework is not. Publish revision, example valid/invalid payloads,
file ownership, native interfaces and test commands before B–E start.

Freeze:

- Launch intent: picker/file/restore, caller-resolved absolute path, invocation
  identity, explicit-open acknowledgment/error and timeout behavior.
- Discovery: root/scan ID, relative paths, batch/completion/error; reject obsolete
  scans. State interfaces distinguish successful empty results from unreadable roots.
- Document load/result: document identity, monotonic generation, source, optional
  anchor/locator; staged success before committing history/current document.
- Position: heading/block identity plus offset, deleted-content fallback;
  capture acknowledgment before navigation and restore after matching ready.
  Renderer owns transient selection, not persisted native DOM state.
- Navigation: source generation, target, trusted native authorization path.
  A message's claimed user gesture or supplied file path conveys no authority.
- Resources: opaque document-scoped IDs, request kinds, bounded bytes/denial,
  revocation on navigation/root change; no arbitrary file-read operation.
- Appearance: reader/syntax tokens and scale, not imported CSS or JavaScript.
- Closed actions: search query/case/next/previous, copy source/code/rendered text,
  select-all, outline/anchor navigation, position capture/restore and focus ownership.
- Recoverable errors and progress: preserve last valid view, explicit retry actions,
  matching request/generation for asynchronous results.

A selects finite local/remote byte limits, timeout, redirect count, allowed MIME
handling and transport in M0. C supplies finite parsing/diagram/math budgets; B
supplies debounce duration. Record actual numbers and boundary fixtures here at
freeze, not arbitrary values that have not been tested.

Security contract must cover canonical paths and symlinks, per-resource outside-root
grants, document-session remote consent, credential-free HTTP(S), DNS and redirect
validation (IPv4/IPv6 private/loopback/link-local), SVG external references, CSP,
subframes, automatic navigation and stale asynchronous completions. Opening an
outside-root Markdown file never authorizes adjacent images.

## Checks and handoff

Round-trip every message and reject unknown kinds, malformed/oversized payloads,
stale generations and unauthorized resources. F supplies fixtures for duplicate
headings, mixed HTML, nested lists/code, relative links, selection, atomic saves,
malicious HTML/SVG/Mermaid and malformed math. Lane-local tests stay with owners.

B–E explicitly acknowledge the same revision and write paths in the freeze evidence.
Then start 003–006 concurrently; F continues 008 preparation. New contract/dependency
requests go to A and require affected consumer acknowledgment before merging.
Stop if a consumer must invent missing semantics; resolve the contract first.
