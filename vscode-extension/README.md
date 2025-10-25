# Squirrel Language Server

Language Server Protocol (LSP) support for Squirrel `.nut` files in VS Code. Provides code formatting capabilities powered by the Rust-based `squirrel-lsp` server.

## Features

- **Document Formatting** – Format Squirrel code with proper indentation and spacing
- **Format on Save** – Optionally auto-format files when saving

## Installation

### 1. Install the VS Code Extension

Download the VSIX package (`squirrel-lsp-vscode-<version>.vsix`) from the [GitHub Releases](https://github.com/mnshdw/squirrel-lsp/releases) page and install it:

- In VS Code: Extensions panel → `...` → "Install from VSIX..."
- Or via command line:
  ```bash
  code --install-extension /path/to/squirrel-lsp-vscode-<version>.vsix
  ```

### 2. Install the Language Server Binary

Download the prebuilt `squirrel-lsp` binary for your platform from [GitHub Releases](https://github.com/mnshdw/squirrel-lsp/releases):

- `squirrel-lsp-windows-x86_64.exe` (Windows)
- `squirrel-lsp-macos-aarch64` (macOS Apple Silicon)
- `squirrel-lsp-macos-x86_64` (macOS Intel)
- `squirrel-lsp-linux-x86_64` (Linux)

On Unix-like systems, make the binary executable:
```bash
chmod +x /path/to/squirrel-lsp
```

### 3. Configure the Extension

Set the binary path in VS Code settings:

- Open Settings → search "Squirrel LSP: Server Path"
- Enter the absolute path to your `squirrel-lsp` binary
- Or place the binary in your `PATH` and leave the setting empty

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

## Troubleshooting

- **"Could not locate the squirrel-lsp executable"**
  - Set the "Server Path" setting to the binary's absolute path, or ensure it's in your `PATH`
- **Permission denied (Unix/macOS/Linux)**
  - Run: `chmod +x /path/to/squirrel-lsp`
- **Extension not activating**
  - Ensure the file extension is `.nut` and VS Code is version 1.90.0 or newer

## Building from Source

If you want to build the language server yourself instead of using prebuilt binaries, see the [main repository README](https://github.com/mnshdw/squirrel-lsp#developing) for build instructions.

## License

MIT
