/**
 * Dependency-neutral document renderer core.
 *
 * This is deliberately a bounded scaffold, not CommonMark or a syntax
 * highlighter. Approved parser/highlighter/diagram/math assets can replace
 * individual hooks without changing the document, locator, or generation API.
 */

export const MAX_SOURCE_BYTES = 10 * 1024 * 1024;
export const DEFAULT_RENDER_BUDGET = {
  maxDiagramBytes: 64 * 1024,
  maxDiagramLines: 2_000,
  maxMathBytes: 32 * 1024,
};

export type BlockKind =
  | "heading"
  | "paragraph"
  | "code"
  | "list"
  | "table"
  | "quote"
  | "diagram"
  | "math";

export interface Heading {
  id: string;
  level: number;
  text: string;
  blockId: string;
}

export interface CodeBlock {
  blockId: string;
  language: string | null;
  source: string;
  highlighted: false;
}

export interface ResourceRequest {
  kind: "image" | "svg-reference";
  reference: string;
  alt?: string;
}

export interface RenderError {
  kind: "source" | "mermaid" | "math";
  message: string;
  blockId?: string;
}

export interface RenderBlock {
  id: string;
  kind: BlockKind;
  text: string;
  html: string;
  code?: CodeBlock;
}

export interface RenderModel {
  source: string;
  generation?: number;
  html: string;
  blocks: RenderBlock[];
  headings: Heading[];
  codeBlocks: CodeBlock[];
  resources: ResourceRequest[];
  errors: RenderError[];
}

export interface RenderOptions {
  generation?: number;
  resolveResource?: (
    reference: string,
    kind: ResourceRequest["kind"],
  ) => string | null;
  budget?: Partial<typeof DEFAULT_RENDER_BUDGET>;
}

export interface Locator {
  heading?: string;
  block?: string;
  offset?: number;
  fallback?: "nearest_heading" | "document_start";
}

export interface RestoredLocator {
  blockId: string | null;
  offset: number;
  reason:
    | "exact_block"
    | "heading"
    | "nearest_heading"
    | "document_start"
    | "empty";
}

export interface SelectionState {
  startBlockId: string;
  startOffset: number;
  endBlockId: string;
  endOffset: number;
  text: string;
}

export interface SearchMatch {
  blockId: string;
  index: number;
  length: number;
  text: string;
}

export interface RenderedAsync<T> {
  generation: number;
  value: T;
}

const LANGUAGE_ALIASES: Record<string, string> = {
  py: "python",
  python: "python",
  js: "javascript",
  cjs: "javascript",
  mjs: "javascript",
  javascript: "javascript",
  ts: "typescript",
  typescript: "typescript",
  jsx: "jsx",
  tsx: "tsx",
  bash: "bash",
  sh: "bash",
  shell: "bash",
  zsh: "bash",
  sql: "sql",
  rs: "rust",
  rust: "rust",
  go: "go",
  golang: "go",
  c: "c",
  "c++": "cpp",
  cpp: "cpp",
  cxx: "cpp",
  cc: "cpp",
  java: "java",
  cs: "csharp",
  "c#": "csharp",
  csharp: "csharp",
  ruby: "ruby",
  rb: "ruby",
  php: "php",
  swift: "swift",
  kotlin: "kotlin",
  kt: "kotlin",
  html: "html",
  xhtml: "html",
  css: "css",
  scss: "css",
  sass: "css",
  json: "json",
  yaml: "yaml",
  yml: "yaml",
  toml: "toml",
  md: "markdown",
  markdown: "markdown",
};

export const SUPPORTED_LANGUAGES = [
  ...new Set(Object.values(LANGUAGE_ALIASES)),
];

export function normalizeLanguage(value: string): string | null {
  const key = value.trim().toLowerCase();
  return LANGUAGE_ALIASES[key] ?? null;
}

export function slugifyHeading(
  value: string,
  used = new Map<string, number>(),
): string {
  // GitHub keeps Unicode letters/numbers and ASCII hyphens, but drops punctuation.
  const base = value
    .replace(/<[^>]*>/g, "")
    .toLocaleLowerCase()
    .trim()
    .replace(/[^\p{L}\p{N}\s_-]/gu, "")
    .replace(/[\s_]/g, "-");
  const slug = base || "section";
  const count = used.get(slug) ?? 0;
  used.set(slug, count + 1);
  return count ? `${slug}-${count}` : slug;
}

