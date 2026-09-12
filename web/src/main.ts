import "./reader.css";
import "katex/dist/katex.min.css";
import {
    GenerationGate,
    mountDocument,
    renderDocument,
    renderMermaidAsync,
    sanitizeResourceSvg,
    searchRendered,
    type RenderModel,
} from "./document/renderer";

const rootElement = document.querySelector<HTMLElement>("#document");
if (rootElement === null) throw new Error("mdvr document root missing");
const root: HTMLElement = rootElement;

export type AppearanceMode = "light" | "dark" | "system";
export type SyntaxRole =
    | "keyword"
    | "string"
    | "comment"
    | "number"
    | "function"
    | "type"
    | "operator"
    | "punctuation";
export type AppearanceSyntaxToken = {
    role: SyntaxRole;
    foreground: string;
    background: string | null;
    bold: boolean;
    italic: boolean;
};
export type AppearanceTokens = {
    mode: AppearanceMode;
    scale_percent: number;
    reader_background: string;
    reader_foreground: string;
    code_background: string;
    accent: string;
    syntax: AppearanceSyntaxToken[];
};

const appearanceKeys = [
    "mode",
    "scale_percent",
    "reader_background",
    "reader_foreground",
    "code_background",
    "accent",
    "syntax",
] as const;
const syntaxKeys = [
    "role",
    "foreground",
    "background",
    "bold",
    "italic",
] as const;
const syntaxRoles = new Set<SyntaxRole>([
    "keyword",
    "string",
    "comment",
    "number",
    "function",
    "type",
    "operator",
    "punctuation",
]);
const colorPattern = /^#[0-9a-f]{6}$/i;

function exactKeys(value: Record<string, unknown>, keys: readonly string[]) {
    const allowed = new Set(keys);
    const actual = Object.keys(value);
    return (
        actual.length === keys.length && actual.every((key) => allowed.has(key))
    );
}

function isRecord(value: unknown): value is Record<string, unknown> {
    return typeof value === "object" && value !== null && !Array.isArray(value);
}

export function validateAppearanceTokens(
    value: unknown,
): value is AppearanceTokens {
    if (!isRecord(value) || !exactKeys(value, appearanceKeys)) return false;
    if (
        value.mode !== "light" &&
        value.mode !== "dark" &&
        value.mode !== "system"
    ) {
        return false;
    }
    if (
        typeof value.scale_percent !== "number" ||
        !Number.isInteger(value.scale_percent) ||
        value.scale_percent < 50 ||
        value.scale_percent > 300
    ) {
        return false;
    }
    if (
        ![
            "reader_background",
            "reader_foreground",
            "code_background",
            "accent",
        ].every(
            (key) =>
                typeof value[key] === "string" &&
                colorPattern.test(value[key] as string),
        )
    ) {
        return false;
    }
    if (!Array.isArray(value.syntax) || value.syntax.length > 128) return false;
    return value.syntax.every((candidate) => {
        if (!isRecord(candidate) || !exactKeys(candidate, syntaxKeys)) {
            return false;
        }
        return (
            typeof candidate.role === "string" &&
            syntaxRoles.has(candidate.role as SyntaxRole) &&
            typeof candidate.foreground === "string" &&
            colorPattern.test(candidate.foreground) &&
            (candidate.background === null ||
                (typeof candidate.background === "string" &&
                    colorPattern.test(candidate.background))) &&
            typeof candidate.bold === "boolean" &&
            typeof candidate.italic === "boolean"
        );
    });
}

const syntaxProperties: Record<
    SyntaxRole,
    { foreground: string; background: string; weight: string; style: string }
