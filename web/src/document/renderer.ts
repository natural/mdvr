import MarkdownIt from "markdown-it";
import footnote from "markdown-it-footnote";
import taskLists from "markdown-it-task-lists";
import DOMPurify from "dompurify";
import hljs from "highlight.js/lib/common";
import katex from "katex";
import mermaid from "mermaid";

export const MAX_SOURCE_BYTES = 20 * 1024 * 1024;
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
  highlighted: boolean;
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
  return LANGUAGE_ALIASES[value.trim().toLowerCase()] ?? null;
}

export function slugifyHeading(
  value: string,
  used = new Map<string, number>(),
): string {
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
  if (
    !url ||
    /^\/\//.test(url) ||
    /^(?:javascript|vbscript|data|file|gopher):/i.test(url)
  )
    return null;
  if (/^[a-z][a-z\d+.-]*:/i.test(url) && !/^(?:https?|mailto):/i.test(url))
    return null;
  return url;
}

const ALLOWED_TAGS = [
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
  "input",
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
];
const ALLOWED_ATTR = [
  "alt",
  "title",
  "id",
  "class",
  "colspan",
  "rowspan",
  "scope",
  "href",
  "src",
  "data-block-id",
  "data-searchable",
  "data-language",
  "data-highlighted",
  "data-mdvr-resource",
  "data-renderer",
  "role",
  "aria-label",
  "type",
  "checked",
  "disabled",
];
const DROP_TAGS =
  /<(?:script|style|iframe|frame|frameset|form|object|embed|applet|base|link|meta|template)\b[^>]*>[\s\S]*?(?:<\/(?:script|style|iframe|frame|frameset|form|object|embed|applet|base|link|meta|template)\s*>|$)/gi;
const DROP_SINGLE_TAGS =
  /<\/?(?:script|style|iframe|frame|frameset|form|object|embed|applet|base|link|meta|template)\b[^>]*>/gi;

export interface SanitizerOptions {
  resolveResource?: (
    reference: string,
    kind: "image" | "svg-reference",
  ) => string | null;
  approvedUrls?: ReadonlySet<string>;
}

