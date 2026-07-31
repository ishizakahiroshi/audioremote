#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";

const require = createRequire(import.meta.url);
const platformKey = `${process.platform}-${process.arch}`;
const packages = {
  "win32-x64": ["@ishizakahiroshi/audioremote-win32-x64", "audioremote.exe"],
};

const selected = packages[platformKey];
if (!selected) {
  process.stderr.write(
    `audioremote: unsupported platform ${platformKey} (Windows 11 x64 only)\n`,
  );
  process.exit(2);
}

const [packageName, binaryName] = selected;
let binary = process.env.AUDIOREMOTE_BINARY_PATH;
if (!binary) {
  try {
    const manifest = require.resolve(`${packageName}/package.json`);
    binary = join(dirname(manifest), "bin", binaryName);
  } catch {
    process.stderr.write(
      `audioremote: native package ${packageName} is missing. Reinstall audioremote without --no-optional.\n`,
    );
    process.exit(2);
  }
}

const result = spawnSync(binary, process.argv.slice(2), {
  stdio: "inherit",
  windowsHide: true,
});
if (result.error) {
  process.stderr.write(`audioremote: failed to start native binary: ${result.error.message}\n`);
  process.exit(2);
}
if (result.signal) {
  process.kill(process.pid, result.signal);
} else {
  process.exit(result.status ?? 2);
}