> = {
    keyword: {
        foreground: "--syntax-keyword-foreground",
        background: "--syntax-keyword-background",
        weight: "--syntax-keyword-weight",
        style: "--syntax-keyword-style",
    },
    string: {
        foreground: "--syntax-string-foreground",
        background: "--syntax-string-background",
        weight: "--syntax-string-weight",
        style: "--syntax-string-style",
    },
    comment: {
        foreground: "--syntax-comment-foreground",
        background: "--syntax-comment-background",
        weight: "--syntax-comment-weight",
        style: "--syntax-comment-style",
    },
    number: {
        foreground: "--syntax-number-foreground",
        background: "--syntax-number-background",
        weight: "--syntax-number-weight",
        style: "--syntax-number-style",
    },
    function: {
        foreground: "--syntax-function-foreground",
        background: "--syntax-function-background",
        weight: "--syntax-function-weight",
        style: "--syntax-function-style",
    },
    type: {
        foreground: "--syntax-type-foreground",
        background: "--syntax-type-background",
        weight: "--syntax-type-weight",
        style: "--syntax-type-style",
    },
    operator: {
        foreground: "--syntax-operator-foreground",
        background: "--syntax-operator-background",
        weight: "--syntax-operator-weight",
        style: "--syntax-operator-style",
    },
    punctuation: {
        foreground: "--syntax-punctuation-foreground",
        background: "--syntax-punctuation-background",
        weight: "--syntax-punctuation-weight",
        style: "--syntax-punctuation-style",
    },
};

export function applyAppearance(value: unknown): boolean {
    if (!validateAppearanceTokens(value)) return false;
    const theme = document.querySelector<HTMLSelectElement>("#theme");
    if (theme && !theme.selectedOptions[0]?.hasAttribute("data-imported"))
        theme.value = value.mode;
    const rootStyle = document.documentElement.style;
    rootStyle.setProperty("--reader-background", value.reader_background);
    rootStyle.setProperty("--reader-text", value.reader_foreground);
    rootStyle.setProperty("--reader-code", value.code_background);
    rootStyle.setProperty("--reader-accent", value.accent);
    rootStyle.setProperty("--reader-scale", String(value.scale_percent / 100));
    rootStyle.setProperty(
        "--reader-color-scheme",
        value.mode === "system" ? "light dark" : value.mode,
    );

    for (const properties of Object.values(syntaxProperties)) {
        for (const property of Object.values(properties)) {
            rootStyle.removeProperty(property);
        }
    }
    for (const token of value.syntax) {
        const properties = syntaxProperties[token.role];
        rootStyle.setProperty(properties.foreground, token.foreground);
        if (token.background !== null) {
            rootStyle.setProperty(properties.background, token.background);
        }
        rootStyle.setProperty(
            properties.weight,
            token.bold ? "bold" : "normal",
        );
        rootStyle.setProperty(
            properties.style,
            token.italic ? "italic" : "normal",
        );
    }
    return true;
}

const gate = new GenerationGate();
let current: RenderModel | null = null;

function textNodes(element: HTMLElement): Text[] {
    const walker = document.createTreeWalker(element, NodeFilter.SHOW_TEXT);
    const nodes: Text[] = [];
    for (let node = walker.nextNode(); node; node = walker.nextNode())
        if (!node.parentElement?.closest("button")) nodes.push(node as Text);
    return nodes;
}

function pointAt(element: HTMLElement, offset: number): [Text, number] | null {
    let consumed = 0;
    for (const node of textNodes(element)) {
        if (offset <= consumed + node.length)
            return [node, Math.max(0, offset - consumed)];
        consumed += node.length;
    }
    return null;
}

function pointOffset(element: HTMLElement, node: Node, offset: number): number {
    let consumed = 0;
    for (const text of textNodes(element)) {
        if (text === node) return consumed + offset;
        consumed += text.length;
    }
    return consumed;
}

function selectionBlock(node: Node): HTMLElement | null {
    return (
        (node instanceof Element
            ? node
            : node.parentElement
        )?.closest<HTMLElement>("[data-block-id]") ?? null
    );
}