function fallbackSanitize(input: string, options: SanitizerOptions): string {
  const clean = input
    .replace(/<!--[\s\S]*?-->/g, "")
    .replace(DROP_TAGS, "")
    .replace(DROP_SINGLE_TAGS, "");
  return clean.replace(
    /<\s*(\/?)\s*([a-z][\w:-]*)([^>]*)>/gi,
    (_whole, close: string, name: string, rawAttrs: string) => {
      const tag = name.toLowerCase();
      if (!ALLOWED_TAGS.includes(tag)) return "";
      if (close) return `</${tag}>`;
      if (
        tag === "input" &&
        (!/\btype\s*=\s*(["']?)checkbox\1/i.test(rawAttrs) ||
          !/\bdisabled(?:\s|=|>|$)/i.test(rawAttrs) ||
          !/\btask-list-item-checkbox\b/i.test(rawAttrs))
      )
        return "";
      const attrs: string[] = [];
      for (const match of rawAttrs.matchAll(
        /([:\w-]+)(?:\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s"'=<>`]+)))?/g,
      )) {
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
          attribute === "type" &&
          tag === "input" &&
          value.toLowerCase() !== "checkbox"
        )
          continue;
        if (![...ALLOWED_ATTR, "open"].includes(attribute)) continue;
        if (attribute === "src") {
          const approved = options.approvedUrls?.has(value)
            ? value
            : options.resolveResource?.(value, "image");
          if (approved) attrs.push(`src="${escapeAttribute(approved)}"`);
          continue;
        }
        if (attribute === "href") {
          const approved = safeUrl(value);
          if (approved) attrs.push(`href="${escapeAttribute(approved)}"`);
          continue;
        }
        attrs.push(
          attribute === "disabled" || attribute === "checked"
            ? attribute
            : `${attribute}="${escapeAttribute(value)}"`,
        );
      }
      return `<${tag}${attrs.length ? ` ${attrs.join(" ")}` : ""}>`;
    },
  );
}

/** DOMPurify is production sanitizer; fallback keeps pure Bun parser tests deterministic. */
export function sanitizeHtml(
  input: string,
  options: SanitizerOptions = {},
): string {
  if (typeof document === "undefined" || typeof window === "undefined")
    return fallbackSanitize(input, options);
  const routed = input.replace(
    /(<img\b[^>]*\bsrc\s*=\s*)(["'])([^"']+)\2/gi,
    (whole, prefix, quote, reference) => {
      const approved = options.approvedUrls?.has(reference)
        ? reference
        : options.resolveResource?.(reference, "image");
      return approved
        ? `${prefix}${quote}${escapeAttribute(approved)}${quote}`
        : whole.replace(/\bsrc\s*=\s*(?:"[^"]*"|'[^']*')/i, "");
    },
  );
  const fragment = DOMPurify.sanitize(routed, {
    ALLOWED_TAGS,
    ALLOWED_ATTR,
    FORBID_TAGS: ["style", "script", "iframe", "form", "object", "embed"],
    FORBID_ATTR: ["style"],
    RETURN_DOM_FRAGMENT: true,
  }) as DocumentFragment;
  fragment.querySelectorAll("input").forEach((input) => {
    if (
      input.type !== "checkbox" ||
      !input.disabled ||
      !input.classList.contains("task-list-item-checkbox")
    )
      input.remove();
  });
  const container = document.createElement("div");
  container.append(fragment);
  return container.innerHTML;
}

export function sanitizeGeneratedSvg(input: string): string {
  return input
    .replace(
      /<\/?(?:script|style)\b[^>]*>[\s\S]*?(?:<\/\s*(?:script|style)\s*>|$)/gi,
      "",
    )
    .replace(
      /\s(?:on\w+|href|xlink:href)\s*=\s*(?:"[^"]*"|'[^']*'|[^\s>]+)/gi,
      "",
    );
}

export function sanitizeResourceSvg(input: string): string {
  const safe = sanitizeGeneratedSvg(input);
  if (typeof document === "undefined" || typeof window === "undefined")
    return safe.replace(/<\/?foreignObject\b[^>]*>/gi, "");
  return DOMPurify.sanitize(safe, {
    USE_PROFILES: { svg: true, svgFilters: true },
    FORBID_TAGS: ["script", "style", "foreignObject"],
    FORBID_ATTR: ["style", "href", "xlink:href"],
  });
}

function plainText(value: string): string {
  return value
    .replace(/`([^`]+)`/g, "$1")
    .replace(/!\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/\[([^\]]+)\]\([^)]*\)/g, "$1")
    .replace(/\*\*|__|~~|[*_]/g, "")
    .replace(/<[^>]*>/g, "")
    .replace(
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

interface MathPart {
  source: string;
  display: boolean;
}
const MATH_START = "\uE000MDVR_MATH_";
const MATH_END = "\uE001";
function mathMarker(index: number): string {
  return `${MATH_START}${index}${MATH_END}`;
}
function protectMath(source: string): {
  source: string;
  parts: MathPart[];
  malformed: boolean;
} {
  const lines = source.replace(/\r\n?/g, "\n").split("\n");
  const parts: MathPart[] = [];
  let inFence = false;
  let malformed = false;
  const output: string[] = [];
  for (let i = 0; i < lines.length; i += 1) {
    const line = lines[i]!;
    const opening = line.match(/^ {0,3}(`{3,}|~{3,})/);
    if (opening) {
      inFence = !inFence;
      output.push(line);
      continue;
    }
    if (inFence) {
      output.push(line);
      continue;
    }
    if (/^\s*\$\$\s*$/.test(line)) {
      const body: string[] = [];
      i += 1;
      while (i < lines.length && !/^\s*\$\$\s*$/.test(lines[i]!))
        body.push(lines[i++]!);
      if (i === lines.length) {
        malformed = true;
        output.push(line, ...body);
        break;
      }
      const index = parts.push({ source: body.join("\n"), display: true }) - 1;
      output.push(mathMarker(index));
      continue;
    }
    const replaced = line.replace(
      /\$\$([^$\n]+)\$\$|(?<!\\)\$([^$\n]+)(?<!\\)\$/g,
      (_whole, display, inline) => {
        const index =
          parts.push({
            source: display ?? inline,
            display: display !== undefined,
          }) - 1;
        return mathMarker(index);
      },
    );
    if (
      (replaced.match(/(?<!\\)\$\$/g) ?? []).length % 2 ||
      (replaced.match(/(?<!\\)(?<!\$)\$(?!\$)/g) ?? []).length % 2
    )
      malformed = true;
    output.push(replaced);
  }
  return { source: output.join("\n"), parts, malformed };
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
  if (new TextEncoder().encode(source).byteLength > maxBytes)
    return {
      status: "error",
      html: `<div class="render-error">Math input exceeds finite render budget.</div>`,
      message: "Math input exceeds finite render budget.",
    };
  try {
    const html = katex.renderToString(source, {
      displayMode: display,
      throwOnError: true,
      output: "htmlAndMathml",
      trust: false,
    });
    // Keep pending status for API compatibility: production completion is synchronous for bounded TeX.
    return { status: "pending", html };
  } catch (error) {
    const message =
      error instanceof Error ? error.message : "Invalid math input.";
    return {
      status: "error",
      html: `<div class="render-error">${escapeHtml(message)}</div>`,
      message,
    };
  }
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
  )
    return {
      status: "error",
      html: `<div class="render-error">Mermaid input exceeds finite render budget.</div>`,
      message: "Mermaid input exceeds finite render budget.",
    };
  if (
    !/\b(?:flowchart|graph|sequenceDiagram|stateDiagram(?:-v2)?)\b/i.test(
      source,
    )
  )
    return {
      status: "error",
      html: `<div class="render-error">Mermaid input has no supported diagram declaration.</div>`,
      message: "Mermaid input has no supported diagram declaration.",
    };
  if (/-->\s*(?:$|\n)/m.test(source))
    return {
      status: "error",
      html: `<div class="render-error">Mermaid edge is missing its target.</div>`,
      message: "Mermaid edge is missing its target.",
    };
  return {
    status: "pending",
    html: `<div class="diagram-pending" data-renderer="mermaid">Mermaid diagram pending bundled renderer.</div>`,
  };
}

