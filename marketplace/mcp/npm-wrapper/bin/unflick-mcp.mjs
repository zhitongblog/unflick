#!/usr/bin/env node
// unflick-mcp — thin launcher for the unflick MCP server.
//
// unflick ships as a native binary, not as a JS package. This wrapper exists so
// the MCP ecosystem (Smithery, the official registry, npx-based client configs)
// can start the server with a single `npx -y unflick-mcp`. It locates the
// installed `unflick` binary and execs `unflick --mcp`, passing stdio straight
// through so the JSON-RPC stream is untouched.

import { spawn, spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { platform } from "node:os";

const BINARY = platform() === "win32" ? "unflick.exe" : "unflick";

// Common install locations, in addition to whatever is on PATH.
const CANDIDATE_PATHS =
  platform() === "win32"
    ? [
        `${process.env.LOCALAPPDATA ?? ""}\\Programs\\unflick\\unflick.exe`,
        `${process.env.PROGRAMFILES ?? ""}\\unflick\\unflick.exe`,
      ]
    : [
        "/usr/local/bin/unflick",
        "/opt/homebrew/bin/unflick",
        `${process.env.HOME ?? ""}/.local/bin/unflick`,
        "/Applications/unflick.app/Contents/MacOS/unflick",
      ];

function resolveBinary() {
  // 1. Anything on PATH (covers most installs).
  const which = platform() === "win32" ? "where" : "which";
  const found = spawnSync(which, [BINARY], { encoding: "utf8" });
  if (found.status === 0) {
    const first = found.stdout.split(/\r?\n/).find(Boolean);
    if (first) return first.trim();
  }
  // 2. Known install locations.
  for (const p of CANDIDATE_PATHS) {
    if (p && existsSync(p)) return p;
  }
  return null;
}

const bin = resolveBinary();

if (!bin) {
  process.stderr.write(
    [
      "unflick-mcp: could not find the `unflick` binary.",
      "",
      "Install unflick first:",
      "  macOS / Linux : curl -fsSL https://unflick.app/install.sh | bash",
      "  Windows       : irm https://unflick.app/install.ps1 | iex",
      "  or grab an installer at https://github.com/zhitongblog/unflick/releases/latest",
      "",
      "Then make sure `unflick` is on your PATH and retry.",
      "",
    ].join("\n"),
  );
  process.exit(1);
}

const child = spawn(bin, ["--mcp", ...process.argv.slice(2)], {
  stdio: "inherit",
});

child.on("exit", (code, signal) => {
  if (signal) process.kill(process.pid, signal);
  else process.exit(code ?? 0);
});

for (const sig of ["SIGINT", "SIGTERM"]) {
  process.on(sig, () => child.kill(sig));
}
