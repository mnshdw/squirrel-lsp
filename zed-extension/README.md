# Squirrel Language Extension for Zed

This extension provides Squirrel language support for the [Zed](https://zed.dev) editor, including:

- Syntax highlighting via tree-sitter
- Language Server Protocol (LSP) support via `squirrel-lsp`

## Prerequisites

Install the Squirrel LSP server before using this extension:

```bash
cargo install --git https://github.com/mnshdw/squirrel-lsp
```

Make sure `squirrel-lsp` is available in your PATH.

## Installation

### As a Dev Extension (for development)

1. Open Zed
2. Open the command palette (Cmd+Shift+P)
3. Run "zed: install dev extension"
4. Select the `zed-extension` directory

### From Zed Extensions (once published)

1. Open Zed
2. Go to Extensions (Cmd+Shift+X)
3. Search for "Squirrel"
4. Click Install

## Features

- Syntax highlighting for `.nut` files
- Code diagnostics from squirrel-lsp
- Code formatting
- Go to definition
- Find references
- And more LSP features

## Configuration

You can configure the extension in your Zed settings (`~/.config/zed/settings.json`):

```json
{
  "languages": {
    "Squirrel": {
      "tab_size": 4
    }
  }
}
```

## Development

To build the extension locally:

```bash
cd zed-extension
cargo build --target wasm32-wasip1
```
