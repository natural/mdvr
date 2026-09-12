# Acceptance matrix

Statuses distinguish implementation/tests from required live or release evidence.
`pass` means current requirement evidence is complete on available arm64 hardware;
`partial` names remaining proof; `blocked` requires unavailable hardware/credentials.

| Requirement | Implementation / automated check | Real-app evidence | Status |
| --- | --- | --- | --- |
| D1–D2 | `src/files/mod.rs`: extension, ignore, hidden/symlink, progressive/cancel/rescan tests | `picker.png`, `picker-watch.png` | pass |
| D3–D4 | `src/ui/mod.rs`, `src/app.rs`: fuzzy order, selection, no-match, picker-return paths | `picker-filter.png`, `picker-keyboard-open.png`, `picker-return-*.png` | pass |
| R1–R2 | `web/tests/document.test.ts`: GFM, Unicode/duplicate heading fixtures | `rendering-formats.png`, `outline.png` | pass |
| R3–R4 | sanitizer, malformed math/Mermaid, finite-budget tests | `hostile-content.png`, `rendering-formats.png` | pass |
| R5 | format, broker, SVG, GIF-first-frame tests | `local-resource.png`, `rendering-formats.png` | pass |
| R6 | language/alias and unknown-fence tests | `rendering-formats.png` | pass |
| R7–R8 | scale bounds/persistence and responsive CSS checks | `text-scale.png`, `outline.png`, `full-screen.png` | pass |
| R9 | exact source/code/rendered-copy and document-only selection tests | `code-copy.png`, `source-copy.png`, `rendered-copy-select-all.png` | pass |
| R10 | literal/case/wrap/highlight tests | `search.png` | pass |
| N1–N4 | transactional history/anchor/load/watcher/stale tests | `navigation-reload.png`, `history.png`, `picker-history.png` | pass |
| N5 | block locator, offset, unchanged/affected selection and persistence tests | `reload-position.png`; dedicated live unchanged-selection proof absent | partial |
| N6 | delete/reappear/empty/last-good tests | live delete failures/reappearance logged; shared alert shown in `recoverable-error.png` | pass |
| U1 | defaults/import validation, family selection, persistence, appearance-generation tests | `theme-light.png`, `imported-theme.png`; automated system switch not live-recorded | partial |
| U2 | atomic preferences, precedence, geometry, locator capture/restore tests | `preferences-appearance.png`, `window-geometry.png`; persisted locator restart not live-recorded | partial |
| U3 | closed shortcut/focus/action tests | search, palette, picker, history, text-scale evidence | pass |
| U4–U5 | retained failed path, retry/chooser/no-match/status checks | `recoverable-error.png`; startup three-action surface not live-recorded | partial |
| S1 | CSP, sanitizer, navigation, bridge and restricted-file WebKit probe | `hostile-content.png` | pass |
| S2 | canonical root, symlink, grant scope/revocation/size tests | `outside-resource-consent.png`, `outside-resource-approved.png` | pass |
| S3–S4 | per-document consent, pinned DNS addresses, redirect/limit/MIME/retry tests | `remote-consent.png`; integrated fetched-image completion not live-recorded | partial |
| S5–S6 | closed/stale bridge, broker, native scheme/local-file routing tests | hostile/resource/local-file consent evidence | pass |
| Launch §2 | argument/precedence/Finder/Dock/window/request-file ACK tests | Finder/Dock/CLI reuse and real app-load ACK observed | pass |
| Performance §8 | release renderer probe and `measure-reload.py` | M2 Pro provisional cold/reload results; M1 baseline and frame capture unavailable | blocked |
| Distribution §9 | read-only DMG rendered arm64/x86 under Rosetta with fresh homes; CI packaging | physical Intel/separate clean host, Developer ID/notary profile unavailable | blocked |
| Licenses | deterministic web + remote-fetch notices, zero listed missing texts | full GPUI/native graph and legal review incomplete | partial |

Exact commands, measurements, screenshots, blockers, and caveats live in
`008-acceptance.md`; this matrix does not upgrade unit checks into desktop proof.
