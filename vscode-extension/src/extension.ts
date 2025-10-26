import * as fs from "fs";
import * as path from "path";
import * as vscode from "vscode";
import {
  Executable,
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
} from "vscode-languageclient/node";

let client: LanguageClient | undefined;
let clientLog: vscode.OutputChannel | undefined;

export async function activate(
  context: vscode.ExtensionContext
): Promise<void> {
  clientLog = vscode.window.createOutputChannel("Squirrel LSP (client)");
  context.subscriptions.push(clientLog);
  clientLog.appendLine("Activating Squirrel LSP client...");

  context.subscriptions.push(
    vscode.commands.registerCommand("squirrel-lsp.restartServer", async () => {
      clientLog?.appendLine("Restart command invoked");
      await restartClient(context);
    })
  );

  await startClient(context);
}

export async function deactivate(): Promise<void> {
  clientLog?.appendLine("Deactivating Squirrel LSP client");
  if (client) {
    await client.stop();
    client = undefined;
  }
  clientLog?.dispose();
  clientLog = undefined;
}

async function restartClient(context: vscode.ExtensionContext): Promise<void> {
  if (client) {
    await client.stop();
    client = undefined;
  }
  await startClient(context);
}

async function startClient(context: vscode.ExtensionContext): Promise<void> {
  clientLog?.appendLine("Resolving server executable...");
  const executable = await resolveServerExecutable(context);
  if (!executable) {
    clientLog?.appendLine("Server executable not found; aborting start");
    return;
  }

  clientLog?.appendLine(
    `Using server command: ${executable.command} ${
      executable.args?.join(" ") ?? ""
    }`
  );

  const debugExecutable: Executable = {
    ...executable,
    args: [...(executable.args ?? []), "--trace"],
  };

  const serverOptions: ServerOptions = {
    run: executable,
    debug: debugExecutable,
  };

  const clientOptions: LanguageClientOptions = {
    documentSelector: [{ language: "squirrel" }],
    outputChannelName: "Squirrel Language Server",
  };

  client = new LanguageClient(
    "squirrelLanguageServer",
    "Squirrel Language Server",
    serverOptions,
    clientOptions
  );

  client.onDidChangeState((event) => {
    clientLog?.appendLine(`Client state changed: ${event.newState}`);
  });

  try {
    await client.start();
    clientLog?.appendLine("Language client is ready");
    context.subscriptions.push({ dispose: () => client?.stop() });
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    clientLog?.appendLine(`Client start failed: ${message}`);
    vscode.window.showErrorMessage(`Failed to start squirrel-lsp: ${message}`);
    throw error;
  }
}

async function resolveServerExecutable(
  context: vscode.ExtensionContext
): Promise<Executable | undefined> {
  const config = vscode.workspace.getConfiguration("squirrelLsp");
  const configuredPath = config.get<string>("serverPath");
  const candidates: string[] = [];

  if (configuredPath && configuredPath.trim().length > 0) {
    clientLog?.appendLine(`Configured serverPath: ${configuredPath}`);
    candidates.push(configuredPath);
  }

  // Prefer a bundled binary if present in the extension under bin/<platform-arch>/
  const bundled = resolveBundledServerPath(context);
  if (bundled) {
    clientLog?.appendLine(`Found bundled binary: ${bundled}`);
    candidates.push(bundled);
  }

  const workspaceBinary = findWorkspaceBinary();
  if (workspaceBinary) {
    clientLog?.appendLine(`Found workspace binary: ${workspaceBinary}`);
    candidates.push(workspaceBinary);
  }

  const defaultCommand =
    process.platform === "win32" ? "squirrel-lsp.exe" : "squirrel-lsp";
  clientLog?.appendLine(`Adding default command candidate: ${defaultCommand}`);
  candidates.push(defaultCommand);

  for (const candidate of candidates) {
    const executable = await normalizeCandidate(
      candidate,
      context.extensionPath
    );
    if (executable) {
      clientLog?.appendLine(`Resolved executable: ${executable.command}`);
      return executable;
    }
    clientLog?.appendLine(`Candidate did not resolve: ${candidate}`);
  }

  vscode.window.showErrorMessage(
    "Could not locate the squirrel-lsp executable. Set `squirrelLsp.serverPath` to the compiled binary."
  );
  return undefined;
}

function findWorkspaceBinary(): string | undefined {
  const folders = vscode.workspace.workspaceFolders;
  if (!folders || folders.length === 0) {
    return undefined;
  }

  const root = folders[0].uri.fsPath;
  const releasePath = path.join(
    root,
    "target",
    "release",
    platformBinaryName()
  );
  if (fs.existsSync(releasePath)) {
    return releasePath;
  }

  const debugPath = path.join(root, "target", "debug", platformBinaryName());
  if (fs.existsSync(debugPath)) {
    return debugPath;
  }

  return undefined;
}

function platformBinaryName(): string {
  return process.platform === "win32" ? "squirrel-lsp.exe" : "squirrel-lsp";
}

function platformKey(): string {
  const plat = process.platform; // 'win32' | 'darwin' | 'linux'
  let arch = process.arch; // 'x64' | 'arm64' | 'arm' | ...
  if (arch === "x64" || arch === "arm64") {
    return `${plat}-${arch}`;
  }
  return `${plat}-x64`;
}

function resolveBundledServerPath(
  context: vscode.ExtensionContext
): string | null {
  const binDir = path.join(context.extensionPath, "bin", platformKey());
  const bin = path.join(binDir, platformBinaryName());
  if (fs.existsSync(bin)) {
    try {
      if (process.platform !== "win32") {
        fs.chmodSync(bin, 0o755);
      }
    } catch {
      // Non-fatal; continue
    }
    return bin;
  }
  return null;
}

async function normalizeCandidate(
  candidate: string,
  extensionPath: string
): Promise<Executable | undefined> {
  const trimmed = candidate.trim();

  const resolved = path.isAbsolute(trimmed)
    ? trimmed
    : path.join(extensionPath, trimmed);

  if (fs.existsSync(resolved) && fs.statSync(resolved).isFile()) {
    clientLog?.appendLine(`Candidate exists at ${resolved}`);
    return { command: resolved, args: [], options: { env: process.env } };
  }

  if (trimmed === candidate) {
    clientLog?.appendLine(`Falling back to PATH lookup for ${trimmed}`);
    return { command: trimmed, args: [], options: { env: process.env } };
  }

  return undefined;
}
