import { expect, test } from "bun:test";

const html = await Bun.file(new URL("../index.html", import.meta.url)).text();
const main = await Bun.file(new URL("../src/main.ts", import.meta.url)).text();
const script = html.slice(
  html.indexOf("const validBridgeAction"),
  html.indexOf("let navigationContext"),
);

test("bridge actions use revision 1 action envelope with nonzero requests", () => {
  expect(script).toContain("revision: 1");
  expect(script).toContain("kind: 'action'");
  expect(script).toContain("request: nextRequestId");
  expect(script).toContain("JSON.stringify(envelope)");
  expect(script).toContain("nextRequestId === Number.MAX_SAFE_INTEGER ? 1");
  expect(script).toContain(
    "payload: { request: nextRequestId, ...navigationContext, action }",
  );
  expect(script).not.toMatch(/\b(path|authority)\b/);
});

test("navigation bridge has no renderer authority", () => {
  expect(html).toContain("kind: 'navigation.request'");
  expect(html).toContain("window.mdvrSetNavigationContext");
  expect(html).not.toContain("window.location");
  expect(html).not.toContain("readFile");
});

test("bridge helper is no-op when WebKit handler is absent", () => {
  expect(script).toContain(
    "const handler = window.webkit?.messageHandlers?.mdvr;",
  );
  expect(script).toContain(
    "if (!handler || typeof handler.postMessage !== 'function') return false;",
  );
  expect(script).toContain(
    "if (!validBridgeAction(action) || !navigationContext) return false;",
  );
  expect(script).toContain("catch {\n            return false;");
});

test("GIF resources are frozen to first PNG frame", () => {
  expect(main).toContain('mime === "image/gif"');
  expect(main).toContain("createImageBitmap");
  expect(main).toContain('canvas.toBlob(resolve, "image/png")');
});

test("local images use context-bound native resource requests", () => {
  expect(html).toContain("kind: 'resource.request'");
  expect(html).toContain("reference: { relative_path: { value: reference } }");
  expect(html).toContain("window.mdvrResolveResource");
  expect(html).toContain("URL.revokeObjectURL(url)");
});

test("command palette is keyboard accessible and authority-free", () => {
  expect(html).toContain('aria-label="Command palette"');
  expect(html).toContain("if (event.shiftKey) openPalette()");
  expect(html).toContain("palette.querySelector('button').focus()");
  expect(html).toContain(
    "postNativeAction({ kind: 'focus', payload: 'renderer' })",
  );
});

test("open controls request native file and folder pickers", () => {
  expect(html).toContain('data-open="file"');
  expect(html).toContain('data-open="folder"');
  expect(html).toContain("kind: 'open'");
  expect(html).toContain("event.shiftKey ? 'folder' : 'file'");
});

test("history controls and shortcuts stay native-owned", () => {
  expect(html).toContain('aria-label="Document navigation"');
  expect(html).toContain("kind: 'history'");
  expect(html).toContain("['[', ']', 'r'].includes(event.key.toLowerCase())");
  expect(html).not.toContain("history.back");
});

test("theme chooser sends only closed native choices", () => {
  expect(html).toContain('aria-label="Reader theme"');
  expect(html).toContain("kind: 'theme'");
  expect(html).toContain(
    "['system', 'light', 'dark', 'import'].includes(action.payload)",
  );
  expect(html).toContain("action.payload?.named");
  expect(main).toContain("theme.value = value.mode");
});

test("text scale shortcuts stay native-owned", () => {
  expect(html).toContain("kind: 'text_scale'");
  expect(html).toContain("['+', '=', '-', '0'].includes(event.key)");
  expect(html).not.toContain("localStorage");
});

test("document search is keyboard accessible and wraps", () => {
  expect(html).toContain('role="search"');
  expect(html).toContain("event.metaKey && event.key.toLowerCase() === 'f'");
  expect(html).toContain(
    "window.find(searchInput.value, false, backwards, true",
  );
  expect(html).toContain("event.key === 'Escape'");
});

test("reload restores visible block and unchanged selection", () => {
  expect(main).toContain("const view = captureView()");
  expect(main).toContain("restoreView(view)");
  expect(main).toContain("CSS.escape(view.block)");
  expect(main).toContain("text.indexOf(view.selection)");
});

test("outline is generated safely from rendered headings", () => {
  expect(html).toContain('aria-label="Document outline"');
  expect(main).toContain("for (const heading of model.headings)");
  expect(main).toContain("link.textContent = heading.text");
  expect(main).toContain("installOutline(model)");
});

test("source copy uses exact Markdown and restores focus selection", () => {
  expect(html).toContain("data-copy-source");
  expect(main).toContain("navigator.clipboard.writeText(current.source)");
  expect(main).toContain("selection.getRangeAt(index).cloneRange()");
  expect(main).toContain("active?.focus()");
});

test("code copy uses exact model source with clipboard fallback", () => {
  expect(main).toContain("model.codeBlocks[index]?.source");
  expect(main).toContain("navigator.clipboard.writeText(source)");
  expect(main).toContain('document.execCommand("copy")');
});

test("only requested native actions are wired", () => {
  expect(html).toContain("kind: 'search'");
  expect(html).toContain("kind: 'copy'");
  expect(html).toContain("kind: 'select_all'");
  expect(html).toContain("kind: step < 0 ? 'previous' : 'next'");
  expect(html).not.toContain("window.location");
  expect(html).not.toContain("readFile");
});
