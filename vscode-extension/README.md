# Squirrel Language Server VS Code Extension

This extension wraps the Rust-based [`squirrel-lsp`](../) language server and wires it into VS Code via the Language Server Protocol. Right now the server focuses on document formatting for Squirrel (`.nut`) files.

## Prerequisites

1. **Rust toolchain** – required to build the `squirrel-lsp` binary.
2. **Node.js 18+** – required to build and package the extension.
3. **VS Code 1.90.0 or newer** – matches the engine version declared in `package.json`.

## Build the server binary

```fish
cd (dirname (status --current-file))/..  # repo root
cargo build --release
```

The compiled binary will live at `target/release/squirrel-lsp` (`.exe` on Windows). Keep track of the absolute path—you can point the extension at it.

## Install dependencies & compile the extension

```fish
cd squirrel-lsp/vscode-extension
npm install
npm run compile
```

`npm run compile` emits the transpiled JavaScript into `dist/`.

## Launching in VS Code

1. Open the repository root in VS Code.
2. Run `npm install` and `npm run compile` as shown above.
3. Press `F5` in VS Code to launch an **Extension Development Host**. The extension defined in this folder will activate when you open a `.nut` file.
4. When prompted, configure the `squirrel LSP: Server Path` setting with the absolute path to the compiled `squirrel-lsp` executable. If the binary lives in `target/release`, the extension will normally discover it automatically.

With the extension running, use **Format Document** (`Shift+Alt+F` / `Shift+Option+F`) to format Squirrel files through the language server.

## Packaging for distribution

```fish
npm run package
```

This produces a `.vsix` file you can share or publish. Update the `publisher`, `name`, and version in `package.json` before publishing to the marketplace.

## Troubleshooting

- **Server binary not found** – set `squirrelLsp.serverPath` to the absolute binary path. The extension also searches the workspace `target/release` and `target/debug` folders and finally falls back to `squirrel-lsp` on `PATH`.
- **Permission denied on Unix** – ensure the binary is executable: `chmod +x target/release/squirrel-lsp`.
- **Formatting command slow** – build the server in release mode and point the extension at the release binary.

## Commands

- `Squirrel LSP: Restart Server` – manually restarts the language client (useful after rebuilding the server).

Let us know if additional Squirrel tooling would be helpful—linting, diagnostics, or hover support can extend this foundation.
