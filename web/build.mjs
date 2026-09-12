// WKWebView file URLs cannot fetch ES modules. The HTML build emits one
// self-contained bundle; load it after the document parses, without CORS.
const result = Bun.spawnSync(["bun", "build", "./index.html", "--outdir", "dist", "--minify"], { stdout: "inherit", stderr: "inherit" });
if (result.exitCode !== 0) process.exit(result.exitCode);
const html = await Bun.file("dist/index.html").text();
await Bun.write("dist/index.html", html.replaceAll(" crossorigin", "").replaceAll('type="module"', "defer"));
