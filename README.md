## Squirrel Language Server (squirrel-lsp)

Rust-based Language Server Protocol (LSP) implementation for the Squirrel language (`.nut`) with a VS Code client extension.

This README explains how to install and use the server in VS Code, and how to obtain binaries (prebuilt or self-built) on Windows, macOS, and Linux.

---

## Install in VS Code

You need two pieces:

1. VS Code extension (client)

Install directly from the VS Code Marketplace:
- Open VS Code Extensions panel (`Ctrl+Shift+X` / `Cmd+Shift+X`)
- Search for "Squirrel Language Server"
- Click "Install"

Or via command line:
```bash
code --install-extension mnshdw.squirrel-lsp-vscode
```

2. LSP server binary

- Provide the `squirrel-lsp` executable on your system and tell the extension where to find it:
  - VS Code Settings → search "Squirrel LSP: Server Path" → set absolute path to the binary
  - Or place `squirrel-lsp` on your PATH so the extension can find it automatically

Open any `.nut` file to activate the extension. Check Output → "Squirrel LSP (client)" for logs like "Language client is ready". Use "Format Document" to format or add `
  "[squirrel]": {
    "editor.formatOnSave": true
  }` to your settings for auto-format on save.

---

## Get the server binary

Choose one of the following:

### A) Download prebuilt binaries (recommended)

- Download prebuilt binaries for your platform (Windows, macOS [Intel & Apple Silicon], Linux x86_64) from the [GitHub Releases](https://github.com/mnshdw/squirrel-lsp/releases) page.

Binaries are named by platform, for example:

- `squirrel-lsp-windows-x86_64.exe`
- `squirrel-lsp-macos-aarch64`
- `squirrel-lsp-macos-x86_64`
- `squirrel-lsp-linux-x86_64`

On Unix-like systems, make the binary executable if needed:

```bash
chmod +x /path/to/squirrel-lsp
```

### B) Build locally with Cargo

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

- Absolute path to the `squirrel-lsp` executable.
- Leave empty to let the extension search your PATH (or workspace build directories if developing).

Command: "Squirrel LSP: Restart Server"

- Manually restarts the language client after you update the server binary.

---

## Troubleshooting

- "Could not locate the squirrel-lsp executable"
  - Set the Server Path setting to the binary, or place it on PATH.
- Permission denied (Unix)
  - Ensure the binary is executable: `chmod +x /path/to/squirrel-lsp`.
- Extension not activating
  - Ensure the file extension is `.nut` and VS Code version is ≥ 1.90.

---

## Developing

### Prerequisites

- **Rust toolchain** (rustup) – to build the LSP server
- **Node.js 18+** – to build the VS Code extension

### Build the server

```bash
cargo build --release
```

The binary will be at:
- macOS/Linux: `target/release/squirrel-lsp`
- Windows: `target\release\squirrel-lsp.exe`

### Build/package VS Code extension

```bash
npm --prefix ./vscode-extension install
npm --prefix ./vscode-extension run compile
npm --prefix ./vscode-extension run package
```

### Install locally (helper script)

```bash
./install.sh
```

This script builds both the server and extension, then installs the extension in VS Code.

---

## License

MIT
