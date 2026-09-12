# 007 — Native policy and continuous integration (lane A, M2)

**Gate:** 002; integrate contract-compatible slices from 003–006 as they arrive.
**Writes:** `src/main.rs`, `src/app.rs`, `src/platform/`, `contracts/`, shared
manifests/lockfiles, web entry point, root build/CI config and A's evidence.
**Requirements:** S1–S6 native enforcement, N3, launch/window behavior; end-to-end
wiring of every D/R/N/U requirement. Other owners fix their own files.

## Native implementation

- Explicit launch requests resolve at caller cwd and receive success/failure ACK.
  Missing/unreadable explicit paths print stderr and exit nonzero. Support --help,
  --version, reject unsupported/multiple inputs, reuse running app's single window.
  Finder root is file parent; Dock restoration differs from bare CLI picker intent.
  Test IPC ownership/validation, concurrent requests, timeout and app startup failure.
- Own WKWebView lifetime, trusted bundled assets, nonpersistent data store,
  CSP/navigation delegates and validated bridge. Block automatic navigation,
  subframes/forms/objects and unapproved schemes; do not grant directory-wide file://.
- Broker resources with canonical-root checks after symlink resolution, bounded
  bytes and opaque identities. Outside-root permission is one approved resource
  for current document session. Revoke permissions and pending work on navigation.
- Remote images require per-document consent. Credential-free HTTP(S) fetches
  validate DNS destinations and every redirect, prevent validation/connect races,
  block private/loopback/link-local IPv4/IPv6, and enforce M1 numeric limits.
  Consent never permits other remote content. Denial/failure is recoverable.
- Route trusted explicit links: local Markdown to B, anchors to C, HTTP(S)/mailto
  to native handlers, images to image viewer. Confirm other local files; block
  executable files and unapproved schemes. Never invoke a shell.

## Integration order

1. B successful load → C render-ready → committed current document/history; failures
   leave old valid state. Discard stale reads/render results at every boundary.
2. D picker/open/search/palette/actions → A routing → B/C; focus returns predictably.
3. B reload → C position/selection-preserving update; deletion retains last render,
   empty source replaces it. Capture position before history transitions.
4. E appearance/restore → A precedence → D/C; restore matching locator after ready.
5. Complete resource-consent UI, errors, Dock/Finder/CLI activation and window reopen.

Add macOS CI only after local pinned build works: locked native/web builds,
format/lint/tests and contract serialization checks. F owns app-level acceptance
scripts; CI passing without a desktop cannot claim real-app coverage.

## Gate

Run all native/web checks and F's smoke scenarios after each merge. M2 is a
feature-complete candidate only when every requirement has an owner and integrated
implementation, not merely six branches reporting success. F then runs full 008.
Record exact failures; A coordinates fixes with owners. Stop for unenforceable
policy or incompatible contract changes; no allow-all fallback or toolkit switch.
