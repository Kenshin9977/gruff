// Copy the release `gruff` binary into server/<platform>-<arch>/ for packaging.
// GSC_BIN overrides the source (default: ../../target/release/gruff[.exe]).

const fs = require("fs");
const path = require("path");

const exe = process.platform === "win32" ? "gruff.exe" : "gruff";
const src = process.env.GSC_BIN || path.join(__dirname, "..", "..", "..", "target", "release", exe);
const dir = path.join(__dirname, "..", "server", `${process.platform}-${process.arch}`);

if (!fs.existsSync(src)) {
  console.error(`no server binary at ${src}: run \`cargo build --release\` first (or set GSC_BIN)`);
  process.exit(1);
}
fs.mkdirSync(dir, { recursive: true });
fs.copyFileSync(src, path.join(dir, exe));
if (process.platform !== "win32") {
  fs.chmodSync(path.join(dir, exe), 0o755);
}
console.log(`bundled ${src} -> ${path.relative(process.cwd(), path.join(dir, exe))}`);
