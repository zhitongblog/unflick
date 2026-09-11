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
- Version: 0.14.0

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
not as an npm/pypi package. Every config here therefore launches the binary:

```
"command": "unflick", "args": ["--mcp"]
```

Some marketplaces prefer an `npx`/`pip` runnable identifier, which a **thin npm
wrapper** (`mcp/npm-wrapper/`) provides — it finds the installed binary and execs
`unflick --mcp`, so the heavy native artifact stays out of npm. It is written,
packaged and tested, but **not published yet** (`SUBMISSION.md` §1), so nothing
in this kit depends on it: `npx -y unflick-mcp` will not work until it is.

```
npx -y unflick-mcp   →   finds the installed `unflick` binary   →   exec `unflick --mcp`
```

That wrapper is the part other apps with a native binary will most want to copy.

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

The installers put `unflick` on PATH. Once the wrapper is published, `"command":
"npx", "args": ["-y", "unflick-mcp"]` becomes an alternative that does not need it.

## For other app authors

This kit is deliberately generic. To adapt it:

1. Replace the binary name, tool list, and metadata in `mcp/server.json`,
   `mcp/manifest.json`, `mcp/glama.json`, and `mcp/smithery.yaml`.
2. Re-point `mcp/npm-wrapper/bin/*` at your binary's `--mcp` (or equivalent) flag.
3. Rewrite the seven `skills/*/SKILL.md` to your domain workflows.
4. Follow `SUBMISSION.md` — the steps are the same for any MCP server.
