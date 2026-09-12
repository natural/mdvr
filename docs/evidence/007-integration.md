# 007 integration evidence — native composition slice

Status: **bridge dispatch slice implemented; M2 not passed**.

## Implemented

- `src/app.rs` opens one GPUI window and owns `ShellState` plus
  `Option<EmbeddedWebView>` in one root view.
- `EmbeddedWebView` is attached while GPUI builds that window, not during
  repeated renders. The field retains WebKit view and navigation delegate until
  root-view drop; `Drop` clears the delegate and removes/releases native
  objects.
- App invokes `EmbeddedWebView::load_initial_document()` after attachment.
  Native code loads production `web/dist/index.html` with its hashed JS/CSS
  assets from packaged `Contents/Resources/web` or dev-checkout fallback.
  Page readiness is now tracked through `didFinishNavigation`; generation-tagged
  source, appearance, and bridge-context updates queue until renderer functions
  exist, with stale generations discarded.
- Root GPUI element has no interactive overlay children. `ShellState` starts
  with renderer focus; this slice does not route picker/search/palette input.
- Existing nonpersistent WebKit store and restrictive production CSP remain in
  force. `loadFileURL:allowingReadAccessToURL:` limits reads to the canonical
  app-owned web directory; navigation allows only its entrypoint and cancels
  external or other local navigations.
- Native bridge exposes a typed queue for action and navigation envelopes. GPUI
  drains actions and renderer navigation requests; `NavigationState` resolves
  anchors, local Markdown, external URLs, and non-Markdown targets, and commits
  local loads only after successful bounded reads. History/current state remains
  unchanged on failed or stale loads.
- GPUI render maps search/focus/copy/select-all through `ShellState`;
  copy/select-all require renderer or search-input focus.
- Bridge context updates on document commit. Queue actions are checked again at
  dispatch, so queued actions from prior document/generation are rejected.
  No bridge action grants path or filesystem authority.
- Native appearance propagation validates contract revision-1 `Appearance`
  tokens before JSON encoding `window.mdvrApplyAppearance(...)`. Document
  generations reject stale appearance updates while allowing repeated updates
  for current generation.
- Bundled renderer exposes `window.mdvrApplyAppearance(tokens)` with exact
  field/role allowlists, bounded scale/syntax counts, opaque hex-color checks,
  and fixed CSS custom-property names only. It imports no theme CSS or JS and
  never assigns arbitrary style text.

## Local resource authorization core

`src/platform/resource_policy.rs` now provides the pure-Rust local policy core,
wired into `src/platform/mod.rs` without adding dependencies or exposing a
path-based read API. It canonicalizes root, document, and each resource after
symlink resolution; denies outside-root resources by default; issues opaque
resource IDs scoped to document/generation; supports one-resource explicit
grants; revokes all grants on root/document/generation changes; and enforces a
finite local byte ceiling both before grant and before read.

Adversarial unit tests cover symlink escape, traversal, root and generation
changes, stale grants, missing/unreadable paths, same-root allow versus
outside-root deny/explicit grant, and size-limit changes after authorization.

## Remote policy core

`src/platform/remote_policy.rs` is wired into `src/platform/mod.rs` as a pure
policy/data layer. It accepts credential-free HTTP(S) only; rejects malformed
or unsupported schemes, fragments, userinfo, and overlong URLs; blocks private,
loopback, link-local, unspecified, and mapped-private IP literals; and carries
finite defaults of 2 KiB URL, five redirects, 8 MiB response, and 10 second
request timeout. `authorize_redirect` validates every target before allowing a
redirect under the count limit. No HTTP client or arbitrary network operation
was added.

Tests cover public URLs, credentials, schemes, malformed URLs, IPv4/IPv6
private/loopback/link-local literals, redirect limits, response limits, and
finite timeout configuration.

Explicit gaps: no remote consent flow; no DNS resolution or DNS destination
validation; no validation/connect-race protection; no socket, fetch, response
read, or redirect-following integration. Native handler accepts only validated
contract actions into an internal queue; no arbitrary file-read operation was added.

## Explicit non-claims and gaps

This is not full picker, file, launch IPC, `DocumentLoad` bridge, Markdown
renderer, remote-consent, DNS/connect-race, or full document-load
integration. Appearance is exposed as a native helper but app-level preference
or system-appearance event wiring remains outside this slice. The bundled page
remains a static fixture and does not prove live WebKit policy, focus, resize,
close/reopen, selection, or clipboard behavior. No speculative Objective-C API
was added.

## Verification

```text
cargo fmt --check                         passed
cargo check --locked                      passed; 1 upstream future-incompat warning
cargo test --locked                       passed; 70 tests
cargo clippy --locked --all-targets -- -D warnings
                                           passed; 1 upstream future-incompat warning
cd web && bun install --frozen-lockfile    passed; 135 packages
cd web && bun test                         passed; 15 tests
cd web && bun run build                    passed
```

Desktop runtime observation remains pending. The source-level asset-loading and
page-readiness race is covered by implementation paths, but visible WebKit
rendering, click navigation, history, and reload behavior still require runtime
evidence.
