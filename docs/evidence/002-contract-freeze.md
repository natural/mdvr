# 002 contract evidence — M1 contract source-of-truth slice

Status: **implemented contract slice; M1 gate not passed**.

M0 runtime evidence remains open. This document records the checked Rust/JSON
shape so B–E can compile against one source, but it does not claim that GPUI,
WKWebView, native resource policy, or the renderer has validated these values.

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

No numeric timeout, redirect count, DNS/private-address rule, debounce value,
parser budget, or MIME policy is frozen here. `ack_timeout_ms` is carried as
caller intent only; M0/native code must define and enforce timeout behavior.

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
cargo test --locked                       passed (10 tests)
cargo clippy --locked --all-targets -- -D warnings
                                           passed
```

## M0 unresolved gate

M0 is still blocked/open for real-app evidence: GPUI/WKWebView resize, focus,
keyboard, close/reopen and activation; live CSP/navigation behavior; selection,
copy and reload position preservation; production parsing/rendering; bridge
runtime probes; canonical-path and symlink enforcement; outside-root grants;
remote consent, DNS/redirect/private-network blocking and finite network
limits; malicious HTML/SVG/Mermaid checks; dependency license audit; and
universal Intel build feasibility. Therefore this slice is **not** an M1 pass,
not a claim of native policy enforcement, and not release evidence.
