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
                window.mdvrLoadDocument('# Renderer probe', 1);
                return document.querySelector('#document h1')?.textContent === 'Renderer probe';
            })()
            """) { result, error in
                guard error == nil, result as? Bool == true else {
                    print("FAIL: bundled renderer did not render Markdown")
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
