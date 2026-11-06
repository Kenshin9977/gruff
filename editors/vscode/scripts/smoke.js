// Smoke test under plain Node (no VS Code): the manifest and grammars are well formed, and the
// server the extension would pick starts, loads the gsc_lsp fixture workspace and answers.
// Run after `npm run compile` (it loads out/server.js) and with a server binary available.

const assert = require("assert");
const fs = require("fs");
const path = require("path");
const cp = require("child_process");
const rpc = require("vscode-jsonrpc/node");

const ext = path.join(__dirname, "..");
const repo = path.join(ext, "..", "..");

function grammarRegexes(node, out) {
  if (Array.isArray(node)) {
    node.forEach((n) => grammarRegexes(n, out));
  } else if (node && typeof node === "object") {
    for (const k of ["match", "begin", "end"]) {
      if (typeof node[k] === "string") out.push(node[k]);
    }
    Object.values(node).forEach((v) => grammarRegexes(v, out));
  }
  return out;
}

function checkManifest() {
  const pkg = JSON.parse(fs.readFileSync(path.join(ext, "package.json"), "utf8"));
  const ids = pkg.contributes.languages.map((l) => l.id);
  assert.deepStrictEqual(ids, ["gsc", "csc", "gsh"]);
  for (const g of pkg.contributes.grammars) {
    const grammar = JSON.parse(fs.readFileSync(path.join(ext, g.path), "utf8"));
    assert.strictEqual(grammar.scopeName, g.scopeName);
    for (const re of grammarRegexes(grammar, [])) {
      // Oniguruma is a superset of what JS accepts here; this catches typos and bad escapes.
      new RegExp(re);
    }
  }
  const lc = JSON.parse(fs.readFileSync(path.join(ext, "language-configuration.json"), "utf8"));
  const start = new RegExp(lc.folding.markers.start);
  const end = new RegExp(lc.folding.markers.end);
  assert.ok(start.test("\t/#") && end.test("\t#/"), "dev blocks fold");
  assert.ok(start.test("/@") && end.test("@/"), "doc comments fold");
  const settings = Object.keys(pkg.contributes.configuration.properties);
  for (const s of ["gruff.game", "gruff.dataDir", "gruff.libraryRoots", "gruff.hashPackages", "gruff.mode", "gruff.server.path"]) {
    assert.ok(settings.includes(s), s);
  }
  console.log(`manifest ok: ${ids.join(", ")}; ${settings.length} settings`);
}

async function checkServer() {
  const { findServer, serverSettings } = require(path.join(ext, "out", "server.js"));
  const found = findServer(process.env.GSC_SERVER || "", ext);
  assert.ok(found, "no gruff binary (bundle one with `npm run bundle-server`)");
  console.log(`server: ${found.command} (${found.source})`);

  const fixtures = path.join(repo, "crates", "gsc_lsp", "tests", "fixtures");
  const ws = path.join(fixtures, "ws");
  const main = path.join(ws, "scripts", "main.gsc");
  const uri = (p) => "file:///" + p.replace(/\\/g, "/").replace(/^\//, "").replace(/^([A-Za-z]):/, (_, d) => d.toLowerCase() + "%3A");

  const child = cp.spawn(found.command, ["lsp", "--stdio"], { stdio: ["pipe", "pipe", "inherit"] });
  const conn = rpc.createMessageConnection(
    new rpc.StreamMessageReader(child.stdout),
    new rpc.StreamMessageWriter(child.stdin),
  );
  const ready = new Promise((resolve) => conn.onNotification("gsc/ready", resolve));
  const diags = new Promise((resolve) =>
    conn.onNotification("textDocument/publishDiagnostics", (p) => {
      if (p.uri === uri(main)) resolve(p);
    }),
  );
  conn.onRequest("window/workDoneProgress/create", () => null);
  conn.listen();

  const t0 = Date.now();
  const init = await conn.sendRequest("initialize", {
    processId: process.pid,
    rootUri: uri(ws),
    workspaceFolders: [{ uri: uri(ws), name: "ws" }],
    capabilities: { window: { workDoneProgress: true } },
    initializationOptions: serverSettings({ dataDir: path.join(fixtures, "data") }, ws),
  });
  assert.strictEqual(init.serverInfo.name, "gruff");
  conn.sendNotification("initialized", {});
  await ready;
  const text = fs.readFileSync(main, "utf8");
  conn.sendNotification("textDocument/didOpen", {
    textDocument: { uri: uri(main), languageId: "gsc", version: 1, text },
  });
  const d = await diags;
  assert.ok(d.diagnostics.some((x) => x.code === "L001"), JSON.stringify(d));
  const comp = await conn.sendRequest("textDocument/completion", {
    textDocument: { uri: uri(main) },
    position: { line: 6, character: 15 },
  });
  assert.ok(comp.items.some((i) => i.label === "helper"));
  const hover = await conn.sendRequest("textDocument/hover", {
    textDocument: { uri: uri(main) },
    position: { line: 6, character: 17 },
  });
  assert.ok(hover.contents.value.includes("util::helper"));
  console.log(`server ok: ready, diagnostics, completion and hover in ${Date.now() - t0} ms`);
  await conn.sendRequest("shutdown");
  conn.sendNotification("exit");
  await new Promise((resolve) => child.on("exit", resolve));
  conn.dispose();
}

(async () => {
  checkManifest();
  await checkServer();
  console.log("smoke test passed");
})().catch((e) => {
  console.error(e);
  process.exit(1);
});
