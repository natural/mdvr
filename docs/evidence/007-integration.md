# 007 integration evidence — native composition slice

Status: **smallest live composition slice implemented; M2 not passed**.

## Implemented

- `src/app.rs` opens one GPUI window and owns `ShellState` plus
  `Option<EmbeddedWebView>` in one root view.
- `EmbeddedWebView` is attached while GPUI builds that window, not during
  repeated renders. The field retains WebKit view and navigation delegate until
  root-view drop; `Drop` clears the delegate and removes/releases native
  objects.
- App invokes `EmbeddedWebView::load_initial_document()` after attachment.
  Native code loads bundled `web/index.html`, the existing offline fixture.
- Root GPUI element has no interactive overlay children. `ShellState` starts
  with renderer focus; this slice does not route picker/search/palette input.
- Existing nonpersistent WebKit store, restrictive fixture CSP, and
  navigation-cancel delegate remain in force.

## Explicit non-claims and gaps

This is not full picker, file, launch IPC, `DocumentLoad` bridge, Markdown
renderer, resource broker, remote-consent, symlink/canonical-root, or
Objective-C message-handler integration. The bundled page remains a static
fixture and does not prove live WebKit policy, focus, resize, close/reopen,
selection, or clipboard behavior. No speculative Objective-C API was added.

## Verification

```text
cargo fmt --check                         passed
cargo check --locked                      passed; 1 upstream future-incompat warning
cargo test --locked                       passed; 31 tests
cargo clippy --locked --all-targets -- -D warnings
                                           passed; 1 upstream future-incompat warning
cd web && bun test                         passed; 9 tests
cd web && bun run build                    passed
```

Desktop runtime observation remains pending. Existing lane contracts and
module implementations remain unchanged by this slice.
