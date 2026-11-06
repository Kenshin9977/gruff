// Extension tests, run inside VS Code on the gsc_lsp fixture workspace.

import * as assert from "assert";
import * as path from "path";
import * as vscode from "vscode";

async function until<T>(what: string, f: () => T | undefined | Promise<T | undefined>, ms = 30000): Promise<T> {
  const end = Date.now() + ms;
  for (;;) {
    const v = await f();
    if (v !== undefined) {
      return v;
    }
    if (Date.now() > end) {
      throw new Error(`timed out waiting for ${what}`);
    }
    await new Promise((r) => setTimeout(r, 100));
  }
}

export async function run(): Promise<void> {
  const data = process.env.GSC_TEST_DATA;
  assert.ok(data, "GSC_TEST_DATA");
  await vscode.workspace.getConfiguration("gruff").update("dataDir", data, vscode.ConfigurationTarget.Global);

  const folder = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
  assert.ok(folder, "a workspace folder");
  const main = vscode.Uri.file(path.join(folder, "scripts", "main.gsc"));
  const doc = await vscode.workspace.openTextDocument(main);
  assert.strictEqual(doc.languageId, "gsc");
  const csc = await vscode.languages.getLanguages();
  assert.ok(csc.includes("csc") && csc.includes("gsh"), "csc and gsh are registered");
  await vscode.window.showTextDocument(doc);

  const ext = vscode.extensions.all.find((e) => e.packageJSON.name === "gruff");
  assert.ok(ext, "the extension is installed");
  await ext.activate();
  const api = ext.exports as { client(): unknown };
  await until("the client", () => (api.client() ? true : undefined));

  // Diagnostics from the server: L001 on the loop of `spin`.
  const l001 = await until("L001", () =>
    vscode.languages.getDiagnostics(main).find((d) => String(d.code) === "L001"),
  );
  assert.strictEqual(l001.range.start.line, 14);

  const comp = await until("completions", async () => {
    const list = await vscode.commands.executeCommand<vscode.CompletionList>(
      "vscode.executeCompletionItemProvider",
      main,
      new vscode.Position(6, 15),
    );
    const labels = list.items.map((i) => (typeof i.label === "string" ? i.label : i.label.label));
    return labels.includes("helper") ? labels : undefined;
  });
  assert.ok(comp.includes("helper"));

  const defs = await vscode.commands.executeCommand<(vscode.Location | vscode.LocationLink)[]>(
    "vscode.executeDefinitionProvider",
    main,
    new vscode.Position(6, 17),
  );
  assert.strictEqual(defs.length, 1);
  const target = "targetUri" in defs[0] ? defs[0].targetUri : defs[0].uri;
  assert.ok(target.fsPath.toLowerCase().endsWith("util.gsc"), target.fsPath);

  const hovers = await vscode.commands.executeCommand<vscode.Hover[]>(
    "vscode.executeHoverProvider",
    main,
    new vscode.Position(6, 17),
  );
  const md = hovers.flatMap((h) => h.contents).map((c) => (typeof c === "string" ? c : (c as vscode.MarkdownString).value)).join("\n");
  assert.ok(md.includes("util::helper"), md);

  const edits = await vscode.commands.executeCommand<vscode.TextEdit[]>(
    "vscode.executeFormatDocumentProvider",
    main,
    { tabSize: 4, insertSpaces: false },
  );
  assert.ok(edits.length >= 1, "formatting edits");

  const rename = await vscode.commands.executeCommand<vscode.WorkspaceEdit>(
    "vscode.executeDocumentRenameProvider",
    main,
    new vscode.Position(6, 17),
    "add_two",
  );
  assert.strictEqual(rename.entries().length, 2);

  const legend = await vscode.commands.executeCommand<vscode.SemanticTokensLegend>(
    "vscode.provideDocumentSemanticTokensLegend",
    main,
  );
  assert.ok(legend.tokenTypes.includes("hashLiteral"));
  const tokens = await vscode.commands.executeCommand<vscode.SemanticTokens>(
    "vscode.provideDocumentSemanticTokens",
    main,
  );
  assert.ok(tokens.data.length > 0 && tokens.data.length % 5 === 0);

  console.log("extension tests passed");
}
