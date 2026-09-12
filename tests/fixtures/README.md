# Verification fixtures

Deterministic, small inputs for lane F. Paths are stable and fixtures contain no network dependency.

- `documents/rendering.md`: duplicate/Unicode headings, Markdown/HTML, lists, code, aliases, links, images, math, Mermaid.
- `security/`: hostile HTML/SVG and malformed math/Mermaid.
- `links/`: relative navigation and image resolution.
- `reload/`: before/after, empty, delete/reappear atomic-save sequence.
- `symlink-cases/`: symlink directory and outside-fixture symlink; preserve symlink identity.

Image files are deliberately 1×1 or equivalent tiny assets. Missing/unsupported image behavior uses referenced nonexistent paths; no fake placeholder asset is needed.
