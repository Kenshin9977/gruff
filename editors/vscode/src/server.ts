// Locating the gruff binary and shaping the settings the server reads. No `vscode` import here,
// so the smoke test can load this module under plain Node.

import * as fs from "fs";
import * as path from "path";

export interface Found {
  command: string;
  /** `setting`, `bundled` or `PATH`. */
  source: string;
}

/** The binary's name; `gsc` is its deprecated former name, still looked for on `PATH`. */
export function exeName(platform: string = process.platform, base: string = "gruff"): string {
  return platform === "win32" ? `${base}.exe` : base;
}

/** The names looked for on `PATH`, in order. */
export const PATH_NAMES = ["gruff", "gsc"];

/** `server/<platform>-<arch>/gruff[.exe]` inside the extension. */
export function bundledPath(extensionDir: string): string {
  return path.join(extensionDir, "server", `${process.platform}-${process.arch}`, exeName());
}

/** `gruff` on `PATH`, else the deprecated `gsc`. */
export function onPath(env: NodeJS.ProcessEnv = process.env): string | undefined {
  const dirs = (env.PATH ?? env.Path ?? "").split(path.delimiter).filter((d) => d.length > 0);
  for (const name of PATH_NAMES) {
    for (const d of dirs) {
      const candidate = path.join(d, exeName(process.platform, name));
      if (fs.existsSync(candidate)) {
        return candidate;
      }
    }
  }
  return undefined;
}

/** The binary to run: the `gruff.server.path` setting, then the bundled one, then `gruff` (else
 * the deprecated `gsc`) on `PATH`. */
export function findServer(setting: string | undefined, extensionDir: string): Found | undefined {
  const s = (setting ?? "").trim();
  if (s.length > 0) {
    return fs.existsSync(s) ? { command: s, source: "setting" } : undefined;
  }
  const bundled = bundledPath(extensionDir);
  if (fs.existsSync(bundled)) {
    return { command: bundled, source: "bundled" };
  }
  const p = onPath();
  return p ? { command: p, source: "PATH" } : undefined;
}

export interface GscSettings {
  game?: string;
  dataDir?: string;
  libraryRoots?: string[];
  hashPackages?: string[];
  mode?: string;
}

/** The settings as the server reads them, with `${workspaceFolder}` expanded. */
export function serverSettings(raw: GscSettings, workspaceFolder: string | undefined): GscSettings {
  const expand = (s: string) =>
    workspaceFolder ? s.split("${workspaceFolder}").join(workspaceFolder) : s;
  return {
    game: raw.game ?? "auto",
    dataDir: expand(raw.dataDir ?? ""),
    libraryRoots: (raw.libraryRoots ?? []).map(expand),
    hashPackages: (raw.hashPackages ?? []).map(expand),
    mode: raw.mode ?? "retail",
  };
}
