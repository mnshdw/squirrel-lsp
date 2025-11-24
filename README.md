## Squirrel Language Server (squirrel-lsp)

Rust-based Language Server Protocol (LSP) implementation for the Squirrel language (`.nut`) with VS Code and Zed editor extensions.

The VS Code extension bundles prebuilt `squirrel-lsp` binaries for major platforms, so most users don’t need to download or configure anything extra.

---

## Install in VS Code

Install the extension from the Marketplace:

- Open VS Code Extensions panel (`Ctrl+Shift+X` / `Cmd+Shift+X`)
- Search for "Squirrel"
- Click "Install"

Or via command line:

```bash
code --install-extension mnshdw.squirrel-lsp-vscode
```

That’s it. The extension will automatically start the bundled server for your platform.

Advanced: if you prefer to use a custom server binary, set the absolute path in Settings → "Squirrel LSP: Server Path" (`squirrelLsp.serverPath`). If left empty, the extension uses the bundled binary, or falls back to PATH.

Open any `.nut` file to activate the extension. Check Output → "Squirrel LSP (client)" for logs like "Language client is ready". Use "Format Document" to format or add `
  "[squirrel]": {
    "editor.formatOnSave": true
  }` to your settings for auto-format on save.

---

## Install in Zed

1. Open Zed
2. Go to Extensions (`Cmd+Shift+X`)
3. Search for "Squirrel"
4. Click Install

The LSP binary will be downloaded automatically from GitHub releases.

For manual installation or custom builds, see the [Zed extension README](./zed-extension/README.md).

---

## Supported platforms (bundled)

The extension bundles binaries for:

- Windows: x64, ARM64
- macOS: Intel (x64), Apple Silicon (ARM64)
- Linux: x64, ARM64

If your platform isn’t covered, the extension will fall back to a `squirrel-lsp` found on PATH or a custom path via `squirrelLsp.serverPath`.

## Build locally with Cargo (optional)

Prerequisites: Rust toolchain (rustup)

```bash
cargo build --release
```

The binary will be at:

- macOS/Linux: `target/release/squirrel-lsp`
- Windows: `target\release\squirrel-lsp.exe`

---

## Configuration in VS Code

Setting: "Squirrel LSP: Server Path" (`squirrelLsp.serverPath`)

- Absolute path to a custom `squirrel-lsp` executable.
- Leave empty to use the bundled binary (default). The extension also falls back to PATH or your workspace’s Cargo target dir while developing.

Command: "Squirrel LSP: Restart Server"

- Manually restarts the language client after you update the server binary.

---

## Developing

### Prerequisites

- Rust toolchain (rustup) – to build the LSP server
- Node.js 18+ – to build the VS Code extension

### Build the server

```bash
cargo build --release
```

### Build/package the VS Code extension

The CI builds per-platform binaries and packs them into the extension automatically on tags. For local packaging, either use the prebuilt artifacts or copy your locally built binary into the matching folder before packaging:

```
vscode-extension/bin/
  darwin-x64/squirrel-lsp
  darwin-arm64/squirrel-lsp
  linux-x64/squirrel-lsp
  linux-arm64/squirrel-lsp
  win32-x64/squirrel-lsp.exe
  win32-arm64/squirrel-lsp.exe
```

Then:

```bash
npm --prefix ./vscode-extension install
npm --prefix ./vscode-extension run compile
npm --prefix ./vscode-extension run package
```

### Install locally

```bash
./install-vscode.sh
```

This script builds both the server and extension, then installs the extension in VS Code.

---

## License

MIT
