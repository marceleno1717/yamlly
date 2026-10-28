#!/usr/bin/env bash
# Usage: ./scripts/bump-version.sh <version>
# Example: ./scripts/bump-version.sh 0.0.4
#
# Updates the version across the Cargo workspace, npm packages, and Cargo.lock.
# Note:
# - crates/yamlly-core/Cargo.toml and crates/yamlly-cli/Cargo.toml inherit from Root Cargo.toml
# - pyproject.toml uses `dynamic = ["version"]` so maturin reads Cargo.toml automatically

set -euo pipefail

if [ $# -lt 1 ]; then
  echo "Usage: $0 <version> (e.g. $0 0.0.4)"
  exit 1
fi

VERSION="${1#v}" # strip leading 'v' if passed (e.g. v0.0.4 -> 0.0.4)
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

echo "Bumping version to ${VERSION}..."

# 1. Root Cargo.toml ([workspace.package] version & [workspace.dependencies] yamlly-core)
sed -i -E "s/^version = \"[^\"]+\"/version = \"${VERSION}\"/" Cargo.toml
sed -i -E "s/^yamlly-core = \{ version = \"[^\"]+\"/yamlly-core = { version = \"${VERSION}\"/" Cargo.toml

# 2. npm-package/package.json & platform packages
node -e "
  const fs = require('fs');
  const mainPath = 'npm-package/package.json';
  const mainJson = JSON.parse(fs.readFileSync(mainPath, 'utf8'));
  mainJson.version = '${VERSION}';
  for (const k of Object.keys(mainJson.optionalDependencies || {})) {
    mainJson.optionalDependencies[k] = '${VERSION}';
  }
  fs.writeFileSync(mainPath, JSON.stringify(mainJson, null, 2) + '\n');

  for (const dir of fs.readdirSync('npm-package/npm')) {
    const p = \`npm-package/npm/\${dir}/package.json\`;
    if (fs.existsSync(p)) {
      const j = JSON.parse(fs.readFileSync(p, 'utf8'));
      j.version = '${VERSION}';
      fs.writeFileSync(p, JSON.stringify(j, null, 2) + '\n');
    }
  }
"

# 3. Update Cargo.lock
cargo check --workspace --quiet

echo "✓ All manifests updated to v${VERSION}!"
echo ""
echo "Next steps when ready to release:"
echo "  git add -A && git commit -m \"chore: release v${VERSION}\""
echo "  git tag v${VERSION} && git push origin main --tags"
