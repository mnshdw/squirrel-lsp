#!/bin/bash
set -e

if [ -z "$1" ]; then
    echo "Usage: $0 <new_version>"
    echo "Example: $0 0.8.0"
    exit 1
fi

NEW_VERSION="$1"
OLD_VERSION=$(grep -E '^version = ' Cargo.toml | head -n1 | sed 's/version = "\(.*\)"/\1/')

if [ -z "$OLD_VERSION" ]; then
    echo "Error: Could not find version in Cargo.toml"
    exit 1
fi

sed -i.bak "s/version = \"$OLD_VERSION\"/version = \"$NEW_VERSION\"/" Cargo.toml
rm Cargo.toml.bak

sed -i.bak "s/\"version\": \"$OLD_VERSION\"/\"version\": \"$NEW_VERSION\"/" vscode-extension/package.json
rm vscode-extension/package.json.bak

cargo check --quiet
git commit -am "Bump version to $NEW_VERSION"

TAG_NAME="v$NEW_VERSION"
git tag "$TAG_NAME"
git push origin main
git push origin "$TAG_NAME"
