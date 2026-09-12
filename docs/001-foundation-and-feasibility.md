# 001 — Foundation and feasibility (M0)

## Status and execution order

`design.md` remains authoritative. This series is an execution plan, not a claim
that v1 or the spike has shipped. Repository preparation provides a GPUI bootstrap
window and a separately built static web asset, not an embedded document viewer.

```text
001 feasibility (A) ──> 002 contract freeze (A)
                              ├──> 003 files/navigation (B) ───────┐
                              ├──> 004 web document (C) ───────────┤
                              ├──> 005 shell (D) ─────────────────┤
                              ├──> 006 appearance/state (E) ──────┤
008 fixture/license preparation (F, starts now) ──────────────────┤
                                                                v
                                                    007 integration (A)
                                                                v
                                                    008 acceptance/release (F)
```

Numbers identify work packages, not a strictly serial schedule. B–E start only
after 002 passes; F prepares fixtures and audits concurrently with 001. A merges
small working slices throughout, rather than waiting for every lane to finish.

## Prepared baseline

- Single Rust binary crate, edition 2024; `gpui = =0.2.2`, default Linux/Windows
  features disabled, complete resolution in `Cargo.lock`.
- GPUI crate metadata declares Apache-2.0; this is not a transitive license audit.
  Existing lowercase `license` remains unchanged and covers original MIT code.
- Bun 1.4.0 bundles `web/index.html`; no runtime web dependencies selected yet.
  No web lockfile is generated while there are no dependencies. A must commit one
  when selecting renderer dependencies.
- Local tools: macOS 26.7 (25G229), arm64; Xcode 26.5 (17F42), SDK 26.5;
  Homebrew rustc/cargo 1.98.1; rustfmt 1.9.0; clippy 0.1.98; Bun 1.4.0.
- Installed missing Metal compiler using
  `xcodebuild -downloadComponent MetalToolchain` (17F42). GPUI compilation had
  failed with `cannot execute tool 'metal' due to missing Metal Toolchain`.
- Only `aarch64-apple-darwin` Rust target is currently installed; rustup is absent.
  Minimum supported macOS, Intel compilation/runtime, and universal distribution
  are not proven by this host build. Do not publish a guessed deployment target.

## Preparation verification

- `cargo build --locked`: passed; native debug binary linked.
- `cargo fmt --check`: passed.
- `cargo clippy --locked --all-targets -- -D warnings`: passed.
- `cargo test --locked`: passed with **zero tests**; bootstrap only.
- `cd web && bun install --frozen-lockfile && bun run build`: passed.
- Active Rust diagnostics: no errors; expected inactive non-macOS cfg hint.
- `git diff --check`: passed.
- Cargo reports upstream future-incompatibility warnings for `block 0.1.6` and
  `proc-macro-error2 2.0.1`. Track during dependency audit; not application errors.
- No real-app interaction, WKWebView embedding, Intel build, benchmark or signing
  evidence yet. These checks do not close M0.

## Assignment

**Owner:** A. **Requirements:** platform feasibility for R1–R10, N5, S1–S6;
launch/window behavior from design §2. **Writes:** `src/main.rs`, `src/app.rs`,
`src/platform/`, shared manifests/lockfiles/build config, `web/index.html`, future
`web/src/main.*`, `contracts/`, this plan and authoritative design. Spike-only
renderer experiments stay in `spike/`; transfer useful code to C at contract freeze.
F owns adversarial fixtures and license evidence, not competing platform code.

1. Run the bootstrap on a real desktop. Identify supported GPUI native-window
   access; embed WKWebView with main-thread lifetime and teardown safety. Prove
   resize, clipping, focus, keyboard input, close/reopen and activation.
2. Select/pin WebKit bindings and bundled parser, sanitizer, highlighter, Mermaid,
   and math libraries. Check actual API availability and individual licenses.
   Prefer maintained packages; no Rust Markdown AST, parser, or rendering framework.
3. Build the smallest offline document with HTML, code, Mermaid, and TeX. Prove
   cross-block selection, rendered copy, select-all, exact code copy, and source copy.
4. Demonstrate reload preserving heading/block position and unchanged selection
   without focus theft. Full parse is allowed; wholesale view replacement that
   loses required state is not. Test edited selection and deleted locator fallback.
5. Prove bounded resource transport, nonpersistent WebKit storage, restrictive CSP,
   delegate navigation denial and closed bridge validation. Test malicious HTML,
   SVG, Mermaid and stale messages against actual WebKit, not only browser mocks.
6. Inspect pinned dependencies for macOS floor, architecture support and license
   obligations. Prove Intel build feasibility, then freeze actual supported target.

## Exit / stop

Record API choices, exact commands, dependency versions, real-app observations,
and reproductions in `docs/evidence/001-feasibility.md` (A). M0 passes only when
all design §7 spike checks pass. Missing hardware/API enforcement is a blocker,
not a waiver. Stop on GPUI/WKWebView incompatibility or unenforceable security;
report failing requirement and options. Never substitute a toolkit or reduce v1.

All lane handoffs include diff/commit, exact files, delivered requirement IDs,
checks/results, interaction evidence, shared-contract requests and remaining gaps.
Only A edits shared files. Separate branches/worktrees per lane; no concurrent
writes to another lane. Product changes require user approval.