let mermaidConfigured = false;
export async function renderMermaidAsync(
  source: string,
  id: string,
  limits = DEFAULT_RENDER_BUDGET,
): Promise<DiagramResult> {
  const checked = renderMermaid(source, limits);
  if (checked.status === "error") return checked;
  try {
    if (!mermaidConfigured) {
      mermaid.initialize({
        startOnLoad: false,
        securityLevel: "strict",
        theme: "base",
        flowchart: { htmlLabels: false },
      });
      mermaidConfigured = true;
    }
    const result = await mermaid.render(
      `mdvr-mermaid-${id.replace(/[^a-z\d_-]/gi, "-")}`,
      source,
    );
    return { status: "pending", html: sanitizeGeneratedSvg(result.svg) };
  } catch (error) {
    const message =
      error instanceof Error
        ? error.message
        : "Mermaid input could not be rendered.";
    return {
      status: "error",
      html: `<div class="render-error">${escapeHtml(message)}</div>`,
      message,
    };
  }
}

function marker(id: string): string {
  return `<!--mdvr:${id}-->`;
}
function endMarker(): string {
  return "<!--/mdvr-->";
}
function inlineText(token: any): string {
  return (
    token?.children
      ?.map((child: any) =>
        child.type === "image" ? (child.attrGet("alt") ?? "") : child.content,
      )
      .join("") ??
    token?.content ??
    ""
  );
}
function tokenText(tokens: any[], start: number, end: number): string {
  return tokens
    .slice(start, end + 1)
    .filter((token) => token.type === "inline")
    .map(inlineText)
    .join("\n")
    .replace(/\n/g, " ")
    .trim();
}

