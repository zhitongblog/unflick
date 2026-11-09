# Send to unflick — Browser Extension

Lightweight Manifest V3 extension for Chrome, Edge, Brave, and Firefox that sends the current tab, link, or `<video>`/`<audio>` element straight to **unflick**.

## How it works

1. **Direct HTTP bridge (`127.0.0.1:19542`)**: When unflick is running, its control socket accepts both line-delimited JSON and HTTP requests (`GET /status`, `POST /open`, `POST /rpc`) with CORS and Private Network Access headers.
2. **Protocol fallback (`unflick://`)**: If unflick is not running yet, the extension opens `unflick://play?url=<encoded>` (or `unflick://queue?url=<encoded>`), which launches unflick via the OS protocol handler.

## Load unpacked (Chrome / Edge / Brave)

1. Open `chrome://extensions` (or `edge://extensions`).
2. Enable **Developer mode**.
3. Click **Load unpacked** and select this `browser-extension/` directory.
