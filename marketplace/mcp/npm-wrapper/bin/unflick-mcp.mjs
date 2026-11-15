#!/usr/bin/env node
// unflick-mcp — thin launcher for the unflick MCP server.
//
// unflick ships as a native binary, not as a JS package. This wrapper exists so
// the MCP ecosystem (Smithery, Glama, the official registry, npx-based client
// configs) can start the server with a single `npx -y unflick-mcp`. It locates
// the installed `unflick` binary and execs `unflick --mcp`, passing stdio
// straight through so the JSON-RPC stream is untouched.
//
// When `unflick` is not installed yet:
// - Interactive terminals (TTY) get clear installation instructions and exit 1.
// - Non-TTY MCP stdio clients (or registry health checks) get a clean JSON-RPC
//   surface (`initialize`, `tools/list`, `resources/list`) from `surface.json`
//   and an actionable install message on `tools/call`.

import { spawn, spawnSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { platform } from "node:os";
import { createInterface } from "node:readline";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const BINARY = platform() === "win32" ? "unflick.exe" : "unflick";

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
  if (process.env.UNFLICK_MCP_FORCE_STUB === "1") return null;
  const which = platform() === "win32" ? "where" : "which";
  const found = spawnSync(which, [BINARY], { encoding: "utf8" });
  if (found.status === 0) {
    const first = found.stdout.split(/\r?\n/).find(Boolean);
    if (first) return first.trim();
  }
  for (const p of CANDIDATE_PATHS) {
    if (p && existsSync(p)) return p;
  }
  return null;
}

const INSTALL_HINT = [
  "unflick desktop binary was not found on this machine.",
  "",
  "Install unflick first:",
  "  macOS / Linux : curl -fsSL https://unflick.app/install.sh | bash",
  "  Windows       : irm https://unflick.app/install.ps1 | iex",
  "  or download   : https://github.com/zhitongblog/unflick/releases/latest",
].join("\n");

const bin = resolveBinary();

if (bin) {
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
} else if (process.stdin.isTTY) {
  process.stderr.write(`${INSTALL_HINT}\n`);
  process.exit(1);
} else {
  const here = dirname(fileURLToPath(import.meta.url));
  const surface = JSON.parse(
    readFileSync(join(here, "..", "surface.json"), "utf8"),
  );

  const reply = (id, result) => {
    process.stdout.write(
      `${JSON.stringify({ jsonrpc: "2.0", id, result })}\n`,
    );
  };
  const replyError = (id, code, message) => {
    process.stdout.write(
      `${JSON.stringify({ jsonrpc: "2.0", id, error: { code, message } })}\n`,
    );
  };

  const rl = createInterface({ input: process.stdin, crlfDelay: Infinity });
  rl.on("line", (line) => {
    const trimmed = line.trim();
    if (!trimmed) return;
    let msg;
    try {
      msg = JSON.parse(trimmed);
    } catch {
      return;
    }
    if (msg.id === undefined || msg.id === null) return;

    switch (msg.method) {
      case "initialize":
        reply(msg.id, {
          protocolVersion: msg.params?.protocolVersion || "2024-11-05",
          capabilities: { tools: {}, resources: {} },
          serverInfo: surface.serverInfo,
        });
        break;
      case "ping":
        reply(msg.id, {});
        break;
      case "tools/list":
        reply(msg.id, { tools: surface.tools });
        break;
      case "resources/list":
        reply(msg.id, { resources: surface.resources });
        break;
      case "prompts/list":
        reply(msg.id, { prompts: [] });
        break;
      case "tools/call":
        reply(msg.id, {
          content: [{ type: "text", text: INSTALL_HINT }],
          isError: true,
        });
        break;
      case "resources/read":
        replyError(msg.id, -32002, INSTALL_HINT);
        break;
      default:
        replyError(msg.id, -32601, `Method not found: ${msg.method}`);
        break;
    }
  });
}
