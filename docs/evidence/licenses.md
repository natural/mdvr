# Dependency and asset license evidence

Updated for production web renderer and remote-fetch dependencies. Metadata and
bundled license files are evidence, not legal advice. Full GPUI/native Cargo audit
remains outside this lane.

## Locked web dependencies

`web/bun.lock` is committed and resolves 134 package records. Direct runtime
packages and metadata observed from each locked `node_modules/*/package.json`:

| Package | Locked version | Declared license | License evidence |
| --- | ---: | --- | --- |
| `markdown-it` | 15.0.2 | MIT | `node_modules/markdown-it/LICENSE` |
| `markdown-it-footnote` | 4.0.0 | MIT | `node_modules/markdown-it-footnote/LICENSE` |
| `markdown-it-task-lists` | 2.1.1 | ISC | `node_modules/markdown-it-task-lists/LICENSE` |
| `dompurify` | 3.4.15 | `(MPL-2.0 OR Apache-2.0)` | `node_modules/dompurify/LICENSE`, `LICENSE-MPL` |
| `highlight.js` | 11.12.0 | BSD-3-Clause | `node_modules/highlight.js/LICENSE` |
| `mermaid` | 12.0.0 | MIT | `node_modules/mermaid/LICENSE` |
| `katex` | 0.18.7 | MIT | `node_modules/katex/LICENSE` |

Development-only type packages are locked as `@types/markdown-it` 14.2.0 (MIT)
and `@types/dompurify` 3.2.0 (MIT). They are not renderer runtime assets.

Mermaid brings its own locked runtime dependency graph, including parser,
Chevrotain, D3, Cytoscape, ELK, KaTeX, and DOMPurify packages. Their versions,
integrity hashes, and dependency relationships are recorded in `bun.lock`.
`third-party-notices.md` deterministically inventories 124 installed web packages
and 107 Cargo packages in pinned `reqwest` and `ignore` runtime graphs. Every listed
package includes discovered license/notice text with zero missing files.

## Bundling and network facts

- `web/src/main.ts` imports renderer, CSS, and all parser/highlighter/diagram/math
  modules. `bun build ./index.html --outdir dist --minify` bundles production
  dependencies; no CDN or runtime grammar fetch is used.
- `web/index.html` has `connect-src 'none'`, `default-src 'none'`, restrictive
  script/style policy, and no remote script/source URL. Approved remote images are
  fetched only by native pinned-address transport and returned as bounded bytes.
- DOMPurify sanitizes browser output; pure Bun tests use its small deterministic
  fallback because Bun test has no DOM. Renderer-generated Mermaid SVG is
  separately stripped of executable and external-reference attributes.
- No third-party theme, font, image, grammar, or copied fixture asset is
  shipped. Test fixtures remain test-only. `assets/icon.svg` is an original
  project asset created for mdvr and carries project MIT licensing.

## Unverified obligations and release blockers

- Generated notices require final human/legal review before public distribution;
  generation and package metadata are evidence, not legal advice.
- Package metadata and repository license files were inspected, but this is not
  independent legal verification of source provenance, optional package files,
  or bundled output obligations.
- DOMPurify has dual MPL-2.0/Apache-2.0 licensing; distribution must select and
  satisfy applicable terms.
- Minimum macOS/WebKit support, universal packaging, signing, notarization,
  and native resource-policy obligations remain outside web lane and open.

## Reproduction

```sh
cd web
bun install --frozen-lockfile
bun test tests
bun run build
cd ..
bun scripts/verify/generate-notices.mjs
! grep -q '| missing |' third-party-notices.md
```

Inspect resolved metadata and license files before release:

```sh
bun pm ls
for p in markdown-it markdown-it-footnote markdown-it-task-lists dompurify highlight.js mermaid katex; do
  node -p "require('./node_modules/$p/package.json').version + ' ' + require('./node_modules/$p/package.json').license"
  find "node_modules/$p" -maxdepth 1 -iname 'LICENSE*' -o -iname 'COPYING*' -o -iname 'NOTICE*'
done
```
