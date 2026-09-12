# 001 — M0 feasibility evidence

Date: 2026-09-12
Host: macOS 26.7 (25G229), arm64
Toolchain: rustc 1.98.1 (48a229cea 2026-09-01), cargo 1.98.1, Xcode 26.5 (17F42), macOS SDK 26.5

## Scope delivered

Smallest compilable GPUI/WKWebView embedding slice plus one offline,
deterministic document fixture. Fixture is not a Markdown parser and does not
claim production rendering.

- `src/app.rs` opens one GPUI window and owns one embedded view.
- `src/platform/mod.rs` obtains GPUI's AppKit `NSView`, creates `WKWebView`
  through the Objective-C runtime, uses `WKWebsiteDataStore`'s
  `nonPersistentDataStore`, loads production `web/dist` assets from the
  packaged `Contents/Resources/web` directory or dev-checkout fallback, and
  sets width/height autoresizing.
- Native delegate class conforms to `WKNavigationDelegate` and implements the
  SDK-documented `webView:decidePolicyForNavigationAction:decisionHandler:`
  selector. It cancels every navigation action. No unsupported selector is
  used.
- Delegate lifetime is retained by `EmbeddedWebView`; the weak WebKit delegate
  property is cleared before both native objects are released.
- No `WKScriptMessageHandler` or other JS/native message handler is registered;
  bridge surface is closed for this slice.
- `web/index.html` contains static HTML, code, Mermaid, and TeX placeholder
  sections. Inline nonce-authorized CSS/JS is covered by restrictive CSP:
  `default-src 'none'`, no network/media/frame/object/worker sources, and only
  the fixture nonce for inline script/style.
- Fixture JS provides literal case-insensitive section search with next/previous
  and Enter/Shift-Enter, code selection, and copy attempts with a host/clipboard
  failure status. It does not parse or render document formats.
- Cargo pins direct macOS bindings to `block = 0.1.6`, `cocoa = 0.26.0`,
  `objc = 0.2.7`, and `raw-window-handle = 0.6.2`; `Cargo.lock` is updated.

## API inspection

GPUI 0.2.2 source inspected at
`$HOME/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/gpui-0.2.2`.

- `gpui::Window` implements `raw_window_handle::HasWindowHandle`.
- GPUI's macOS `MacWindow` returns `RawWindowHandle::AppKit` containing its
  native content `NSView`.
- GPUI's public `Window` method with the same name returns an internal window
  handle, so embedding explicitly calls
  `raw_window_handle::HasWindowHandle::window_handle(window)`.
- `raw-window-handle` documents AppKit `NSView` access as main-thread-only.
  Attachment occurs during GPUI render on its foreground window path; no native
  view pointer crosses a thread boundary.

Local SDK inspection:

- `WKWebView.h`: `WKWebView : NSView`; designated initializer is
  `initWithFrame:configuration:`; `loadHTMLString:baseURL:` is available.
- `WKWebsiteDataStore.h`: `+nonPersistentDataStore` is available from macOS
  10.11.
- `WKWebView.h`: `navigationDelegate` and `setNavigationDelegate:` are
  available; it is weak.
- `WKNavigationDelegate.h`: the implemented three-argument policy selector is
  available from macOS 10.10.
- `xcrun --sdk macosx clang ... -framework WebKit` linked successfully in the
  bootstrap check.
- Final binary contains
  `/System/Library/Frameworks/WebKit.framework/Versions/A/WebKit` after the
  successful native build.

## Verification

Commands and results for this slice:

```text
cargo fmt --check
# passed

cargo build --locked
# passed; 0 errors, 1 warning
# warning: future-incompatibility notices for block 0.1.6 and proc-macro-error2 2.0.1

cargo clippy --locked --all-targets -- -D warnings
# pending final run after this edit

cargo test --locked
# pending final run after this edit

cd web && bun run build
# passed; Bundled 1 module; output web/dist/index.html
```

The Cargo warning is upstream future-incompatibility reporting, not an
application warning. Source compilation verifies Objective-C method signatures
and linkage. It does not prove delegate callback execution.

## Implemented checks vs unverified checks

| Check | Current evidence | Status |
| --- | --- | --- |
| Offline production bundle is loaded | `loadFileURL:allowingReadAccessToURL:` selects packaged `Contents/Resources/web` or dev `web/dist`; `bun run build` passes | source/build verified |
| HTML/code/Mermaid/TeX placeholders | Static sections and source text in `web/index.html` | implemented as placeholders only |
| Search/selection/copy hooks | Fixture JS handlers and visible status messages | implemented in fixture; WebKit interaction unverified |
| Restrictive CSP | Static nonce CSP in fixture; build passes | source/build verified; runtime enforcement unverified |
| Nonpersistent storage | `WKWebsiteDataStore::nonPersistentDataStore` | source/build verified; runtime storage behavior unverified |
| Navigation policy | SDK-documented delegate selector cancels actions | source/build verified; live callback and external-link denial unverified |
| Closed bridge | No script message handler is installed | source verified; hostile-page runtime probe unverified |
| Resize/clipping, focus/keyboard, close/reopen, activation | No desktop observation | unverified |
| Production Markdown/HTML/sanitizer/Mermaid/TeX/resource broker | Not implemented in A slice | unverified; owned by later lanes |
| Malicious-content, navigation-scheme, symlink, redirect, and resource-limit tests | Not implemented in A slice | unverified |
| Universal build feasibility | Not run | unverified |

## Remaining M0 gaps / blockers

M0 still lacks real-app evidence for resize/clipping, focus/keyboard input,
close/reopen, activation, selection/copy/search, reload position and selection
preservation, and live CSP/navigation behavior. Bundled Markdown/Mermaid/TeX
rendering, bridge validation, malicious-content tests, resource brokering,
dependency-license audit, and universal-build feasibility remain open. No
production parser or native interaction is claimed by this fixture.
