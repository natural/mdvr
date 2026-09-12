# Document renderer

Production renderer for revision-1 `document.load` data.

- `markdown-it` + footnote/task-list plugins provide CommonMark/GFM features.
- DOMPurify sanitizes browser output; pure Bun tests use bounded fallback sanitizer.
- `highlight.js` uses bundled common grammars and preserves original code in `CodeBlock.source`.
- KaTeX renders bounded inline/display math. Mermaid is bundled and completed lazily through `renderMermaidAsync`.
- `resolveResource` remains only image/SVG resource URL authority; denied resources stay placeholders.
- Heading IDs, block locators, literal search, selection preservation, and generation rejection remain renderer-owned APIs.
- `main.ts` mounts production output, exposes `mdvrLoadDocument`/`mdvrSearchDocument`, and rejects stale Mermaid completion.

`mountDocument` uses `DOMParser` and `replaceChildren`; embedded WebKit copy,
focus, reload, selection, and native resource-policy evidence remain host-level
verification gaps, not Bun DOM-test claims.
