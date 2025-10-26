# Squirrel Language Server

Language Server Protocol (LSP) support for Squirrel `.nut` files in VS Code. Formatting powered by the Rust-based `squirrel-lsp` server.

## Features

- **Document Formatting** – Format Squirrel code with proper indentation and spacing
- **Format on Save** – Optionally auto-format files when saving

## Installation

Install from the VS Code Marketplace:

- Open VS Code Extensions panel (`Ctrl+Shift+X` / `Cmd+Shift+X`)
- Search for "Squirrel Language Server"
- Click "Install"

Or via command line:

```bash
code --install-extension mnshdw.squirrel-lsp-vscode
```

No extra setup is required, the extension bundles prebuilt server binaries for the major platforms and will start the right one automatically.

Advanced: to use a custom server, set Settings → "Squirrel LSP: Server Path" (`squirrelLsp.serverPath`). If left empty, the bundled binary is used; the extension also falls back to PATH.

## Usage

Open any `.nut` file to activate the extension. Use **Format Document** (`Shift+Alt+F` / `Shift+Option+F`) or enable format-on-save:

```json
{
  "[squirrel]": {
    "editor.formatOnSave": true
  }
}
```

## Commands

- **Squirrel LSP: Restart Server** – Manually restart the language server (useful after updating the binary)

## Supported platforms (bundled)

- Windows: x64, ARM64
- macOS: Intel (x64), Apple Silicon (ARM64)
- Linux: x64, ARM64

## Troubleshooting

- "Could not locate the squirrel-lsp executable"
  - On unsupported platforms, set the "Server Path" to your custom binary, or place `squirrel-lsp` on PATH
- Permission denied (Unix/macOS/Linux)
  - If you’re using a custom binary, ensure it’s executable: `chmod +x /path/to/squirrel-lsp`
- Extension not activating
  - Ensure the file extension is `.nut` and VS Code is version 1.90.0 or newer

## Building from Source

If you prefer to build the server yourself, see the [main repository README](https://github.com/mnshdw/squirrel-lsp#developing).

## License

MIT
