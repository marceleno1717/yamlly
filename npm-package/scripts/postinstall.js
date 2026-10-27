#!/usr/bin/env node
/**
 * postinstall.js — Runs after `npm install yamlly`.
 * Verifies if the optional platform package (@yamlly/<target>-bin) is present;
 * if not, attempts to download the matching standalone binary from GitHub Releases.
 */
"use strict";

const fs = require("fs");
const os = require("os");
const path = require("path");
const https = require("https");

const PLATFORM_MAP = {
  "linux-x64": { pkg: "@yamlly/linux-x64-bin", asset: "yamlly-linux-x64" },
  "darwin-x64": { pkg: "@yamlly/darwin-x64-bin", asset: "yamlly-darwin-x64" },
  "darwin-arm64": { pkg: "@yamlly/darwin-arm64-bin", asset: "yamlly-darwin-arm64" },
  "win32-x64": { pkg: "@yamlly/win32-x64-bin", asset: "yamlly-win32-x64.exe" },
};

const key = `${process.platform}-${os.arch()}`;
const target = PLATFORM_MAP[key];

if (!target) {
  console.warn(`\nyamlly: Pre-built binary not available for ${key}.`);
  console.warn("Install from source with Cargo: cargo install yamlly\n");
  process.exit(0);
}

// Check if optional platform package already provides the binary
try {
  const pkgDir = path.dirname(require.resolve(`${target.pkg}/package.json`));
  const binName = process.platform === "win32" ? "yamlly.exe" : "yamlly";
  if (fs.existsSync(path.join(pkgDir, "bin", binName))) {
    process.exit(0);
  }
} catch {
  // Optional package not installed; attempt GitHub Release fallback download
}

const pkgJson = JSON.parse(
  fs.readFileSync(path.join(__dirname, "..", "package.json"), "utf8")
);
const version = pkgJson.version;
const destName = process.platform === "win32" ? "yamlly-bin.exe" : "yamlly-bin";
const destPath = path.join(__dirname, "..", "bin", destName);
const url = `https://github.com/marceleno1717/yamlly/releases/download/v${version}/${target.asset}`;

function download(downloadUrl, dest, redirects = 0) {
  if (redirects > 5) return;
  https
    .get(downloadUrl, (res) => {
      if (res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
        res.resume();
        return download(res.headers.location, dest, redirects + 1);
      }
      if (res.statusCode !== 200) {
        res.resume();
        return;
      }
      const file = fs.createWriteStream(dest, { mode: 0o755 });
      res.pipe(file);
      file.on("finish", () => file.close());
    })
    .on("error", () => {
      // Non-fatal during postinstall
    });
}

download(url, destPath);
