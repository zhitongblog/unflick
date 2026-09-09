# unflick — MCP & Skills distribution kit

Everything needed to publish the **unflick MCP server** and its **Claude Skills**
to the major marketplaces — and a reference any other app can copy to do the same.

> unflick is a free, ad-free, cross-platform video player for **humans and AI**.
> The same playback core is exposed three ways: GUI, CLI, and an MCP server.
> AI agents (Claude, Cursor, Codex — anything that speaks MCP) drive it natively
> through **94 tools** and **3 live resources**.

- Website: https://unflick.app
- Repository: https://github.com/zhitongblog/unflick
- License: MIT
- Version: 0.13.1

---

## What's in here

```
marketplace/
├── README.md              ← you are here
├── SUBMISSION.md          ← per-marketplace, step-by-step submit checklist
├── mcp/                   ← MCP server distribution
│   ├── README.md          ← full tool + resource reference
│   ├── server.json        ← official MCP Registry manifest
│   ├── smithery.yaml      ← Smithery config
│   ├── glama.json         ← Glama metadata
│   ├── manifest.json      ← generic directory manifest (mcp.so / PulseMCP fields)
│   ├── client-configs/    ← copy-paste configs for Claude Desktop / Cursor / Codex / VS Code
│   └── npm-wrapper/       ← publishable `unflick-mcp` npm package (the npx entrypoint)
├── skills/                ← seven Claude Skills wrapping common unflick workflows
│   ├── README.md
│   ├── unflick-play/
│   ├── unflick-clip-gif/
│   ├── unflick-subtitles/
│   ├── unflick-navigate/
│   ├── unflick-library/
│   ├── unflick-screenshot/
│   └── unflick-sponsorblock/
├── .claude-plugin/        ← plugin.json — makes marketplace/ an installable Claude Code plugin
└── assets/                ← screenshot / logo checklist for the listing pages
```

> The Claude Code **plugin marketplace** manifest lives at the repo root:
> `../.claude-plugin/marketplace.json`. It points at this `marketplace/` folder
> as the `unflick` plugin, which bundles the seven skills *and* the MCP server.
> Install with `/plugin marketplace add zhitongblog/unflick` then `/plugin install unflick`.

## The distribution model (how an MCP binary reaches every marketplace)

unflick ships as a native binary (installed via the installer or `curl … | bash`),
not as an npm/pypi package. Most marketplaces, however, expect an `npx`/`pip`
runnable identifier. We bridge that with a **thin npm wrapper** (`mcp/npm-wrapper/`):

```
npx -y unflick-mcp   →   finds the installed `unflick` binary   →   exec `unflick --mcp`
```

The wrapper is what registries reference; the heavy native binary stays out of npm.
This is the standard pattern for native-binary MCP servers and is the part other
apps will most want to copy.

```
AI agent  ──stdio──>  npx unflick-mcp  ──spawn──>  unflick --mcp  ──>  unflick daemon
                                                                         (libmpv core)
```

## Quick start for users (already documented in each listing)

```jsonc
// Claude Desktop / Cursor — claude_desktop_config.json
{
  "mcpServers": {
    "unflick": { "command": "unflick", "args": ["--mcp"] }
  }
}
```

If `unflick` is not on PATH, use the wrapper instead: `"command": "npx", "args": ["-y", "unflick-mcp"]`.

## For other app authors

This kit is deliberately generic. To adapt it:

1. Replace the binary name, tool list, and metadata in `mcp/server.json`,
   `mcp/manifest.json`, `mcp/glama.json`, and `mcp/smithery.yaml`.
2. Re-point `mcp/npm-wrapper/bin/*` at your binary's `--mcp` (or equivalent) flag.
3. Rewrite the seven `skills/*/SKILL.md` to your domain workflows.
4. Follow `SUBMISSION.md` — the steps are the same for any MCP server.
