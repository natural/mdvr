import { expect, test } from "bun:test";

const html = await Bun.file(new URL("../index.html", import.meta.url)).text();
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

test("local images use context-bound native resource requests", () => {
  expect(html).toContain("kind: 'resource.request'");
  expect(html).toContain("reference: { relative_path: { value: reference } }");
  expect(html).toContain("window.mdvrResolveResource");
  expect(html).toContain("URL.revokeObjectURL(url)");
});

test("only requested native actions are wired", () => {
  expect(html).toContain("kind: 'search'");
  expect(html).toContain("kind: 'copy'");
  expect(html).toContain("kind: 'select_all'");
  expect(html).toContain("kind: step < 0 ? 'previous' : 'next'");
  expect(html).not.toContain("window.location");
  expect(html).not.toContain("readFile");
});
