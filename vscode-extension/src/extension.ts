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

export async function activate(context: vscode.ExtensionContext): Promise<void> {
    context.subscriptions.push(
        vscode.commands.registerCommand("squirrel-lsp.restartServer", async () => {
            await restartClient(context);
        }),
    );

    await startClient(context);
}

export async function deactivate(): Promise<void> {
    if (client) {
        await client.stop();
        client = undefined;
    }
}

async function restartClient(context: vscode.ExtensionContext): Promise<void> {
    if (client) {
        await client.stop();
        client = undefined;
    }
    await startClient(context);
}

async function startClient(context: vscode.ExtensionContext): Promise<void> {
    const executable = await resolveServerExecutable(context);
    if (!executable) {
        return;
    }

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
        clientOptions,
    );

    try {
        await client.start();
        context.subscriptions.push({ dispose: () => client?.stop() });
    } catch (error) {
        vscode.window.showErrorMessage(
            `Failed to start squirrel-lsp: ${error instanceof Error ? error.message : String(error)}`,
        );
    }
}

async function resolveServerExecutable(
    context: vscode.ExtensionContext,
): Promise<Executable | undefined> {
    const config = vscode.workspace.getConfiguration("squirrelLsp");
    const configuredPath = config.get<string>("serverPath");
    const candidates: string[] = [];

    if (configuredPath && configuredPath.trim().length > 0) {
        candidates.push(configuredPath);
    }

    const workspaceBinary = findWorkspaceBinary();
    if (workspaceBinary) {
        candidates.push(workspaceBinary);
    }

    const defaultCommand = process.platform === "win32" ? "squirrel-lsp.exe" : "squirrel-lsp";
    candidates.push(defaultCommand);

    for (const candidate of candidates) {
        const executable = await normalizeCandidate(candidate, context.extensionPath);
        if (executable) {
            return executable;
        }
    }

    vscode.window.showErrorMessage(
        "Could not locate the squirrel-lsp executable. Set `squirrelLsp.serverPath` to the compiled binary.",
    );
    return undefined;
}

function findWorkspaceBinary(): string | undefined {
    const folders = vscode.workspace.workspaceFolders;
    if (!folders || folders.length === 0) {
        return undefined;
    }

    const root = folders[0].uri.fsPath;
    const releasePath = path.join(root, "target", "release", platformBinaryName());
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

async function normalizeCandidate(
    candidate: string,
    extensionPath: string,
): Promise<Executable | undefined> {
    const trimmed = candidate.trim();

    // Allow relative paths from the extension directory
    const resolved = path.isAbsolute(trimmed)
        ? trimmed
        : path.join(extensionPath, trimmed);

    if (fs.existsSync(resolved) && fs.statSync(resolved).isFile()) {
        return { command: resolved, args: [], options: { env: process.env } };
    }

    // Fallback to relying on PATH
    if (trimmed === candidate) {
        return { command: trimmed, args: [], options: { env: process.env } };
    }

    return undefined;
}
