// Run after `cd web && bun run build`:
// swift scripts/verify/check-renderer.swift web/dist/index.html
import Cocoa
import WebKit

let app = NSApplication.shared
app.setActivationPolicy(.prohibited)

final class RendererProbe: NSObject, WKNavigationDelegate {
    var view: WKWebView!

    func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
        webView.evaluateJavaScript("""
            (() => {
                if (typeof window.mdvrLoadDocument !== 'function') return false;
                const source = '# Renderer probe\\n\\nhello **world**\\n\\n<img src="assets/raw.png" alt="raw">\\n\\n`$literal$` and $math$';
                window.mdvrLoadDocument(source, 1);
                const paragraph = [...document.querySelectorAll('#document p')].find((node) => node.textContent === 'hello world');
                const range = document.createRange();
                range.setStart(paragraph.firstChild, 0);
                range.setEnd(paragraph.querySelector('strong').firstChild, 5);
                getSelection().removeAllRanges();
                getSelection().addRange(range);
                window.mdvrLoadDocument('# Inserted\\n\\n' + source, 2);
                const selectionPreserved = getSelection().toString() === 'hello world';
                const foundAcrossInlineNodes = window.mdvrFind('hello world', false, false);
                return [
                    document.querySelector('#document h1')?.textContent === 'Inserted',
                    selectionPreserved,
                    foundAcrossInlineNodes,
                    document.querySelectorAll('mark[data-mdvr-search="0"]').length === 2,
                    document.querySelector('code')?.textContent === '$literal$',
                    Boolean(document.querySelector('.katex')),
                    document.querySelector('img[data-mdvr-resource="assets/raw.png"]') !== null,
                ];
            })()
            """) { result, error in
                guard error == nil, let checks = result as? [Bool], checks.allSatisfy({ $0 }) else {
                    print("FAIL: bundled renderer checks failed: \(String(describing: result)) \(String(describing: error))")
                    exit(1)
                }
                print("PASS: restricted file-URL renderer rendered Markdown")
                exit(0)
            }
    }

    func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: Error) {
        print("FAIL: renderer navigation failed")
        exit(1)
    }
}

guard CommandLine.arguments.count == 2 else {
    print("Usage: swift scripts/verify/check-renderer.swift web/dist/index.html")
    exit(2)
}
let probe = RendererProbe()
let configuration = WKWebViewConfiguration()
configuration.websiteDataStore = .nonPersistent()
probe.view = WKWebView(frame: NSRect(x: 0, y: 0, width: 800, height: 600), configuration: configuration)
probe.view.navigationDelegate = probe
let entry = URL(fileURLWithPath: CommandLine.arguments[1]).standardizedFileURL
probe.view.loadFileURL(entry, allowingReadAccessTo: entry.deletingLastPathComponent())
DispatchQueue.main.asyncAfter(deadline: .now() + 20) {
    print("FAIL: renderer startup timed out")
    exit(1)
}
app.run()
