import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const repo = fileURLToPath(new URL("../../", import.meta.url));
const web = join(repo, "web");
const packages = [];

function licenseText(directory, explicit) {
  const paths = new Set();
  if (explicit) paths.add(join(directory, explicit));
  for (const name of readdirSync(directory)) {
    if (/^(?:licen[cs]e|copying|notice)(?:[._-].*)?$/i.test(name))
      paths.add(join(directory, name));
  }
  return [...paths]
    .filter((path) => existsSync(path) && statSync(path).isFile())
    .sort()
    .map(
      (path) =>
        `--- ${path.slice(directory.length + 1)} ---\n${readFileSync(
          path,
          "utf8",
        )
          .trim()
          .replace(/^\t/gm, " ")
          .replace(/[ \t]+$/gm, "")}`,
    )
    .join("\n\n");
}

for await (const relative of new Bun.Glob("node_modules/**/package.json").scan({
  cwd: web,
})) {
  if (relative.includes("/node_modules/")) continue;
  const metadata = await Bun.file(join(web, relative)).json();
  if (!metadata.name || !metadata.version) continue;
  const directory = dirname(join(web, relative));
  packages.push({
    ecosystem: "web",
    name: metadata.name,
    version: metadata.version,
    license: metadata.license ?? "UNKNOWN",
    notice: licenseText(directory, metadata.license_file),
  });
}

const cargo = Bun.spawnSync(
  ["cargo", "metadata", "--locked", "--format-version", "1"],
  { cwd: repo, stdout: "pipe", stderr: "inherit" },
);
const tree = Bun.spawnSync(
  [
    "cargo",
    "tree",
    "--locked",
    "--package",
    "reqwest",
    "--target",
    "aarch64-apple-darwin",
    "--edges",
    "normal",
    "--prefix",
    "none",
    "--format",
    "{p}",
  ],
  { cwd: repo, stdout: "pipe", stderr: "inherit" },
);
if (cargo.exitCode !== 0 || tree.exitCode !== 0) process.exit(1);
let cargoMetadata;
try {
  cargoMetadata = JSON.parse(cargo.stdout.toString());
} catch {
  console.error("cargo metadata returned invalid JSON");
  process.exit(1);
}
const compiled = new Set(
  tree.stdout
    .toString()
    .split("\n")
    .map((line) => line.replace(/ \(\*\)$/, "")),
);
for (const item of cargoMetadata.packages) {
  if (!item.source || !compiled.has(`${item.name} v${item.version}`)) continue;
  const directory = dirname(item.manifest_path);
  packages.push({
    ecosystem: "cargo",
    name: item.name,
    version: item.version,
    license: item.license ?? "UNKNOWN",
    notice: licenseText(directory, item.license_file),
  });
}

packages.sort(
  (a, b) =>
    a.ecosystem.localeCompare(b.ecosystem) ||
    a.name.localeCompare(b.name) ||
    a.version.localeCompare(b.version),
);
let output =
  "# Third-party notices\n\nGenerated from frozen remote-fetch and web dependency graphs in `Cargo.lock` and `web/bun.lock` by `scripts/verify/generate-notices.mjs`.\n\n| Ecosystem | Package | Version | Declared license | License file |\n| --- | --- | --- | --- | --- |\n";
for (const item of packages)
  output += `| ${item.ecosystem} | ${item.name.replaceAll("|", "\\|")} | ${item.version} | ${String(item.license).replaceAll("|", "\\|")} | ${item.notice ? "included below" : "missing"} |\n`;
for (const item of packages.filter((item) => item.notice))
  output += `\n## ${item.ecosystem}: ${item.name} ${item.version}\n\nDeclared license: ${item.license}\n\n\`\`\`text\n${item.notice.replaceAll("```", "` ` `")}\n\`\`\`\n`;
await Bun.write(join(repo, "THIRD_PARTY_NOTICES.md"), output);
console.log(`wrote ${packages.length} dependency notices`);