function escapeHtml(value: string): string {
  return value.replace(
    /[&<>"']/g,
    (character) =>
      ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[
        character
      ]!,
  );
}

function escapeAttribute(value: string): string {
  return escapeHtml(value.replace(/[\u0000-\u001f\u007f]/g, ""));
}

function safeUrl(value: string): string | null {
  const url = value.trim();
  if (!url || /^(?:javascript|vbscript|data|file|gopher):/i.test(url))
    return null;
  if (/^[a-z][a-z\d+.-]*:/i.test(url) && !/^(?:https?|mailto):/i.test(url))
    return null;
  return url;
}

const ALLOWED_TAGS = new Set([
  "a",
  "abbr",
  "b",
  "blockquote",
  "br",
  "code",
  "del",
  "details",
  "div",
  "em",
  "h1",
  "h2",
  "h3",
  "h4",
  "h5",
  "h6",
  "hr",
  "i",
  "img",
  "kbd",
  "li",
  "ol",
  "p",
  "pre",
  "q",
  "s",
  "samp",
  "small",
  "span",
  "strong",
  "sub",
  "summary",
  "sup",
  "table",
  "tbody",
  "td",
  "tfoot",
  "th",
  "thead",
  "tr",
  "u",
  "ul",
]);
const DROP_TAGS =
  /<(?:script|style|iframe|frame|frameset|form|object|embed|applet|base|link|meta|template)\b[^>]*>[\s\S]*?(?:<\/(?:script|style|iframe|frame|frameset|form|object|embed|applet|base|link|meta|template)\s*>|$)/gi;
const DROP_SINGLE_TAGS =
  /<\/?(?:script|style|iframe|frame|frameset|form|object|embed|applet|base|link|meta|template)\b[^>]*>/gi;

export interface SanitizerOptions {
  resolveResource?: (
    reference: string,
    kind: "image" | "svg-reference",
  ) => string | null;
}

/** Remove executable/document-level HTML. Resource URLs only come from policy callback. */
export function sanitizeHtml(
  input: string,
  options: SanitizerOptions = {},
): string {
  const clean = input
    .replace(/<!--[\s\S]*?-->/g, "")
    .replace(DROP_TAGS, "")
    .replace(DROP_SINGLE_TAGS, "");
  return clean.replace(
    /<\s*(\/?)\s*([a-z][\w:-]*)([^>]*)>/gi,
    (_whole, close: string, name: string, rawAttrs: string) => {
      const tag = name.toLowerCase();
      if (!ALLOWED_TAGS.has(tag)) return "";
      if (close) return `</${tag}>`;
      const attrs: string[] = [];
      const attributePattern =
        /([:\w-]+)(?:\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s"'=<>`]+)))?/g;
      for (const match of rawAttrs.matchAll(attributePattern)) {
        const attribute = match[1]!.toLowerCase();
        const value = match[2] ?? match[3] ?? match[4] ?? "";
        if (
          attribute.startsWith("on") ||
          attribute === "style" ||
          attribute === "srcset" ||
          attribute === "formaction"
        )
          continue;
        if (attribute === "open" && tag === "details") {
          attrs.push("open");
          continue;
        }
        if (
          ![
            "alt",
            "title",
            "id",
            "colspan",
            "rowspan",
            "scope",
            "href",
            "src",
          ].includes(attribute)
        )
          continue;
        if (attribute === "src") {
          const approved = options.resolveResource?.(value, "image");
          if (approved) attrs.push(`src="${escapeAttribute(approved)}"`);
          continue;
        }
        if (attribute === "href") {
          const approved = safeUrl(value);
          if (approved) attrs.push(`href="${escapeAttribute(approved)}"`);
          continue;
        }
        attrs.push(`${attribute}="${escapeAttribute(value)}"`);
      }
      return `<${tag}${attrs.length ? ` ${attrs.join(" ")}` : ""}>`;
    },
  );
}

function decodeEntities(value: string): string {
  return value.replace(
    /&(?:amp|lt|gt|quot|#39|nbsp);/g,
    (entity) =>
      ({
        "&amp;": "&",
        "&lt;": "<",
        "&gt;": ">",
        "&quot;": '"',
        "&#39;": "'",
        "&nbsp;": " ",
      })[entity]!,
  );
}

function plainText(value: string): string {
  return decodeEntities(
    value
      .replace(/`([^`]+)`/g, "$1")
      .replace(/!\[([^\]]*)\]\([^)]*\)/g, "$1")
      .replace(/\[([^\]]+)\]\([^)]*\)/g, "$1")
      .replace(/\*\*|__|~~|[*_]/g, "")
      .replace(/<[^>]*>/g, ""),
  );
}

interface InlineResult {
  html: string;
  text: string;
  resources: ResourceRequest[];
}

function renderInline(
  value: string,
  options: RenderOptions,
  errors: RenderError[],
  blockId: string,
): InlineResult {
  const tokens: Array<{
    html: string;
    text: string;
    resource?: ResourceRequest;
  }> = [];
  const protect = (html: string, text: string, resource?: ResourceRequest) => {
    const marker = `\u0000${tokens.length}\u0000`;
    tokens.push({ html, text, resource });
    return marker;
  };
  const input = value.replace(
    /`([^`\n]+)`|!\[([^\]]*)\]\(([^)\s]+)(?:\s+["']([^"']*)["'])?\)|\[([^\]]+)\]\(([^)\s]+)(?:\s+["']([^"']*)["'])?\)|<((?:https?:\/\/|mailto:)[^>]+)>|(<\/?[a-z][^>]*>)/gi,
    (
      _whole,
      code,
      imageAlt,
      imageRef,
      _imageTitle,
      linkText,
      linkRef,
      _linkTitle,
      autoLink,
      rawTag,
    ) => {
      if (code !== undefined)
        return protect(`<code>${escapeHtml(code)}</code>`, code);
      if (imageRef !== undefined) {
        const resource: ResourceRequest = {
          kind: "image",
          reference: imageRef,
          alt: imageAlt ?? "",
        };
        const approved = options.resolveResource?.(imageRef, "image");
        const html = approved
          ? `<img src="${escapeAttribute(approved)}" alt="${escapeAttribute(imageAlt ?? "")}">`
          : `<span class="image-placeholder" role="img" aria-label="${escapeAttribute(imageAlt ?? "Image unavailable")}" data-mdvr-resource="${escapeAttribute(imageRef)}">[image: ${escapeHtml(imageAlt ?? "Image unavailable")}]</span>`;
        return protect(html, imageAlt ?? "", resource);
      }
      if (linkRef !== undefined) {
        const href = safeUrl(linkRef);
        return protect(
          href
            ? `<a href="${escapeAttribute(href)}">${renderInline(linkText, options, errors, blockId).html}</a>`
            : escapeHtml(linkText),
          plainText(linkText),
        );
      }
      if (autoLink !== undefined)
        return protect(
          `<a href="${escapeAttribute(autoLink)}">${escapeHtml(autoLink)}</a>`,
          autoLink,
        );
      if (rawTag !== undefined)
        return protect(
          sanitizeHtml(rawTag, { resolveResource: options.resolveResource }),
          plainText(rawTag),
        );
      return _whole;
    },
  );

  let html = escapeHtml(input);
  if ((value.match(/\$\$/g)?.length ?? 0) % 2 === 1)
    errors.push({
      kind: "math",
      message: "Math input is incomplete.",
      blockId,
    });
  html = html.replace(
    /\$\$([^$\n]+)\$\$|\$([^$\n]+)\$/g,
    (_whole, display, inline) => {
      const source = display ?? inline;
      const result = renderMath(
        source,
        Boolean(display),
        options.budget?.maxMathBytes ?? DEFAULT_RENDER_BUDGET.maxMathBytes,
      );
      if (result.status === "error")
        errors.push({
          kind: "math",
          message: result.message ?? "Invalid math input.",
          blockId,
        });
      return protect(result.html, source);
    },
  );
  html = html.replace(
    /\*\*([^*\n]+)\*\*|__([^_\n]+)__|~~([^~\n]+)~~|\*([^*\n]+)\*|_([^_\n]+)_/g,
    (_whole, strongA, strongB, strike, emA, emB) => {
      const text = strongA ?? strongB ?? strike ?? emA ?? emB;
      const tag =
        strongA !== undefined || strongB !== undefined
          ? "strong"
          : strike === undefined
            ? "em"
            : "del";
      return `<${tag}>${text}</${tag}>`;
    },
  );
  html = html.replace(
    /\u0000(\d+)\u0000/g,
    (_whole, index) => tokens[Number(index)]!.html,
  );
  const resources = tokens.flatMap((token) =>
    token.resource ? [token.resource] : [],
  );
  return { html, text: plainText(value), resources };
}

