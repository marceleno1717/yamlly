#!/bin/bash
# Run this locally after 'npm login' to publish the initial empty packages
# so you can configure OIDC on npmjs.com.

set -e

cd "$(dirname "$0")"

echo "Publishing initial @yamlly-cli/linux-x64-bin..."
cd npm/yamlly-linux-x64-bin
npm version 0.0.1 --no-git-tag-version --allow-same-version
rm -rf bin && mkdir -p bin && touch bin/yamlly && chmod +x bin/yamlly
npm publish --access public
cd ../..

echo "Publishing initial @yamlly-cli/darwin-arm64-bin..."
cd npm/yamlly-darwin-arm64-bin
npm version 0.0.1 --no-git-tag-version --allow-same-version
rm -rf bin && mkdir -p bin && touch bin/yamlly && chmod +x bin/yamlly
npm publish --access public
cd ../..

echo "Publishing initial @yamlly-cli/win32-x64-bin..."
cd npm/yamlly-win32-x64-bin
npm version 0.0.1 --no-git-tag-version --allow-same-version
rm -rf bin && mkdir -p bin && touch bin/yamlly.exe
npm publish --access public
cd ../..

echo "Publishing initial main package yamlly-cli..."
npm version 0.0.1 --no-git-tag-version --allow-same-version
chmod +x bin/yamlly.js
npm publish --access public

echo "Done! You can now configure OIDC on npmjs.com for yamlly-cli and the @yamlly-cli/*-bin packages."