function makeMarkdown(
  options: RenderOptions,
  idByToken: WeakMap<object, string>,
  math: MathPart[],
  codeByToken: Map<object, CodeBlock>,
  diagramByToken: Map<object, string>,
) {
  const md = new MarkdownIt({ html: true, linkify: true, breaks: false })
    .use(footnote)
    .use(taskLists, { enabled: false });
  md.renderer.rules.softbreak = () => " ";
  md.renderer.rules.heading_open = (tokens: any[], index: number) => {
    const token = tokens[index]!;
    return `${marker(idByToken.get(token) ?? "")}<${token.tag} id="${escapeAttribute(token.attrGet("id") ?? "section")}" data-block-id="${escapeAttribute(idByToken.get(token) ?? "")}" data-searchable>`;
  };
  md.renderer.rules.heading_close = (tokens: any[], index: number) =>
    `</${tokens[index]!.tag}>${endMarker()}`;
  for (const [open] of [
    ["paragraph_open", "p"],
    ["blockquote_open", "blockquote"],
    ["bullet_list_open", "ul"],
    ["ordered_list_open", "ol"],
    ["table_open", "table"],
  ] as const) {
    md.renderer.rules[open] = (
      tokens: any[],
      index: number,
      opts: any,
      _env: any,
      self: any,
    ) => {
      const token = tokens[index]!;
      const id = idByToken.get(token);
      if (!id) return self.renderToken(tokens, index, opts);
      if (open === "table_open")
        return `${marker(id)}<div data-block-id="${escapeAttribute(id)}" data-searchable>${self.renderToken(tokens, index, opts)}`;
      token.attrSet("data-block-id", id);
      token.attrSet("data-searchable", "");
      return `${marker(id)}${self.renderToken(tokens, index, opts)}`;
    };
    const closeType = open.replace("_open", "_close");
    md.renderer.rules[closeType] = (
      tokens: any[],
      index: number,
      opts: any,
      _env: any,
      self: any,
    ) =>
      `${self.renderToken(tokens, index, opts)}${open === "table_open" && tokens[index]!.level === 0 ? "</div>" : ""}${tokens[index]!.level === 0 ? endMarker() : ""}`;
  }
  md.renderer.rules.html_block = (tokens: any[], index: number) =>
    `${marker(idByToken.get(tokens[index]!) ?? "")}${tokens[index]!.content}${endMarker()}`;
  md.renderer.rules.footnote_block_open = (tokens: any[], index: number) =>
    `${marker(idByToken.get(tokens[index]!) ?? "")}<section class="footnotes" data-block-id="${escapeAttribute(idByToken.get(tokens[index]!) ?? "")}" data-searchable><ol>`;
  md.renderer.rules.footnote_block_close = () =>
    `</ol></section>${endMarker()}`;
  md.renderer.rules.paragraph_open = (
    tokens: any[],
    index: number,
    opts: any,
    _env: any,
    self: any,
  ) => {
    const inline = tokens[index + 1];
    if (
      inline?.type === "inline" &&
      /^\uE000MDVR_MATH_\d+\uE001$/.test(inline.content)
    )
      return "";
    const token = tokens[index]!;
    const id = idByToken.get(token);
    if (!id) return self.renderToken(tokens, index, opts);
    token.attrSet("data-block-id", id);
    token.attrSet("data-searchable", "");
    return `${marker(id)}${self.renderToken(tokens, index, opts)}`;
  };
  md.renderer.rules.paragraph_close = (tokens: any[], index: number) => {
    if (tokens[index]!.hidden) return "";
    const inline = tokens[index - 1];
    if (
      inline?.type === "inline" &&
      /^\uE000MDVR_MATH_\d+\uE001$/.test(inline.content)
    )
      return "";
    return `</p>${endMarker()}`;
  };
  md.renderer.rules.image = (
    tokens: any[],
    index: number,
    _opts: any,
    renderEnv: any,
  ) => {
    const token = tokens[index]!;
    const reference = token.attrGet("src") ?? "";
    const alt =
      token.attrGet("alt") ||
      token.content ||
      token.children?.map((child: any) => child.content).join("") ||
      "";
    const request = { kind: "image" as const, reference, alt };
    renderEnv.resources.push(request);
    const approved = options.resolveResource?.(reference, "image");
    if (approved) {
      renderEnv.approvedUrls.add(approved);
      return `<img src="${escapeAttribute(approved)}" alt="${escapeAttribute(alt)}">`;
    }
    return `<span class="image-placeholder" role="img" aria-label="${escapeAttribute(alt || "Image unavailable")}" data-mdvr-resource="${escapeAttribute(reference)}">[image: ${escapeHtml(alt || "Image unavailable")}]</span>`;
  };
  md.renderer.rules.fence = (
    tokens: any[],
    index: number,
    _opts: any,
    _renderEnv: any,
  ) => {
    const code = codeByToken.get(tokens[index]!)!;
    const diagramId = diagramByToken.get(tokens[index]!);
    if (diagramId)
      return `${marker(code.blockId)}<div data-block-id="${escapeAttribute(code.blockId)}" data-searchable><pre class="mermaid-source"><code>${escapeHtml(code.source)}</code></pre>${renderMermaid(code.source, { ...DEFAULT_RENDER_BUDGET, ...options.budget }).html}</div>${endMarker()}`;
    let content = escapeHtml(code.source);
    if (code.highlighted && code.language)
      content = hljs.highlight(code.source, {
        language: code.language === "html" ? "xml" : code.language,
        ignoreIllegals: true,
      }).value;
    return `${marker(code.blockId)}<pre data-block-id="${escapeAttribute(code.blockId)}" data-searchable><code class="language-${escapeAttribute(code.language ?? "text")}" data-language="${escapeAttribute(code.language ?? "")}" data-highlighted="${code.highlighted}">${content}</code></pre>${endMarker()}`;
  };
  md.renderer.rules.text = (tokens: any[], index: number) => {
    const content = tokens[index]!.content;
    return content.replace(
      new RegExp(`${MATH_START}(\\d+)${MATH_END}`, "g"),
      (_whole: string, value: string) => {
        const part = math[Number(value)]!;
        return renderMath(
          part.source,
          part.display,
          options.budget?.maxMathBytes ?? DEFAULT_RENDER_BUDGET.maxMathBytes,
        ).html;
      },
    );
  };
  return md;
}