function blockShell(tag: string, id: string, body: string): string {
  return `<${tag} data-block-id="${escapeAttribute(id)}" data-searchable>${body}</${tag}>`;
}

function tableCells(line: string): string[] {
  const trimmed = line.trim().replace(/^\|/, "").replace(/\|$/, "");
  return trimmed
    .split(/(?<!\\)\|/)
    .map((cell) => cell.replace(/\\\|/g, "|").trim());
}

function isTableDivider(line: string): boolean {
  return (
    tableCells(line).length > 0 &&
    tableCells(line).every((cell) => /^:?-{3,}:?$/.test(cell))
  );
}

function renderTable(
  lines: string[],
  blockId: string,
  options: RenderOptions,
): { html: string; text: string; resources: ResourceRequest[] } {
  const rows = lines.map(tableCells);
  const header = rows[0] ?? [];
  const body = rows.slice(2);
  const errors: RenderError[] = [];
  const resources: ResourceRequest[] = [];
  const renderCell = (cell: string, tag: "th" | "td") => {
    const rendered = renderInline(cell, options, errors, blockId);
    resources.push(...rendered.resources);
    return `<${tag}>${rendered.html}</${tag}>`;
  };
  const html = `<table><thead><tr>${header.map((cell) => renderCell(cell, "th")).join("")}</tr></thead><tbody>${body.map((row) => `<tr>${row.map((cell) => renderCell(cell, "td")).join("")}</tr>`).join("")}</tbody></table>`;
  return {
    html: blockShell("div", blockId, html),
    text: rows.map((row) => row.join(" ")).join("\n"),
    resources,
  };
}

