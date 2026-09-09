# unflick-mcp

MCP server launcher for the [unflick](https://unflick.app) video player.

unflick ships as a native binary. This package is a tiny launcher so any MCP
client can start the server with one command:

```bash
npx -y unflick-mcp
```

It finds the installed `unflick` binary (on PATH or in the usual install
locations) and runs `unflick --mcp`, forwarding stdio so the JSON-RPC stream is
untouched. If `unflick` isn't installed it prints install instructions and exits.

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

If `unflick` is on your PATH you can also skip the wrapper entirely and use
`"command": "unflick", "args": ["--mcp"]`.

## Tools

94 tools (playback, playlist, subtitles incl. local whisper transcription +
translation, audio tracks, media library, clip/GIF, screenshot, video filters,
settings) and 3 live resources. See the
[full reference](https://github.com/zhitongblog/unflick/blob/main/marketplace/mcp/README.md).

MIT © xiangdong li
