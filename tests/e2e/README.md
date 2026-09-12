# Lane F acceptance scenarios

These scenarios are executable once GPUI/WKWebView integration exists. They name expected evidence, not passing results.

| Scenario | Fixture | Expected evidence |
| --- | --- | --- |
| heading anchors and selection | `../fixtures/documents/rendering.md` | duplicate suffixes, Unicode anchor, cross-block copy |
| rendering and sanitization | `../fixtures/security/hostile.md` | scripts/events/CSS/frames/forms/objects blocked; current view survives |
| bounded parser errors | `../fixtures/security/malformed.md` | math/Mermaid errors visible; no hang |
| local navigation/resources | `../fixtures/links/target.md` | relative Markdown/image resolution; missing image placeholder |
| atomic reload | `../fixtures/reload/` | before → after, delete keeps last good render, reappearance retries, empty is not stale |
| root boundaries | `../fixtures/symlink-cases/` | symlink directory not traversed; escape resource denied or consented explicitly |
| image formats | `../fixtures/documents/assets/` | PNG/JPEG/GIF/WebP/SVG load; missing image placeholder |

Run against actual app, not standalone browser. Record command, commit, OS, fixture bytes, observation, and blocker in `docs/evidence/008-acceptance.md` when integration lands.
