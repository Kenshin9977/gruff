// Bundle the extension client (src/extension.ts and its runtime dependencies) into one file,
// dist/extension.js, so the .vsix carries no node_modules. `vscode` is provided by the host.
// The licences of the bundled packages go to dist/THIRD_PARTY_NOTICES.txt.
// `--dev` keeps a source map and skips minification.

const esbuild = require("esbuild");
const fs = require("fs");
const path = require("path");

const dev = process.argv.includes("--dev");
const root = path.join(__dirname, "..");
const dist = path.join(root, "dist");

/** The package directories under node_modules that contributed to the bundle. */
function bundledPackages(metafile) {
  const dirs = new Set();
  for (const input of Object.keys(metafile.inputs)) {
    const parts = input.split(/[\\/]/);
    const i = parts.lastIndexOf("node_modules");
    if (i < 0) continue;
    const n = parts[i + 1].startsWith("@") ? 2 : 1;
    dirs.add(path.join(root, ...parts.slice(0, i + 1 + n)));
  }
  return [...dirs].sort();
}

function notices(dirs) {
  const out = [
    "Third-party software bundled in dist/extension.js",
    "",
  ];
  for (const dir of dirs) {
    const pkg = JSON.parse(fs.readFileSync(path.join(dir, "package.json"), "utf8"));
    const lic = fs.readdirSync(dir).find((f) => /^(licen[cs]e|copying)(\.|$)/i.test(f));
    out.push("=".repeat(78), `${pkg.name} ${pkg.version} (${pkg.license || "see below"})`, "");
    out.push(lic ? fs.readFileSync(path.join(dir, lic), "utf8").trim() : "(no licence file)", "");
  }
  return out.join("\n");
}

esbuild
  .build({
    entryPoints: [path.join(root, "src", "extension.ts")],
    outfile: path.join(dist, "extension.js"),
    bundle: true,
    platform: "node",
    format: "cjs",
    target: "node20",
    external: ["vscode"],
    minify: !dev,
    sourcemap: dev,
    metafile: true,
    logLevel: "info",
  })
  .then((result) => {
    const dirs = bundledPackages(result.metafile);
    fs.writeFileSync(path.join(dist, "THIRD_PARTY_NOTICES.txt"), notices(dirs));
    console.log(`  dist/THIRD_PARTY_NOTICES.txt  ${dirs.length} packages`);
  })
  .catch(() => process.exit(1));
