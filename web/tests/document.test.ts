import { expect, test } from "bun:test";
import {
  GenerationGate,
  normalizeLanguage,
  preserveSelection,
  rejectStale,
  renderDocument,
  renderMath,
  renderMermaid,
  restoreLocator,
  sanitizeGeneratedSvg,
  sanitizeHtml,
  searchRendered,
} from "../src/document/renderer";

const fixture = (path: string) =>
  Bun.file(new URL(`../../tests/fixtures/${path}`, import.meta.url));

test("renders duplicate and Unicode heading IDs", () => {
  const model = renderDocument(
    "# Duplicate Heading\n\n# Duplicate Heading\n\n# Café — 日本語",
  );
  expect(model.headings.map(({ id }) => id)).toEqual([
    "duplicate-heading",
    "duplicate-heading-1",
    "café--日本語",
  ]);
});

test("sanitizes hostile HTML and dangerous URLs by explicit allowlist", () => {
  const html = sanitizeHtml(
    `<script>alert(1)</script><div onclick="x" style="color:red"><strong>safe</strong></div><img src="file:///etc/passwd"><a href="javascript:alert(1)">bad</a><details open><summary>more</summary></details>`,
  );
  expect(html).toContain("<div><strong>safe</strong></div>");
  expect(html).toContain("<details open>");
  expect(html).not.toContain("script");
  expect(html).not.toContain("alert");
  expect(html).not.toContain("onclick");
  expect(html).not.toContain("style");
  expect(html).not.toContain("file:");
  expect(html).not.toContain("javascript:");
});

test("preserves exact code and keeps unknown fences plain", () => {
  const source =
    "```typescript\nconst value: number = 42;\n```\n\n```unknown\n<a>&\n```";
  const model = renderDocument(source);
  expect(model.codeBlocks[0]?.source).toBe("const value: number = 42;");
  expect(model.codeBlocks[0]?.language).toBe("typescript");
  expect(model.codeBlocks[1]?.source).toBe("<a>&");
  expect(model.codeBlocks[1]?.language).toBeNull();
  expect(model.html).toContain('data-highlighted="false"');
  expect(model.html).toContain("&lt;a&gt;&amp;");
});

test("recognizes baseline aliases without claiming highlighting", () => {
  for (const alias of [
    "py",
    "js",
    "ts",
    "jsx",
    "tsx",
    "bash",
    "sql",
    "rs",
    "go",
    "c",
    "cpp",
    "java",
    "csharp",
    "ruby",
    "php",
    "swift",
    "kotlin",
    "html",
    "css",
    "json",
    "yaml",
    "toml",
    "markdown",
  ]) {
    expect(normalizeLanguage(alias)).not.toBeNull();
  }
});

test("renders tables, read-only tasks, links, images, and literal code search", () => {
  const model = renderDocument(
    "| A | B |\n| --- | --- |\n| 1 | 2 |\n\n- [ ] read-only\n\n```rust\nfn answer() {}\n```\n\n![one](assets/one.png)",
  );
  expect(model.html).toContain("<table>");
  expect(model.html).toContain("disabled");
  expect(model.html).toContain('data-mdvr-resource="assets/one.png"');
  expect(model.resources).toEqual([
    { kind: "image", reference: "assets/one.png", alt: "one" },
  ]);
  expect(searchRendered(model, "ANSWER").map((match) => match.text)).toEqual([
    "answer",
  ]);
});

test("returns bounded Mermaid and math states", () => {
  expect(renderMermaid("flowchart LR\n  A --> B").status).toBe("pending");
  expect(renderMermaid("flowchart LR\n  A -->").status).toBe("error");
  expect(renderMermaid("not a diagram").status).toBe("error");
  expect(renderMath("x^2", true).status).toBe("pending");
  expect(
    renderDocument("Broken: $$ x^2").errors.some(({ kind }) => kind === "math"),
  ).toBe(true);
});

test("restores exact locator, heading fallback, and clears affected selection", () => {
  const before = renderDocument("# Stable\n\nselected content\n\n# Other");
  const after = renderDocument(
    "# Stable\n\nselected content\nchanged\n\n# Other",
  );
  const selection = {
    startBlockId: "paragraph-1",
    startOffset: 0,
    endBlockId: "paragraph-1",
    endOffset: 16,
    text: "selected content",
  };
  expect(preserveSelection(selection, after)?.text).toBe("selected content");
  expect(
    preserveSelection({ ...selection, text: "removed", endOffset: 7 }, after),
  ).toBeNull();
  expect(
    restoreLocator({ block: "missing", heading: "stable", offset: 2 }, after),
  ).toEqual({ blockId: "heading-1", offset: 2, reason: "heading" });
  expect(
    restoreLocator({ block: "paragraph-1", offset: 1 }, before).reason,
  ).toBe("exact_block");
});

test("rejects stale async render completion", async () => {
  const gate = new GenerationGate();
  gate.begin(1);
  const stale = rejectStale(gate, 1, async () => {
    gate.begin(2);
    return "old";
  });
  expect(await stale).toBeNull();
  expect(gate.accept(2, "new")).toEqual({ generation: 2, value: "new" });
});

test("F rendering fixture uses bundled parser, grammars, math, and diagrams", async () => {
  const source = await (await fixture("documents/rendering.md")).text();
  const model = renderDocument(source);
  expect(model.headings.map(({ id }) => id)).toContain("html-heading");
  expect(
    model.codeBlocks.filter(({ highlighted }) => highlighted).length,
  ).toBeGreaterThan(20);
  expect(
    model.codeBlocks.find(({ language }) => language === null)?.highlighted,
  ).toBe(false);
  expect(model.html).toContain("hljs-");
  expect(model.html).toContain("katex");
  expect(model.html).toContain("diagram-pending");
  expect(model.resources.map(({ alt }) => alt)).toEqual([
    "png",
    "jpeg",
    "gif",
    "webp",
    "svg",
    "missing",
  ]);
});

test("F malformed fixture reports useful bounded errors", async () => {
  const source = await (await fixture("security/malformed.md")).text();
  const model = renderDocument(source);
  expect(model.errors.map(({ kind }) => kind)).toContain("math");
  expect(model.errors.map(({ kind }) => kind)).toContain("mermaid");
});

test("hostile SVG loses scripts, handlers, and external references", async () => {
  const source = await (await fixture("security/hostile.svg")).text();
  const sanitized = sanitizeGeneratedSvg(source);
  expect(sanitized).not.toMatch(
    /<script|onload|(?:xlink:)?href=|remote\.invalid|file:/i,
  );
});

test("F security fixture produces no executable document content", async () => {
  const source = await (await fixture("security/hostile.md")).text();
  const model = renderDocument(source);
  expect(model.html).not.toMatch(
    /<script|onload|onclick|<iframe|<form|<input|<object|javascript:/i,
  );
  expect(model.html).toContain("image-placeholder");
});