export interface DiagramResult {
  status: "pending" | "error";
  html: string;
  message?: string;
}

export function renderMermaid(
  source: string,
  limits = DEFAULT_RENDER_BUDGET,
): DiagramResult {
  if (
    new TextEncoder().encode(source).byteLength > limits.maxDiagramBytes ||
    source.split("\n").length > limits.maxDiagramLines
  ) {
    return {
      status: "error",
      html: `<div class="render-error">Mermaid input exceeds finite render budget.</div>`,
      message: "Mermaid input exceeds finite render budget.",
    };
  }
  if (
    !/\b(?:flowchart|graph|sequenceDiagram|stateDiagram(?:-v2)?)\b/i.test(
      source,
    )
  ) {
    return {
      status: "error",
      html: `<div class="render-error">Mermaid input has no supported diagram declaration.</div>`,
      message: "Mermaid input has no supported diagram declaration.",
    };
  }
  if (
    /-->\s*(?:$|\n)/m.test(source) ||
    /\b(?:flowchart|graph)\b[^\n]*\n?\s*[A-Za-z0-9_-]+\s*-->\s*$/m.test(source)
  ) {
    return {
      status: "error",
      html: `<div class="render-error">Mermaid edge is missing its target.</div>`,
      message: "Mermaid edge is missing its target.",
    };
  }
  return {
    status: "pending",
    html: `<div class="diagram-pending" data-renderer="mermaid">Mermaid diagram pending approved bundled renderer.</div>`,
  };
}

export interface MathResult {
  status: "pending" | "error";
  html: string;
  message?: string;
}

