// Build the magneto daemon and stage it as the Tauri sidecar.
//
// Tauri's `externalBin` expects `src-tauri/binaries/magneto-daemon-<target-triple>`.
// At runtime the app finds the daemon as a sibling of its own executable, which
// holds in a bundle (Tauri copies the sidecar there) and in dev (the workspace
// build drops both binaries in the same target/<profile>/ dir).
//
// Usage: node scripts/stage-daemon.mjs [--release]

import { execFileSync } from "node:child_process";
import { copyFileSync, mkdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const release = process.argv.includes("--release");
const profile = release ? "release" : "debug";

const appDir = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const workspaceRoot = resolve(appDir, "..");

const rustcInfo = execFileSync("rustc", ["-vV"], { encoding: "utf8" });
const triple = rustcInfo.match(/^host:\s*(.+)$/m)?.[1].trim();
if (!triple) throw new Error("could not determine host target triple from rustc -vV");
const isWindows = triple.includes("windows");
const exeName = isWindows ? "magneto-daemon.exe" : "magneto-daemon";

const buildArgs = ["build", "-p", "magneto-daemon"];
if (release) buildArgs.push("--release");
execFileSync("cargo", buildArgs, { stdio: "inherit", cwd: workspaceRoot });

const src = join(workspaceRoot, "target", profile, exeName);
const destDir = join(appDir, "src-tauri", "binaries");
mkdirSync(destDir, { recursive: true });
const dest = join(destDir, isWindows ? `magneto-daemon-${triple}.exe` : `magneto-daemon-${triple}`);
copyFileSync(src, dest);
console.log(`staged daemon: ${src} -> ${dest}`);