function captureView() {
    const block = Array.from(
        root.querySelectorAll<HTMLElement>("[data-block-id]"),
    ).find((element) => element.getBoundingClientRect().bottom > 0);
    const selection = window.getSelection();
    const range = selection?.rangeCount ? selection.getRangeAt(0) : null;
    const startBlock = range ? selectionBlock(range.startContainer) : null;
    const endBlock = range ? selectionBlock(range.endContainer) : null;
    const selected =
        range && !range.collapsed && startBlock && endBlock
            ? {
                  text: range.toString(),
                  startBlock: startBlock.dataset.blockId!,
                  startOffset: pointOffset(
                      startBlock,
                      range.startContainer,
                      range.startOffset,
                  ),
                  endBlock: endBlock.dataset.blockId!,
                  endOffset: pointOffset(
                      endBlock,
                      range.endContainer,
                      range.endOffset,
                  ),
              }
            : null;
    return {
        block: block?.dataset.blockId,
        top: block?.getBoundingClientRect().top ?? 0,
        selection: selected,
    };
}

function restoreView(view: ReturnType<typeof captureView>) {
    if (view.block) {
        const block = root.querySelector<HTMLElement>(
            `[data-block-id="${CSS.escape(view.block)}"]`,
        );
        if (block)
            window.scrollBy(0, block.getBoundingClientRect().top - view.top);
    }
    if (!view.selection?.text) return;
    const startBlock = root.querySelector<HTMLElement>(
        `[data-block-id="${CSS.escape(view.selection.startBlock)}"]`,
    );
    const endBlock = root.querySelector<HTMLElement>(
        `[data-block-id="${CSS.escape(view.selection.endBlock)}"]`,
    );
    let start = startBlock && pointAt(startBlock, view.selection.startOffset);
    let end = endBlock && pointAt(endBlock, view.selection.endOffset);
    if (!start || !end) {
        const nodes = textNodes(root);
        const text = nodes.map((node) => node.data).join("");
        const index = text.indexOf(view.selection.text);
        if (index < 0 || text.indexOf(view.selection.text, index + 1) >= 0)
            return;
        start = pointAt(root, index);
        end = pointAt(root, index + view.selection.text.length);
    }
    if (!start || !end) return;
    const range = document.createRange();
    range.setStart(...start);
    range.setEnd(...end);
    if (range.toString() !== view.selection.text) return;
    const selection = window.getSelection();
    selection?.removeAllRanges();
    selection?.addRange(range);
}

function installOutline(model: RenderModel) {
    const outline = document.querySelector<HTMLElement>("#outline");
    const toggle = document.querySelector<HTMLButtonElement>("#outline-toggle");
    if (!outline || !toggle) return;
    outline.replaceChildren();
    for (const heading of model.headings) {
        const link = document.createElement("a");
        link.href = `#${heading.id}`;
        link.textContent = heading.text;
        link.style.marginLeft = `${Math.max(0, heading.level - 1)}rem`;
        link.addEventListener("click", () => {
            const target = document.getElementById(heading.id);
            if (target)
                window.scrollTo(
                    0,
                    target.getBoundingClientRect().top + window.scrollY,
                );
        });
        outline.append(link);
    }
    toggle.hidden = model.headings.length === 0;
    if (toggle.hidden) outline.hidden = true;
}

function installCodeCopy(model: RenderModel) {
    root.querySelectorAll("pre > code").forEach((code, index) => {
        const source = model.codeBlocks[index]?.source;
        if (source === undefined) return;
        const button = document.createElement("button");
        button.className = "code-copy";
        button.type = "button";
        button.textContent = "Copy";
        button.setAttribute("aria-label", "Copy code");
        button.addEventListener("click", async () => {
            try {
                await navigator.clipboard.writeText(source);
            } catch {
                const selection = window.getSelection();
                const range = document.createRange();
                range.selectNodeContents(code);
                selection?.removeAllRanges();
                selection?.addRange(range);
                document.execCommand("copy");
                selection?.removeAllRanges();
            }
            button.textContent = "Copied";
            (window as Window & { mdvrCopyCode?: () => void }).mdvrCopyCode?.();
        });
        code.parentElement?.before(button);
    });
}

