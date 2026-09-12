const root = new URL("../../web/", import.meta.url);
const packages = [];
for await (const relative of new Bun.Glob("node_modules/**/package.json").scan({
  cwd: root.pathname,
})) {
  if (relative.includes("/node_modules/")) continue;
  const directory = relative.slice(0, -"package.json".length);
  const metadata = await Bun.file(new URL(relative, root)).json();
  if (!metadata.name || !metadata.version) continue;
  let notice = "";
  for (const name of [
    "LICENSE",
    "LICENSE.md",
    "LICENSE.txt",
    "LICENSE-MIT.txt",
    "LICENCE",
    "COPYING",
    "NOTICE",
  ]) {
    const file = Bun.file(new URL(directory + name, root));
    if (await file.exists()) {
      notice = (await file.text()).trim();
      break;
    }
  }
  packages.push({
    name: metadata.name,
    version: metadata.version,
    license: metadata.license ?? "UNKNOWN",
    notice,
  });
}
packages.sort(
  (a, b) => a.name.localeCompare(b.name) || a.version.localeCompare(b.version),
);
let output =
  "# Third-party notices\n\nGenerated from frozen `web/bun.lock` installation by `scripts/verify/generate-notices.mjs`.\n\n| Package | Version | Declared license | License file |\n| --- | --- | --- | --- |\n";
for (const item of packages)
  output += `| ${item.name.replaceAll("|", "\\|")} | ${item.version} | ${String(item.license).replaceAll("|", "\\|")} | ${item.notice ? "included below" : "missing"} |\n`;
for (const item of packages.filter((item) => item.notice))
  output += `\n## ${item.name} ${item.version}\n\nDeclared license: ${item.license}\n\n\`\`\`text\n${item.notice.replaceAll("```", "` ` `")}\n\`\`\`\n`;
await Bun.write(
  new URL("../../THIRD_PARTY_NOTICES.md", import.meta.url),
  output,
);
console.log(`wrote ${packages.length} package notices`);
