// Run the extension tests in VS Code. Uses an installed VS Code when one is found (VSCODE_EXE,
// or the usual install locations); downloads one only when GSC_TEST_DOWNLOAD=1.

import * as fs from "fs";
import * as os from "os";
import * as path from "path";
import { runTests } from "@vscode/test-electron";

function installedVsCode(): string | undefined {
  const candidates = [
    process.env.VSCODE_EXE,
    process.env.LOCALAPPDATA && path.join(process.env.LOCALAPPDATA, "Programs", "Microsoft VS Code", "Code.exe"),
    process.env.ProgramFiles && path.join(process.env.ProgramFiles, "Microsoft VS Code", "Code.exe"),
    "/usr/share/code/code",
    "/Applications/Visual Studio Code.app/Contents/MacOS/Electron",
  ];
  return candidates.find((c): c is string => !!c && fs.existsSync(c));
}

async function main(): Promise<void> {
  const ext = path.resolve(__dirname, "..", "..");
  const fixtures = path.resolve(ext, "..", "..", "crates", "gsc_lsp", "tests", "fixtures");
  const exe = installedVsCode();
  if (!exe && process.env.GSC_TEST_DOWNLOAD !== "1") {
    console.error("no VS Code found: set VSCODE_EXE, or GSC_TEST_DOWNLOAD=1 to download one");
    process.exit(1);
  }
  const userData = fs.mkdtempSync(path.join(os.tmpdir(), "gruff-vscode-test-"));
  try {
    await runTests({
      vscodeExecutablePath: exe,
      extensionDevelopmentPath: ext,
      extensionTestsPath: path.join(__dirname, "suite"),
      launchArgs: [
        path.join(fixtures, "ws"),
        "--disable-extensions",
        "--disable-workspace-trust",
        "--skip-welcome",
        "--skip-release-notes",
        `--user-data-dir=${userData}`,
      ],
      extensionTestsEnv: { GSC_TEST_DATA: path.join(fixtures, "data") },
    });
  } finally {
    fs.rmSync(userData, { recursive: true, force: true });
  }
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