export function renderMath(
  source: string,
  display: boolean,
  maxBytes = DEFAULT_RENDER_BUDGET.maxMathBytes,
): MathResult {
  if (new TextEncoder().encode(source).byteLength > maxBytes) {
    return {
      status: "error",
      html: `<div class="render-error">Math input exceeds finite render budget.</div>`,
      message: "Math input exceeds finite render budget.",
    };
  }
  if (
    (source.match(/\\/g) ?? []).length % 2 === 1 ||
    /(?:^|[^\\])\$(?:$|[^$])/.test(source)
  ) {
    return {
      status: "error",
      html: `<div class="render-error">Math input is incomplete.</div>`,
      message: "Math input is incomplete.",
    };
  }
  const tag = display ? "div" : "span";
  return {
    status: "pending",
    html: `<${tag} class="math-pending" data-renderer="tex">TeX rendering pending approved bundled renderer.</${tag}>`,
  };
}

export function renderDocument(
  source: string,
  options: RenderOptions = {},
): RenderModel {
  if (new TextEncoder().encode(source).byteLength > MAX_SOURCE_BYTES) {
    return {
      source,
      generation: options.generation,
      html: `<div class="render-error">Source exceeds 10 MiB renderer budget.</div>`,
      blocks: [],
      headings: [],
      codeBlocks: [],
      resources: [],
      errors: [
        { kind: "source", message: "Source exceeds 10 MiB renderer budget." },
      ],
    };
  }
  const lines = source.replace(/\r\n?/g, "\n").split("\n");
  const blocks: RenderBlock[] = [];
  const headings: Heading[] = [];
  const codeBlocks: CodeBlock[] = [];
  const resources: ResourceRequest[] = [];
  const errors: RenderError[] = [];
  const usedSlugs = new Map<string, number>();
  const footnotes = new Map<string, string>();
  for (const line of lines) {
    const definition = line.match(/^\[\^([^\]]+)\]:\s*(.*)$/);
    if (definition) footnotes.set(definition[1]!, definition[2]!);
  }
  let index = 0;
  let paragraphNo = 0;
  let listNo = 0;
  let tableNo = 0;
  const add = (block: RenderBlock) => blocks.push(block);

  while (index < lines.length) {
    const line = lines[index]!;
    if (!line.trim() || /^\[\^[^\]]+\]:/.test(line)) {
      index += 1;
      continue;
    }
    const fence = line.match(/^ {0,3}(`{3,}|~{3,})\s*([^\s]*)?.*$/);
    if (fence) {
      const marker = fence[1]!;
      const body: string[] = [];
      index += 1;
      while (
        index < lines.length &&
        !new RegExp(`^ {0,3}${marker[0]}{${marker.length},}\\s*$`).test(
          lines[index]!,
        )
      )
        body.push(lines[index++]!);
      if (index < lines.length) index += 1;
      const id = `code-${codeBlocks.length + 1}`;
      const language = normalizeLanguage(fence[2] ?? "");
      const code: CodeBlock = {
        blockId: id,
        language,
        source: body.join("\n"),
        highlighted: false,
      };
      codeBlocks.push(code);
      if ((fence[2] ?? "").toLowerCase() === "mermaid") {
        const result = renderMermaid(code.source, {
          ...DEFAULT_RENDER_BUDGET,
          ...options.budget,
        });
        if (result.status === "error")
          errors.push({
            kind: "mermaid",
            message: result.message!,
            blockId: id,
          });
        add({
          id,
          kind: "diagram",
          text: code.source,
          html: blockShell(
            "div",
            id,
            `<pre class="mermaid-source"><code>${escapeHtml(code.source)}</code></pre>${result.html}`,
          ),
          code,
        });
      } else {
        add({
          id,
          kind: "code",
          text: code.source,
          html: blockShell(
            "pre",
            id,
            `<code data-language="${escapeAttribute(language ?? "")}" data-highlighted="false">${escapeHtml(code.source)}</code>`,
          ),
          code,
        });
      }
      continue;
    }
    const heading = line.match(/^ {0,3}(#{1,6})\s+(.+?)\s*#*\s*$/);
    if (heading) {
      const id = `heading-${headings.length + 1}`;
      const rendered = renderInline(heading[2]!, options, errors, id);
      const headingId = slugifyHeading(plainText(heading[2]!), usedSlugs);
      const item = {
        id: headingId,
        level: heading[1]!.length,
        text: plainText(heading[2]!),
        blockId: id,
      };
      headings.push(item);
      resources.push(...rendered.resources);
      add({
        id,
        kind: "heading",
        text: item.text,
        html: `<h${item.level} id="${escapeAttribute(item.id)}" data-block-id="${id}" data-searchable>${rendered.html}</h${item.level}>`,
      });
      index += 1;
      continue;
    }
    if (
      index + 1 < lines.length &&
      line.includes("|") &&
      isTableDivider(lines[index + 1]!)
    ) {
      const tableLines = [line, lines[index + 1]!];
      index += 2;
      while (
        index < lines.length &&
        lines[index]!.includes("|") &&
        lines[index]!.trim()
      )
        tableLines.push(lines[index++]!);
      const id = `table-${++tableNo}`;
      const table = renderTable(tableLines, id, options);
      resources.push(...table.resources);
      add({ id, kind: "table", text: table.text, html: table.html });
      continue;
    }
    const listStart = line.match(/^\s*(?:[-+*]|\d+[.)])\s+(.*)$/);
    if (listStart) {
      const items: string[] = [];
      const ordered = /^\s*\d+[.)]/.test(line);
      while (index < lines.length) {
        const item = lines[index]!.match(/^\s*(?:[-+*]|\d+[.)])\s+(.*)$/);
        if (!item) break;
        items.push(item[1]!);
        index += 1;
      }
      const id = `list-${++listNo}`;
      const itemHtml = items
        .map((item) => {
          const task = item.match(/^\[([ xX])\]\s+(.*)$/);
          const value = task
            ? `<input type="checkbox" disabled${task[1]!.toLowerCase() === "x" ? " checked" : ""}> ${task[2]}`
            : item;
          const rendered = renderInline(value, options, errors, id);
          resources.push(...rendered.resources);
          return `<li>${task ? value.replace(task[2]!, rendered.html) : rendered.html}</li>`;
        })
        .join("");
      add({
        id,
        kind: "list",
        text: items.map(plainText).join("\n"),
        html: blockShell(ordered ? "ol" : "ul", id, itemHtml),
      });
      continue;
    }
    if (/^\s*>/.test(line)) {
      const quoteLines: string[] = [];
      while (index < lines.length && /^\s*>/.test(lines[index]!))
        quoteLines.push(lines[index++]!.replace(/^\s*>\s?/, ""));
      const id = `quote-${blocks.length + 1}`;
      const rendered = renderInline(quoteLines.join(" "), options, errors, id);
      resources.push(...rendered.resources);
      add({
        id,
        kind: "quote",
        text: rendered.text,
        html: blockShell("blockquote", id, rendered.html),
      });
      continue;
    }
    if (/^\s*\$\$\s*$/.test(line)) {
      const body: string[] = [];
      index += 1;
      while (index < lines.length && !/^\s*\$\$\s*$/.test(lines[index]!))
        body.push(lines[index++]!);
      const closed = index < lines.length;
      if (closed) index += 1;
      const id = `math-${blocks.length + 1}`;
      const sourceText = body.join("\n");
      const result = closed
        ? renderMath(
            sourceText,
            true,
            options.budget?.maxMathBytes ?? DEFAULT_RENDER_BUDGET.maxMathBytes,
          )
        : {
            status: "error" as const,
            html: `<div class="render-error">Math input is incomplete.</div>`,
            message: "Math input is incomplete.",
          };
      if (result.status === "error")
        errors.push({ kind: "math", message: result.message!, blockId: id });
      add({
        id,
        kind: "math",
        text: sourceText,
        html: blockShell(
          "div",
          id,
          `<pre class="math-source"><code>${escapeHtml(sourceText)}</code></pre>${result.html}`,
        ),
      });
      continue;
    }
    const paragraph: string[] = [line];
    index += 1;
    while (
      index < lines.length &&
      lines[index]!.trim() &&
      !/^ {0,3}(?:#{1,6}\s|```|~~~|>|[-+*]\s+|\d+[.)]\s+)/.test(lines[index]!)
    )
      paragraph.push(lines[index++]!);
    const id = `paragraph-${++paragraphNo}`;
    const rendered = renderInline(paragraph.join(" "), options, errors, id);
    resources.push(...rendered.resources);
    let text = rendered.text;
    text = text.replace(
      /\[\^([^\]]+)\]/g,
      (_whole, name) => footnotes.get(name) ?? name,
    );
    add({
      id,
      kind: "paragraph",
      text,
      html: blockShell("p", id, rendered.html),
    });
  }
  return {
    source,
    generation: options.generation,
    html: blocks.map((block) => block.html).join("\n"),
    blocks,
    headings,
    codeBlocks,
    resources,
    errors,
  };
}

