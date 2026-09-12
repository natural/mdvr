# Document renderer lane C

`renderer.ts` is dependency-neutral core/scaffold for revision-1 `document.load` data. It preserves source/code text separately, emits explicit resource requests, sanitizes document HTML by allowlist, tracks heading/block locators, searches rendered and code text literally, preserves selection when selected text survives, and rejects stale generations.

## Deliberate gaps and dependency requests

- No CommonMark parser, syntax highlighter, Mermaid engine, or TeX engine is bundled here. Current parser is bounded fixture coverage; code remains escaped/plain; Mermaid and TeX expose finite pending/error states.
- Request approval for pinned, offline-compatible parser/sanitizer/highlighter/Mermaid/TeX assets before replacing hooks. Record versions, licenses, bundle sizes, and macOS/WebKit behavior in M0 evidence.
- `resolveResource` is the only path from document references to URLs. Native broker must supply it; absent approval produces image placeholders and no direct resource URL.
- `mountDocument` uses `DOMParser` plus `replaceChildren`; native/WebKit tests still must prove copy, focus, selection, reload, CSP, and policy behavior.
