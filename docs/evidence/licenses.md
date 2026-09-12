# Dependency and asset license evidence

Lane F preparation record. This is an audit of currently locked metadata and repository assets, not legal advice or a claim that metadata alone proves license compliance.

## Facts observed

- Root application declares `MIT` in `Cargo.toml`; repository `license` file is the original MIT text with copyright `(c) 2026 Troy Melhase`.
- `Cargo.lock` is present and locked. `cargo metadata --locked --format-version 1` reported 590 package records: 1 workspace package and 589 registry packages.
- Direct dependency tree at this checkout is `mdvr v0.1.0 -> gpui v0.2.2`; `gpui` metadata reports `Apache-2.0` and repository `https://github.com/zed-industries/zed`.
- Cargo metadata reported license metadata for all 590 records: 0 missing both `license` and `license_file`. Reported expressions include MIT, Apache-2.0, BSD, Unicode-3.0, ISC, Zlib, MPL-2.0, LGPL-2.1-or-later, NCSA, BSL-1.0, CC0, Unlicense, and combinations. These are package metadata declarations, not independently verified license texts.
- No web runtime dependency is selected. `web/package.json` has only Bun `1.4.0` and a build script. No web lockfile exists. `web/index.html` is bootstrap HTML; `web/dist/index.html` and `web/dist/index-crv27xgv.js` are generated bootstrap output.
- No third-party renderer, grammar, Mermaid, TeX, theme, or font asset is currently bundled. No dependency license notices or source-offer files beyond repository `license` were found for a future release bundle.
- New `tests/fixtures/documents/assets/*` are tiny test fixtures, not copied project assets. Their provenance is not asserted as third-party licensed material. `security/hostile.svg` is authored test input. Do not ship fixture assets as product assets without separate review.

## Unknowns / release blockers

- Individual crate license texts, copyright notices, optional-feature contents, and Zed component provenance have not been inspected. `gpui`'s Apache-2.0 metadata does not establish licenses for every Zed component.
- No selected Markdown parser, sanitizer, highlighter, Mermaid renderer, math renderer, WebKit binding, theme, grammar, or production image asset exists to audit yet.
- Cargo registry source archives were not treated as proof of complete notice obligations. A release audit must inspect exact resolved sources and generate/retain notices using an approved process.
- Minimum macOS target, Intel build, universal packaging, signing, notarization, and Developer ID access remain unverified as documented in `docs/001-foundation-and-feasibility.md` and `docs/008-verification-and-release.md`.

## Reproduction commands and observed results

Run from repository root:

```sh
cargo metadata --locked --format-version 1
cargo tree --locked --depth 1
shasum -a 256 Cargo.toml Cargo.lock license web/index.html web/dist/index.html web/dist/index-crv27xgv.js
find web -maxdepth 3 -type f -print | sort
```

Observed on 2026-09-12 checkout:

```text
cargo metadata: 590 package records; mdvr 0.1.0 license MIT; gpui 0.2.2 license Apache-2.0
cargo tree --locked --depth 1: mdvr -> gpui v0.2.2
web/package.json: bun@1.4.0; no dependencies; no lockfile
```

Recorded bootstrap asset hashes:

```text
web/index.html                         b07bb5fc25e9c623714ba0e04854bec17a593c80a24aa007161ec1c928c13273
web/dist/index.html                    64740e29792e00c8765da47130f92e111837e50f55f4610ac6d5a46a4d6db19b
web/dist/index-crv27xgv.js             e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
```

Before distribution, rerun metadata/tree against final lockfiles, inspect each selected component's source license and notices, and record exact bundled assets and obligations. Do not infer a blanket license from package name, repository, or GPUI metadata.