export function restoreLocator(
  locator: Locator | null | undefined,
  model: RenderModel,
): RestoredLocator {
  if (!model.blocks.length)
    return { blockId: null, offset: 0, reason: "empty" };
  const offset = Math.max(0, locator?.offset ?? 0);
  if (
    locator?.block &&
    model.blocks.some((block) => block.id === locator.block)
  )
    return { blockId: locator.block, offset, reason: "exact_block" };
  if (locator?.heading) {
    const heading = model.headings.find(
      (item) => item.id === locator.heading || item.blockId === locator.heading,
    );
    if (heading) return { blockId: heading.blockId, offset, reason: "heading" };
  }
  if (locator?.fallback === "document_start")
    return {
      blockId: model.blocks[0]!.id,
      offset: 0,
      reason: "document_start",
    };
  const heading = model.headings[0];
  return heading
    ? { blockId: heading.blockId, offset, reason: "nearest_heading" }
    : { blockId: model.blocks[0]!.id, offset: 0, reason: "document_start" };
}

export function searchRendered(
  model: RenderModel,
  query: string,
  caseSensitive = false,
): SearchMatch[] {
  if (!query) return [];
  const matches: SearchMatch[] = [];
  for (const block of model.blocks) {
    const haystack = caseSensitive
      ? block.text
      : block.text.toLocaleLowerCase();
    const needle = caseSensitive ? query : query.toLocaleLowerCase();
    let from = 0;
    while (from <= haystack.length - needle.length) {
      const index = haystack.indexOf(needle, from);
      if (index < 0) break;
      matches.push({
        blockId: block.id,
        index,
        length: query.length,
        text: block.text.slice(index, index + query.length),
      });
      from = index + Math.max(needle.length, 1);
    }
  }
  return matches;
}

