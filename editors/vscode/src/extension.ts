// The VS Code client of the Gruff language server (`gruff lsp`).

import * as vscode from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  State,
  TransportKind,
} from "vscode-languageclient/node";

import { findServer, GscSettings, serverSettings } from "./server";

let client: LanguageClient | undefined;
let status: vscode.StatusBarItem | undefined;
let output: vscode.LogOutputChannel | undefined;

function settings(): GscSettings {
  const c = vscode.workspace.getConfiguration("gruff");
  const folder = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
  return serverSettings(
    {
      game: c.get<string>("game"),
      dataDir: c.get<string>("dataDir"),
      libraryRoots: c.get<string[]>("libraryRoots"),
      hashPackages: c.get<string[]>("hashPackages"),
      mode: c.get<string>("mode"),
    },
    folder,
  );
}

async function refreshStatus(): Promise<void> {
  if (!client || !status) {
    return;
  }
  try {
    const s: any = await client.sendRequest("gsc/status");
    if (s?.loading) {
      status.text = "$(sync~spin) Gruff";
      status.tooltip = "Loading the GSC workspace";
    } else {
      status.text = `Gruff: ${s.game}`;
      const warnings: string[] = s.warnings ?? [];
      status.tooltip =
        `${s.files} files (${s.primaryFiles} reported), game ${s.game} (${s.gameSource}, ${s.mode}), ` +
        `${s.builtins} builtins` +
        (warnings.length > 0 ? `\n${warnings.join("\n")}` : "");
    }
    status.show();
  } catch {
    // The server is starting or stopping.
  }
}

async function start(context: vscode.ExtensionContext): Promise<void> {
  const cfg = vscode.workspace.getConfiguration("gruff");
  const found = findServer(cfg.get<string>("server.path"), context.extensionPath);
  if (!found) {
    const msg =
      "Gruff: cannot find the `gruff` binary. Set `gruff.server.path`, or put `gruff` on PATH.";
    vscode.window.showErrorMessage(msg);
    output?.error(msg);
    return;
  }
  output?.info(`using ${found.command} (${found.source})`);
  const serverOptions: ServerOptions = {
    command: found.command,
    args: ["lsp"],
    transport: TransportKind.stdio,
  };
  const clientOptions: LanguageClientOptions = {
    documentSelector: [
      { scheme: "file", language: "gsc" },
      { scheme: "file", language: "csc" },
      { scheme: "file", language: "gsh" },
      { scheme: "untitled", language: "gsc" },
    ],
    initializationOptions: settings(),
    outputChannel: output,
    synchronize: {
      fileEvents: [
        vscode.workspace.createFileSystemWatcher("**/*.{gsc,csc,gsh,GSC,CSC,GSH}"),
        vscode.workspace.createFileSystemWatcher(
          "**/{gruff.toml,.gruff.toml,gsc.toml,.gsc.toml,pyproject.toml}",
        ),
      ],
    },
  };
  client = new LanguageClient("gruff", "Gruff Language Server", serverOptions, clientOptions);
  client.onDidChangeState((e) => {
    if (e.newState === State.Running) {
      void refreshStatus();
    }
  });
  await client.start();
  client.onNotification("gsc/ready", () => void refreshStatus());
}

async function stop(): Promise<void> {
  const c = client;
  client = undefined;
  if (c) {
    await c.stop();
  }
}

/** What tests reach through `extension.exports`. */
export interface Api {
  client(): LanguageClient | undefined;
}

export async function activate(context: vscode.ExtensionContext): Promise<Api> {
  output = vscode.window.createOutputChannel("Gruff", { log: true });
  status = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 0);
  status.command = "gruff.showStatus";
  status.text = "$(sync~spin) Gruff";
  context.subscriptions.push(output, status);

  context.subscriptions.push(
    vscode.commands.registerCommand("gruff.restartServer", async () => {
      await stop();
      await start(context);
    }),
    vscode.commands.registerCommand("gruff.showStatus", async () => {
      await refreshStatus();
      output?.show(true);
    }),
    // What a "N references" code lens runs: the server sends plain JSON, the peek view wants
    // VS Code's own types.
    vscode.commands.registerCommand(
      "gruff.showReferences",
      async (uri: string, line: number, character: number) => {
        const u = vscode.Uri.parse(uri);
        const p = new vscode.Position(line, character);
        const locations = await vscode.commands.executeCommand<vscode.Location[]>(
          "vscode.executeReferenceProvider",
          u,
          p,
        );
        await vscode.commands.executeCommand("editor.action.showReferences", u, p, locations ?? []);
      },
    ),
    vscode.workspace.onDidChangeConfiguration(async (e) => {
      if (!e.affectsConfiguration("gruff")) {
        return;
      }
      if (e.affectsConfiguration("gruff.server.path")) {
        await stop();
        await start(context);
      } else if (client) {
        await client.sendNotification("workspace/didChangeConfiguration", {
          settings: { gruff: settings() },
        });
      }
    }),
  );
  await start(context);
  return { client: () => client };
}

export function deactivate(): Thenable<void> | undefined {
  return stop();
}