async function finishMermaid(
    model: RenderModel,
    generation: number,
): Promise<void> {
    const diagrams = model.blocks.filter(
        (block) => block.kind === "diagram" && block.code,
    );
    for (const block of diagrams) {
        if (!block.code) continue;
        const result = await renderMermaidAsync(block.code.source, block.id);
        if (!gate.isCurrent(generation)) return;
        const host = root.querySelector<HTMLElement>(
            `[data-block-id="${CSS.escape(block.id)}"]`,
        );
        const pending = host?.querySelector<HTMLElement>(
            "[data-renderer=mermaid]",
        );
        if (!pending) continue;
        const parsed = new DOMParser().parseFromString(
            result.html,
            "text/html",
        );
        pending.replaceWith(...Array.from(parsed.body.childNodes));
    }
}

export function loadDocument(source: string, generation = 1): RenderModel {
    gate.begin(generation);
    const view = captureView();
    const model = renderDocument(source, { generation });
    current = model;
    mountDocument(root, model);
    domSearch = { query: "", caseSensitive: false, ranges: [], index: -1 };
    restoreView(view);
    installOutline(model);
    installCodeCopy(model);
    (
        window as Window & { mdvrRequestResources?: () => void }
    ).mdvrRequestResources?.();
    setTimeout(() => {
        (
            window as Window & {
                mdvrPostRenderReady?: (
                    headings: RenderModel["headings"],
                    generation: number,
                ) => void;
            }
        ).mdvrPostRenderReady?.(model.headings, generation);
    }, 0);
    void finishMermaid(model, generation);
    return model;
}

async function copyText(value: string): Promise<boolean> {
    try {
        await navigator.clipboard.writeText(value);
        return true;
    } catch {
        const active = document.activeElement as HTMLElement | null;
        const selection = window.getSelection();
        const ranges = selection
            ? Array.from({ length: selection.rangeCount }, (_, index) =>
                  selection.getRangeAt(index).cloneRange(),
              )
            : [];
        const textarea = document.createElement("textarea");
        textarea.value = value;
        textarea.style.position = "fixed";
        textarea.style.opacity = "0";
        document.body.append(textarea);
        textarea.select();
        const copied = document.execCommand("copy");
        textarea.remove();
        selection?.removeAllRanges();
        ranges.forEach((range) => selection?.addRange(range));
        active?.focus();
        return copied;
    }
}

export function copySource(): Promise<boolean> {
    return copyText(current?.source ?? "");
}

export function copyRendered(): Promise<boolean> {
    return copyText(
        current?.blocks.map((block) => block.text).join("\n\n") ?? "",
    );
}

export async function createResourceUrl(
    mime: string,
    bytes: number[],
): Promise<string | null> {
    if (!/^image\/(?:png|jpeg|webp|gif|svg\+xml)$/.test(mime)) return null;
    const raw = new Uint8Array(bytes);
    if (mime === "image/gif") {
        const bitmap = await createImageBitmap(new Blob([raw], { type: mime }));
        const canvas = document.createElement("canvas");
        canvas.width = bitmap.width;
        canvas.height = bitmap.height;
        canvas.getContext("2d")?.drawImage(bitmap, 0, 0);
        bitmap.close();
        const firstFrame = await new Promise<Blob | null>((resolve) =>
            canvas.toBlob(resolve, "image/png"),
        );
        return firstFrame ? URL.createObjectURL(firstFrame) : null;
    }
    const body: BlobPart =
        mime === "image/svg+xml"
            ? sanitizeResourceSvg(new TextDecoder().decode(raw))
            : raw;
    return URL.createObjectURL(new Blob([body], { type: mime }));
}

let domSearch = {
    query: "",
    caseSensitive: false,
    ranges: [] as Range[],
    index: -1,
};

