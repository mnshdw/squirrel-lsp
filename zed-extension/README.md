# Squirrel Language Extension for Zed

This extension provides Squirrel language support for the [Zed](https://zed.dev) editor, including:

- Syntax highlighting via tree-sitter
- Language Server Protocol (LSP) support via `squirrel-lsp`
- Code formatting, diagnostics, go to definition, and more

## Installation

### From Zed Extensions (recommended)

1. Open Zed
2. Go to Extensions (Cmd+Shift+X)
3. Search for "Squirrel"
4. Click Install

The LSP binary will be downloaded automatically from GitHub releases.

### Manual LSP Installation

If you prefer to install the LSP manually or need a custom build:

```bash
cargo install --git https://github.com/mnshdw/squirrel-lsp
```

The extension will use a manually installed `squirrel-lsp` from PATH if available.

## Configuration

You can configure the extension in your Zed settings (`~/.config/zed/settings.json`):

```json
{
  "languages": {
    "Squirrel": {
      "tab_size": 4
    }
  },
  "lsp": {
    "squirrel-lsp": {
      "binary": {
        "path": "/custom/path/to/squirrel-lsp"
      }
    }
  }
}
```

## Development

To build the extension locally:

```bash
cd zed-extension
cargo build --release --target wasm32-wasip1
```

To test as a dev extension:
1. Open Zed
2. Command palette → "zed: install dev extension"
3. Select the `zed-extension` directory
