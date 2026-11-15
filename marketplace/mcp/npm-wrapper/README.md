# unflick-mcp

MCP server launcher for the [unflick](https://unflick.app) video player.

unflick ships as a native desktop binary (Windows, macOS, Linux). This package is
a thin launcher so any MCP client can start the server with one command:

```bash
npx -y unflick-mcp
```

It finds the installed `unflick` binary (on `PATH` or in the standard install
locations) and runs `unflick --mcp`, forwarding stdio so the JSON-RPC stream is
untouched. If `unflick` isn't installed yet, interactive shells get install
instructions and MCP clients receive an actionable install prompt when calling a tool.

## Prerequisite

Install unflick:

```bash
# macOS / Linux
curl -fsSL https://unflick.app/install.sh | bash
# Windows (PowerShell)
irm https://unflick.app/install.ps1 | iex
```

## Use in a client

```jsonc
{
  "mcpServers": {
    "unflick": { "command": "npx", "args": ["-y", "unflick-mcp"] }
  }
}
```

If `unflick` is already on your `PATH`, you can also invoke the binary directly:
`"command": "unflick", "args": ["--mcp"]`.

## Tools

114 tools (playback, playlist, subtitles incl. local whisper transcription,
speaker diarization + translation, interactive transcript search/summary/export,
contact sheets & scene detection, audio EQ, media library, clip/GIF, screenshot,
DLNA cast, DVD/Blu-ray discs, user scripts, dev automation) and 3 live resources.
See the [full reference](https://github.com/zhitongblog/unflick/blob/master/marketplace/mcp/README.md).

MIT © xiangdong li
