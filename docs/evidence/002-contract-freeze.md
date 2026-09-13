# 002 contract evidence — M1 contract source-of-truth slice

Status: **implemented contract slice; M1 gate not passed**.

M0 runtime evidence remains open. This document records the checked Rust/JSON
shape so B–E can compile against one source, but it does not claim that GPUI,
Wry/WebKit, native resource policy, or the renderer has validated these values.

## Source of truth

- Rust source: `src/contracts/mod.rs`
- JSON encoding: `serde` + `serde_json`
- Envelope: `{ "revision": 1, "message": { "kind": ..., "payload": ... } }`
- Unknown envelope fields, payload fields, message kinds, enum values, and
  revisions are rejected.
- Canonical encoding is compact `serde_json::to_vec` field order. Pretty JSON
  files under `contracts/fixtures/v1/` are readable canonical examples; decode
  followed by encode produces canonical wire bytes.
- Contract revision: **1** (`CONTRACT_REVISION = 1`).

## Closed message families

`launch.request`, `launch.ack`, `discovery.request`, `discovery.batch`,
`discovery.complete`, `discovery.error`, `document.load`, `render.ready`,
`render.error`, `position.captured`, `position.restored`,
`navigation.request`, `navigation.result`, `resource.request`,
`resource.result`, `resource.revoked`, `appearance.update`, `action`, `error`,
and `progress`.

IDs use nonzero `u64` newtypes: invocation, root, scan, document, generation,
request, and resource. `Generation` and `ScanId` are compared through
`reject_stale_generation`, `reject_stale_scan`, and message `validate_for`
methods. A mismatched document is rejected. Position capture carries an
acknowledgment message; restore carries document and generation and is only
valid for matching ready state at integration time.

Navigation results distinguish `NativePolicy`, `NativeConfirmation`, and
`None`. Accepted navigation with `None` authority is rejected. Renderer input
contains no authority field: claimed gestures and supplied paths are not
permission. Native policy must still resolve and authorize targets.

Resources are limited to closed request kinds (`Image`, `SvgReference`,
`MermaidAsset`, `MathAsset`) and references. There is no arbitrary file-read
operation. Resource revocation is document/generation scoped.

## Checked boundaries

These are **serialization/contract safety ceilings**, not proven native policy
limits. Values marked provisional must not be presented as M0 performance or
security evidence:

| Constant | Value | Status |
| --- | ---: | --- |
| `MAX_TEXT_BYTES` | 4 KiB | provisional contract ceiling |
| `MAX_PATH_BYTES` | 4 KiB | provisional contract ceiling; canonical/symlink policy blocked on M0 |
| `MAX_SOURCE_BYTES` | 10 MiB | provisional; aligns with design warning threshold, parser budget unmeasured |
| `MAX_RESOURCE_BYTES` | 8 MiB | provisional response ceiling; transport/network policy unproven |
| `MAX_FRAME_BYTES` | 12 MiB | provisional JSON frame ceiling |
| `MAX_BATCH_ITEMS` | 256 | provisional progressive discovery batch ceiling |
| `MAX_HEADINGS` | 4096 | provisional renderer result ceiling |
| `MAX_SYNTAX_TOKENS` | 128 | provisional appearance payload ceiling |
| text scale | 50–300 percent | provisional appearance validation range |
| heading level | 1–6 | Common Markdown shape; renderer compatibility still open |

A-owned remote policy core freezes finite policy-data defaults outside wire
serialization: URL 2 KiB, five redirects, 8 MiB response, and 10 second
request timeout. It validates credential-free HTTP(S), rejects fragments and
malformed/unsupported URLs, blocks private/loopback/link-local IP literals,
and requires each redirect target to pass policy before redirect count is
consumed. `ack_timeout_ms` remains caller intent only.

The frozen contract remains transport-neutral. Later native integration now owns
per-document consent, DNS destination validation, pinned-address connection,
manual redirect following, bounded response reads, and static-image MIME policy.

## Fixtures

- Valid: `contracts/fixtures/v1/valid/`
- Invalid: `contracts/fixtures/v1/invalid/`
- Covered invalid cases: unknown payload field, unknown message kind, zero ID,
  unknown revision, and accepted navigation without native authority.
- Rust tests also exercise source/resource/frame/batch ceilings, round trips,
  stale generation/scan rejection, position capture/restore, and closed
  families.

## Verification

Ran in current checkout:

```text
cargo fmt --check                         passed
cargo test --locked                       passed (88 tests)
cargo clippy --locked --all-targets -- -D warnings
                                           passed
```

## Current consumer integration status

Revision 1 remains the shared contract for the native bridge, renderer, appearance,
reload, and navigation integration. Wry IPC carries the same JSON bytes into the
native bridge; custom-protocol asset loading and page readiness are host concerns.
The files/navigation core is now wired into
the app's navigation state at source level; live WebKit callback timing and
visual navigation evidence remain open.

## M0 unresolved gate

M0 is still blocked/open for real-app evidence: GPUI/Wry WebView resize, focus,
keyboard, close/reopen and activation; live CSP/navigation behavior; selection,
copy and reload position preservation; production parsing/rendering; bridge
runtime probes; integration of the tested local canonical-path/symlink policy
and outside-root grants; malicious HTML/SVG/Mermaid checks; full native dependency
license audit; and universal
Intel build feasibility. Therefore this slice is **not an M1 pass, not a claim
of complete native policy enforcement, and not release evidence.