export function findInDocument(
    query: string,
    caseSensitive = false,
    backwards = false,
): boolean {
    if (!query) return false;
    if (
        domSearch.query !== query ||
        domSearch.caseSensitive !== caseSensitive
    ) {
        root.querySelectorAll("mark[data-mdvr-search]").forEach((mark) =>
            mark.replaceWith(...Array.from(mark.childNodes)),
        );
        root.normalize();
        const needle = caseSensitive ? query : query.toLocaleLowerCase();
        const nodes: Array<{ node: Text; start: number; end: number }> = [];
        const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
        let text = "";
        let previousBlock: Element | null = null;
        for (let node = walker.nextNode(); node; node = walker.nextNode()) {
            const value = node.textContent ?? "";
            const parent = node.parentElement;
            if (!value || parent?.closest("button, [aria-hidden=true]"))
                continue;
            const block = parent?.closest("[data-block-id]") ?? null;
            if (text && block !== previousBlock) text += "\n";
            const start = text.length;
            text += value;
            nodes.push({ node: node as Text, start, end: text.length });
            previousBlock = block;
        }
        const haystack = caseSensitive ? text : text.toLocaleLowerCase();
        const matches: Array<{ start: number; end: number }> = [];
        for (let from = 0; from <= haystack.length - needle.length; ) {
            const index = haystack.indexOf(needle, from);
            if (index < 0) break;
            matches.push({ start: index, end: index + query.length });
            from = index + Math.max(needle.length, 1);
        }
        for (const { node, start, end } of nodes) {
            const overlaps = matches
                .map((match, index) => ({
                    index,
                    start: Math.max(0, match.start - start),
                    end: Math.min(end - start, match.end - start),
                }))
                .filter((match) => match.start < match.end);
            if (!overlaps.length) continue;
            const fragment = document.createDocumentFragment();
            const value = node.data;
            let cursor = 0;
            for (const overlap of overlaps) {
                fragment.append(value.slice(cursor, overlap.start));
                const mark = document.createElement("mark");
                mark.dataset.mdvrSearch = String(overlap.index);
                mark.textContent = value.slice(overlap.start, overlap.end);
                fragment.append(mark);
                cursor = overlap.end;
            }
            fragment.append(value.slice(cursor));
            node.replaceWith(fragment);
        }
        const markedRanges = matches.flatMap((_, index) => {
            const marks = root.querySelectorAll(
                `mark[data-mdvr-search="${index}"]`,
            );
            if (!marks.length) return [];
            const range = document.createRange();
            range.setStartBefore(marks[0]!);
            range.setEndAfter(marks[marks.length - 1]!);
            return [range];
        });
        domSearch = {
            query,
            caseSensitive,
            ranges: markedRanges,
            index: -1,
        };
    }
    if (!domSearch.ranges.length) return false;
    domSearch.index =
        (domSearch.index + (backwards ? -1 : 1) + domSearch.ranges.length) %
        domSearch.ranges.length;
    const range = domSearch.ranges[domSearch.index]!;
    const selection = window.getSelection();
    selection?.removeAllRanges();
    selection?.addRange(range);
    range.startContainer.parentElement?.scrollIntoView({ block: "center" });
    return true;
}

export function restoreSearchSelection(): boolean {
    const range = domSearch.ranges[domSearch.index];
    if (!range) return false;
    const selection = window.getSelection();
    selection?.removeAllRanges();
    selection?.addRange(range);
    return true;
}

export function searchDocument(query: string, caseSensitive = false) {
    return current ? searchRendered(current, query, caseSensitive) : [];
}

Object.assign(window, {
    mdvrFind: findInDocument,
    mdvrLoadDocument: loadDocument,
    mdvrRestoreSearchSelection: restoreSearchSelection,
    mdvrSearchDocument: searchDocument,
    mdvrApplyAppearance: applyAppearance,
    mdvrCopyRendered: copyRendered,
    mdvrCopySource: copySource,
    mdvrCreateResourceUrl: createResourceUrl,
});