export function preserveSelection(
  previous: SelectionState | null,
  next: RenderModel,
): SelectionState | null {
  if (!previous || !previous.text) return null;
  const exactStart = next.blocks.find(
    (block) => block.id === previous.startBlockId,
  );
  if (
    exactStart &&
    exactStart.text.slice(
      previous.startOffset,
      previous.startOffset + previous.text.length,
    ) === previous.text
  ) {
    return {
      ...previous,
      endBlockId: previous.startBlockId,
      endOffset: previous.startOffset + previous.text.length,
    };
  }
  for (const block of next.blocks) {
    const index = block.text.indexOf(previous.text);
    if (index >= 0)
      return {
        startBlockId: block.id,
        startOffset: index,
        endBlockId: block.id,
        endOffset: index + previous.text.length,
        text: previous.text,
      };
  }
  return null;
}

export class GenerationGate {
  private latest = 0;
  begin(generation: number): void {
    this.latest = Math.max(this.latest, generation);
  }
  isCurrent(generation: number): boolean {
    return generation === this.latest;
  }
  accept<T>(generation: number, value: T): RenderedAsync<T> | null {
    return this.isCurrent(generation) ? { generation, value } : null;
  }
}

export async function rejectStale<T>(
  gate: GenerationGate,
  generation: number,
  work: () => T | Promise<T>,
): Promise<RenderedAsync<T> | null> {
  const value = await work();
  return gate.accept(generation, value);
}

export function mountDocument(root: HTMLElement, model: RenderModel): void {
  // One content root keeps browser selection and focus outside renderer-owned DOM.
  root.dataset.generation =
    model.generation === undefined ? "" : String(model.generation);
  const parsed = new DOMParser().parseFromString(model.html, "text/html");
  root.replaceChildren(...Array.from(parsed.body.childNodes));
}