export function renderDocument(
  source: string,
  options: RenderOptions = {},
): RenderModel {
  if (new TextEncoder().encode(source).byteLength > MAX_SOURCE_BYTES)
    return {
      source,
      generation: options.generation,
      html: `<div class="render-error">Source exceeds 20 MiB renderer budget.</div>`,
      blocks: [],
      headings: [],
      codeBlocks: [],
      resources: [],
      errors: [
        { kind: "source", message: "Source exceeds 20 MiB renderer budget." },
      ],
    };
  const prepared = protectMath(source);
  const env: any = {
    resources: [] as ResourceRequest[],
    approvedUrls: new Set<string>(),
  };
  const md = new MarkdownIt({ html: true, linkify: true, breaks: false })
    .use(footnote)
    .use(taskLists, { enabled: false });
  const tokens = md.parse(prepared.source, env);
  const usedSlugs = new Map<string, number>();
  const idByToken = new WeakMap<object, string>();
  const blockStarts: Array<{
    index: number;
    id: string;
    kind: BlockKind;
    end: number;
  }> = [];
  const codeByToken = new Map<object, CodeBlock>();
  const diagramByToken = new Map<object, string>();
  const counts = new Map<string, number>();
  const nextId = (prefix: string): string => {
    const count = (counts.get(prefix) ?? 0) + 1;
    counts.set(prefix, count);
    return `${prefix}-${count}`;
  };
  const starts = new Set([
    "heading_open",
    "paragraph_open",
    "blockquote_open",
    "bullet_list_open",
    "ordered_list_open",
    "table_open",
    "html_block",
    "fence",
    "footnote_block_open",
  ]);
  for (let i = 0; i < tokens.length; i += 1) {
    const token = tokens[i]!;
    if (
      !starts.has(token.type) ||
      (token.type !== "fence" && token.level !== 0)
    )
      continue;
    const prefix =
      token.type === "heading_open"
        ? "heading"
        : token.type === "paragraph_open"
          ? "paragraph"
          : token.type === "fence"
            ? "code"
            : token.type === "html_block"
              ? "html"
              : token.type === "footnote_block_open"
                ? "footnotes"
                : token.type.replace("_open", "");
    const id = nextId(prefix);
    idByToken.set(token, id);
    let end = i;
    if (token.nesting === 1) {
      let depth = 0;
      for (let j = i; j < tokens.length; j += 1) {
        depth += tokens[j]!.nesting;
        if (j > i && depth === 0) {
          end = j;
          break;
        }
      }
    }
    let kind: BlockKind =
      token.type === "heading_open"
        ? "heading"
        : token.type === "fence"
          ? "code"
          : token.type === "table_open"
            ? "table"
            : token.type === "blockquote_open"
              ? "quote"
              : token.type.includes("list")
                ? "list"
                : "paragraph";
    let text =
      token.type === "fence"
        ? token.content.endsWith("\n")
          ? token.content.slice(0, -1)
          : token.content
        : tokenText(tokens, i, end);
    if (
      token.type === "paragraph_open" &&
      /^\uE000MDVR_MATH_\d+\uE001$/.test(tokens[i + 1]?.content ?? "")
    ) {
      kind = "math";
      text =
        prepared.parts[Number(tokens[i + 1]!.content.match(/\d+/)![0])]!.source;
    }
    if (token.type === "heading_open") {
      const raw = tokenText(tokens, i, end);
      const headingId = slugifyHeading(raw, usedSlugs);
      token.attrSet("id", headingId);
      const heading: Heading = {
        id: headingId,
        level: Number(token.tag.slice(1)),

        text: plainText(raw),
        blockId: id,
      };
      (token as any).__mdvrHeading = heading;
    }
    if (token.type === "fence") {
      const rawLanguage = (token.info ?? "").trim().split(/\s+/, 1)[0] ?? "";
      const language = normalizeLanguage(rawLanguage);
      const highlighted = Boolean(
        language && hljs.getLanguage(language === "html" ? "xml" : language),
      );
      const code: CodeBlock = {
        blockId: id,
        language,
        source: text,
        highlighted,
      };
      codeByToken.set(token, code);
      if (rawLanguage.toLowerCase() === "mermaid") {
        kind = "diagram";
        diagramByToken.set(token, id);
      }
      text = code.source;
    }
    blockStarts.push({ index: i, id, kind, end });
  }
  const productionMd = makeMarkdown(
    options,
    idByToken,
    prepared.parts,
    codeByToken,
    diagramByToken,
  );
  // Use same parser instance configuration, but token metadata is shared by object identity only within this render.
  const rendered = productionMd.renderer.render(
    tokens,
    productionMd.options,
    env,
  );
  const blocks: RenderBlock[] = [];
  const blockById = new Map(blockStarts.map((block) => [block.id, block]));
  const codeById = new Map(
    [...codeByToken.values()].map((code) => [code.blockId, code]),
  );
  const rawParts = rendered.split(/(<!--mdvr:[^>]+-->|<!--\/mdvr-->)/g);
  let active: { id: string } | null = null;
  for (const part of rawParts) {
    const open = part.match(/^<!--mdvr:([^>]+)-->$/);
    if (open) {
      active = { id: open[1]! };
      continue;
    }
    if (part === "<!--/mdvr-->") {
      if (active) {
        const info = blockById.get(active.id);
        const code = info ? codeById.get(info.id) : undefined;
        blocks.push({
          id: active.id,
          kind: info?.kind ?? "paragraph",
          text:
            info?.index === undefined
              ? ""
              : info.kind === "code" || info.kind === "diagram"
                ? (code?.source ?? "")
                : tokenText(tokens, info.index, info.end),
          code,
        });
      }
      active = null;
      continue;
    }
  }
  const errors: RenderError[] = [];
  if (prepared.malformed)
    errors.push({ kind: "math", message: "Math input is incomplete." });
  for (const token of tokens)
    if (token.type === "fence" && diagramByToken.has(token)) {
      const result = renderMermaid(token.content.replace(/\n$/, ""), {
        ...DEFAULT_RENDER_BUDGET,
        ...options.budget,
      });
      if (result.status === "error")
        errors.push({
          kind: "mermaid",
          message: result.message!,
          blockId: diagramByToken.get(token),
        });
    }
  for (const block of blocks)
    if (block.kind === "math") {
      const result = renderMath(
        block.text,
        true,
        options.budget?.maxMathBytes ?? DEFAULT_RENDER_BUDGET.maxMathBytes,
      );
      if (result.status === "error")
        errors.push({
          kind: "math",
          message: result.message!,
          blockId: block.id,
        });
    }
  for (const headingToken of tokens.filter(
    (token) => token.type === "heading_open" && (token as any).__mdvrHeading,
  )) {
    const heading = (headingToken as any).__mdvrHeading as Heading;
    const block = blocks.find((item) => item.id === heading.blockId);
    if (block) {
      block.kind = "heading";
      block.text = heading.text;
    }
  }
  const headings: Heading[] = tokens
    .filter(
      (token) => token.type === "heading_open" && (token as any).__mdvrHeading,
    )
    .map((token) => (token as any).__mdvrHeading as Heading);
  let html = sanitizeHtml(
    rendered
      .replace(/<!--mdvr:[^>]+-->|<!--\/mdvr-->/g, "")
      .replace(/\b(?:javascript|vbscript|data|file|gopher):/gi, ""),
    { ...options, approvedUrls: env.approvedUrls },
  );
  html = html.replace(
    /<h([1-6])\b([^>]*)>([\s\S]*?)<\/h\1>/gi,
    (whole, level: string, attrs: string, body: string) => {
      if (/\bid\s*=\s*["']/i.test(attrs)) return whole;
      const text = plainText(body);
      const id = slugifyHeading(text, usedSlugs);
      const blockId =
        blocks.find((block) => block.text.includes(text))?.id ?? "html";
      headings.push({ id, level: Number(level), text, blockId });
      return `<h${level} id="${escapeAttribute(id)}"${attrs}>${body}</h${level}>`;
    },
  );
  return {
    source,
    generation: options.generation,
    html,
    blocks,
    headings,
    codeBlocks: [...codeByToken.values()],
    resources: env.resources,
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
  const needle = caseSensitive ? query : query.toLocaleLowerCase();
  const matches: SearchMatch[] = [];
  for (const block of model.blocks) {
    const haystack = caseSensitive
      ? block.text
      : block.text.toLocaleLowerCase();
    for (let from = 0; from <= haystack.length - needle.length; ) {
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
  )
    return {
      ...previous,
      endBlockId: previous.startBlockId,
      endOffset: previous.startOffset + previous.text.length,
    };
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
  return gate.accept(generation, await work());
}

export function mountDocument(root: HTMLElement, model: RenderModel): void {
  root.dataset.generation =
    model.generation === undefined ? "" : String(model.generation);
  const parsed = new DOMParser().parseFromString(model.html, "text/html");
  root.replaceChildren(...Array.from(parsed.body.childNodes));
}
