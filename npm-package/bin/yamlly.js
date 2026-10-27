#!/usr/bin/env node
/**
 * yamlly launcher — detects the current platform, resolves the pre-built
 * native Rust binary from the optional platform package (or postinstall fallback),
 * and spawns it with inherited stdio.
 */
"use strict";

const { spawnSync } = require("child_process");
const path = require("path");
const fs = require("fs");
const os = require("os");

const PLATFORM_MAP = {
  "linux-x64": "@yamlly/linux-x64-bin",
  "darwin-x64": "@yamlly/darwin-x64-bin",
  "darwin-arm64": "@yamlly/darwin-arm64-bin",
  "win32-x64": "@yamlly/win32-x64-bin",
};

const key = `${process.platform}-${os.arch()}`;
const pkg = PLATFORM_MAP[key];

if (!pkg) {
  console.error(`yamlly: unsupported platform: ${key}`);
  console.error("Supported platforms: linux-x64, darwin-x64, darwin-arm64, win32-x64");
  process.exit(1);
}

const binName = process.platform === "win32" ? "yamlly.exe" : "yamlly";
let binPath = null;

// 1. Try optional platform package (@yamlly/<target>-bin)
try {
  const pkgDir = path.dirname(require.resolve(`${pkg}/package.json`));
  const candidate = path.join(pkgDir, "bin", binName);
  if (fs.existsSync(candidate)) {
    binPath = candidate;
  }
} catch {
  // Fall back to local binary downloaded by postinstall.js
}

// 2. Fallback to binary downloaded into ./bin/ during postinstall
if (!binPath) {
  const fallbackName = process.platform === "win32" ? "yamlly-bin.exe" : "yamlly-bin";
  const fallback = path.join(__dirname, fallbackName);
  if (fs.existsSync(fallback)) {
    binPath = fallback;
  }
}

if (!binPath || !fs.existsSync(binPath)) {
  console.error(`yamlly: native binary for ${key} was not found.`);
  console.error("Try reinstalling: npm install -g yamlly");
  process.exit(1);
}

if (process.platform !== "win32") {
  try {
    fs.chmodSync(binPath, 0o755);
  } catch {
    // Ignore chmod errors on read-only mounts
  }
}

const result = spawnSync(binPath, process.argv.slice(2), {
  stdio: "inherit",
  env: process.env,
});

process.exit(result.status ?? 1);
