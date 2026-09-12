import "./reader.css";
import "katex/dist/katex.min.css";
import {
    GenerationGate,
    mountDocument,
    renderDocument,
    renderMermaidAsync,
    sanitizeGeneratedSvg,
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

function captureView() {
    const block = Array.from(
        root.querySelectorAll<HTMLElement>("[data-block-id]"),
    ).find((element) => element.getBoundingClientRect().bottom > 0);
    return {
        block: block?.dataset.blockId,
        top: block?.getBoundingClientRect().top ?? 0,
        selection: window.getSelection()?.toString() ?? "",
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
    if (!view.selection) return;
    const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
    const nodes: Text[] = [];
    let text = "";
    for (let node = walker.nextNode(); node; node = walker.nextNode()) {
        nodes.push(node as Text);
        text += node.textContent ?? "";
    }
    const start = text.indexOf(view.selection);
    if (start < 0) return;
    let offset = 0;
    let startNode: Text | undefined;
    let endNode: Text | undefined;
    let startOffset = 0;
    let endOffset = 0;
    for (const node of nodes) {
        const length = node.data.length;
        if (!startNode && start <= offset + length) {
            startNode = node;
            startOffset = start - offset;
        }
        if (start + view.selection.length <= offset + length) {
            endNode = node;
            endOffset = start + view.selection.length - offset;
            break;
        }
        offset += length;
    }
    if (!startNode || !endNode) return;
    const range = document.createRange();
    range.setStart(startNode, startOffset);
    range.setEnd(endNode, endOffset);
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
    restoreView(view);
    installOutline(model);
    installCodeCopy(model);
    (
        window as Window & { mdvrRequestResources?: () => void }
    ).mdvrRequestResources?.();
    void finishMermaid(model, generation);
    return model;
}

export function createResourceUrl(
    mime: string,
    bytes: number[],
): string | null {
    if (!/^image\/(?:png|jpeg|webp|svg\+xml)$/.test(mime)) return null;
    const raw = new Uint8Array(bytes);
    const body: BlobPart =
        mime === "image/svg+xml"
            ? sanitizeGeneratedSvg(new TextDecoder().decode(raw))
            : raw;
    return URL.createObjectURL(new Blob([body], { type: mime }));
}

export function searchDocument(query: string, caseSensitive = false) {
    return current ? searchRendered(current, query, caseSensitive) : [];
}

Object.assign(window, {
    mdvrLoadDocument: loadDocument,
    mdvrSearchDocument: searchDocument,
    mdvrApplyAppearance: applyAppearance,
    mdvrCreateResourceUrl: createResourceUrl,
});
