#!/bin/bash
set -e

echo "Building LSP server..."
cargo build --release
cargo install --path .

echo "Building VSCode extension..."
npm --prefix ./vscode-extension run compile
npm --prefix ./vscode-extension run package

VERSION=$(node -p "require('./vscode-extension/package.json').version")
echo "Installing extension version $VERSION..."
code --uninstall-extension mnshdw.squirrel-lsp-vscode 2>/dev/null || true
code --install-extension "./vscode-extension/squirrel-lsp-vscode-${VERSION}.vsix"

echo "Done! Reload VSCode window to activate the updated extension."
