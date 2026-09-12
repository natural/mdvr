# 007 integration evidence — native composition slice

Status: **integrated feature candidate; release blockers remain in 008**.

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
  local loads only after successful bounded reads. Non-Markdown images open through
  `NSWorkspace`; other regular local files require exact-path confirmation, while
  directories and executable-mode files are blocked. History/current state remains
  unchanged on failed or stale loads.
- GPUI render maps search/focus/copy/select-all through `ShellState`;
  copy/select-all require renderer or search-input focus. Startup load failures
  retain the failed path and expose Retry, Choose file, and Browse folder; empty
  picker results expose Choose folder. Runtime navigation/render/theme failures
  keep the current view and use a bounded, dismissible text-only alert.
- Bridge context updates on document commit. Queue actions are checked again at
  dispatch, so queued actions from prior document/generation are rejected.
  Renderer scroll/navigation emits validated generation-bound reading locators;
  native state updates current history and atomically persists the latest locator.
  Dock restoration, reload, and back/forward apply it only after matching source
  mount, with block/heading/start/end fallback. No bridge action grants path or
  filesystem authority.
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

`src/platform/remote_policy.rs` and `remote_fetch.rs` implement native remote-image
transport. It accepts credential-free HTTP(S) only; rejects malformed schemes,
fragments, userinfo, and overlong URLs; resolves each destination before connect;
rejects every private, loopback, link-local, carrier-grade NAT, documentation,
benchmark, multicast, reserved, unspecified, broadcast, transition, or mapped-private
result; and pins the HTTP client to those validated socket addresses to prevent a
validation/connect race. Redirects are disabled in the client and followed manually
only after resolving and validating each target. Proxy discovery, cookies, and browser
credentials are disabled. Limits remain 2 KiB URL, five redirects, 8 MiB decoded body,
10 seconds, supported static-image MIME types, and four in-flight requests.

The first remote image opens explicit consent for the current document only. Consent
is never persisted and resets when document identity changes. Fetches run off the GPUI
thread; stale-generation results are discarded. Failed images expose keyboard/click
retry. Tests cover URL/address policy, private DNS results before connect, MIME policy,
redirect/response limits, and renderer request/retry shape. Live packaged-app evidence
confirms the consent alert: [remote consent](screenshots/remote-consent.png).

## Explicit non-claims and gaps

Remaining integration evidence gaps are listed in `008-acceptance.md`; this file no
longer treats completed picker, renderer, bridge, appearance, resource, or remote-image
work as unintegrated slices.

## Verification

```text
cargo fmt --check                         passed
cargo check --locked                      passed; 1 upstream future-incompat warning
cargo test --locked                       passed; 83 tests
cargo clippy --locked --all-targets -- -D warnings
                                           passed; 1 upstream future-incompat warning
cd web && bun install --frozen-lockfile    passed; 135 packages
cd web && bun test tests                   passed; 31 tests
cd web && bun run build                    passed
```

Current live interaction evidence and remaining blockers are tracked in
`008-acceptance.md`.
