# Submission checklist

Step-by-step to list the unflick MCP server and skills on every major
marketplace. **The steps that need your account/credentials are flagged
`🔑 YOU`** — I can't log in or publish as you. Everything else (the manifests,
wrapper, skills, configs) is already generated in this folder.

Order no longer matters much: step **1** (npm) is optional — the rest reference
the installed binary, not the npm package.

---

## 0. 🔑 YOU — Prerequisites (once)

- [ ] Push the repo (incl. `marketplace/` and `.claude-plugin/`) to
      `github.com/zhitongblog/unflick`, default branch `master`, **public**.
- [ ] Confirm `logo.png` is reachable at `https://unflick.app/logo.png`
      (see `assets/ASSETS.md`).
- [ ] Have an [npmjs.com](https://www.npmjs.com) account and run `npm login`.
- [ ] (Optional, for the official registry) be able to auth the GitHub namespace
      `io.github.zhitongblog` — you own the account, so this is just an OAuth click.

---

## 1. npm — publish the `unflick-mcp` wrapper  🔑 YOU — *optional, currently blocked*

Nothing below depends on this any more: every manifest and client config
launches the installed binary (`unflick --mcp`) instead. Publishing the wrapper
only adds an `npx` path for users who would rather not have `unflick` on PATH.

**Why it is blocked:** `registry.npmjs.org` is reachable from here, but
`www.npmjs.com` answers Cloudflare's "Just a moment…" challenge on every exit
node tried, so neither signup nor token creation completes. Either register from
a different network, or try the registry-only flow (`npm adduser --auth-type=legacy`).

After publishing, put the package back into `mcp/server.json` — it is valid
without one, and the registry rejects an entry pointing at a package that does
not exist:

```json
"packages": [
  {
    "registry_type": "npm",
    "registry_base_url": "https://registry.npmjs.org",
    "identifier": "unflick-mcp",
    "version": "<same as the app>",
    "transport": { "type": "stdio" }
  }
]
```

```bash
cd marketplace/mcp/npm-wrapper
npm publish --access public
# verify:
npx -y unflick-mcp   # should print install hint if unflick isn't installed, else start the server
```

- [ ] Published `unflick-mcp@0.13.1`
- [ ] `npx -y unflick-mcp` behaves (starts server when `unflick` is installed)

> Keep the wrapper version in lockstep with the app — `scripts/release.sh` does
> it: it bumps `package.json`, `server.json`, `manifest.json`, `plugin.json` and
> the root `marketplace.json` along with the app's three version files.

---

## 2. Official MCP Registry (registry.modelcontextprotocol.io)

Manifest: [`mcp/server.json`](./mcp/server.json) (name `io.github.zhitongblog/unflick`).

```bash
# install the publisher CLI (Go or Homebrew)
brew install mcp-publisher        # or: go install github.com/modelcontextprotocol/registry/cmd/mcp-publisher@latest

cd marketplace/mcp
mcp-publisher validate server.json     # lint against the schema
```

- [ ] `validate` passes
- [ ] 🔑 YOU — `mcp-publisher login github` (OAuth in browser)
- [ ] 🔑 YOU — `mcp-publisher publish` from `marketplace/mcp/`
- [ ] Listing live at `registry.modelcontextprotocol.io` (search "unflick")

> Namespace note: `io.github.zhitongblog/*` is authorized by your GitHub login.
> To use `com.unflick/unflick` instead, add the DNS TXT record the CLI prints and
> re-run — only do this if you want the vanity namespace.

---

## 3. Smithery (smithery.ai)

Config: [`mcp/smithery.yaml`](./mcp/smithery.yaml). Smithery indexes from GitHub.

- [ ] 🔑 YOU — sign in at https://smithery.ai with GitHub
- [ ] 🔑 YOU — **Add Server** → pick `zhitongblog/unflick`
- [ ] Point it at `marketplace/mcp/smithery.yaml` (Settings → config path) **or**
      copy that file to the repo root if Smithery only checks root
- [ ] Confirm the deploy/scan succeeds and the tool list renders

> Because unflick is a local binary, this lists it as a **local stdio** server
> (`npx -y unflick-mcp`); it is not hosted/remote-runnable on Smithery.

---

## 4. mcp.so

Crawls GitHub and accepts manual submissions. Fields are in
[`mcp/manifest.json`](./mcp/manifest.json).

- [ ] 🔑 YOU — go to https://mcp.so/submit
- [ ] Submit repo URL `https://github.com/zhitongblog/unflick`
- [ ] Paste name `unflick`, the description, homepage, and the config:
      `{ "command": "npx", "args": ["-y", "unflick-mcp"] }`
- [ ] Add the icon URL and a screenshot from `assets/`

---

## 5. Glama (glama.ai/mcp)

Auto-indexes public GitHub repos that contain an MCP server; metadata comes from
[`mcp/glama.json`](./mcp/glama.json).

- [ ] Ensure `glama.json` is committed (root or `marketplace/mcp/` — Glama finds it)
- [ ] Add GitHub repo **topics**: `mcp`, `model-context-protocol`, `video`
- [ ] 🔑 YOU — (optional) claim the listing at https://glama.ai after it appears
- [ ] Verify the auto-generated page; correct metadata via `glama.json` if needed

---

## 6. PulseMCP (pulsemcp.com)

- [ ] 🔑 YOU — submit at https://www.pulsemcp.com/submit
- [ ] Repo URL + the same description/config as mcp.so
- [ ] Category: Media / Video

---

## 7. awesome-mcp-servers (GitHub PR)  🔑 YOU

- [ ] Fork https://github.com/punkpeye/awesome-mcp-servers
- [ ] Under a relevant category (e.g. *🎬 Media / Video*) add:

  ```markdown
  - [unflick](https://github.com/zhitongblog/unflick) 🎬 - Video player for humans and AI: play local files + 30 streaming sites, clip/GIF, local whisper subtitles + translation, media library. 40 MCP tools.
  ```

- [ ] Open the PR; follow the repo's alphabetization/emoji-legend rules

---

## 8. Skills — Claude Code plugin marketplace

The repo is already a plugin marketplace via `.claude-plugin/marketplace.json`
(root) + `marketplace/.claude-plugin/plugin.json`. Users install with:

```
/plugin marketplace add zhitongblog/unflick
/plugin install unflick
```

- [ ] Verify locally: in Claude Code run the two commands above, confirm the five
      `unflick-*` skills load and the `unflick` MCP server connects
- [ ] 🔑 YOU — (optional) submit the plugin to community indexes, e.g. PR to an
      `awesome-claude-code` / `awesome-claude-skills` list with:

  ```markdown
  - [unflick](https://github.com/zhitongblog/unflick) - Skills + MCP to drive the unflick video player (play, clip/GIF, local subtitles, library).
  ```

> There is no first-party Anthropic "skill store" today; the plugin marketplace
> mechanism above **is** the supported distribution + install path. Anyone can
> `add` your repo and install — no central approval needed.

---

## Post-submission

- [ ] Add badges/links to the main `README.md` (npm, Smithery, Glama, MCP registry)
- [ ] On each release: bump version in `package.json` (wrapper),
      `server.json`, `manifest.json`, `glama.json`, both `.claude-plugin`
      manifests → re-run step 1 (`npm publish`) and step 2 (`mcp-publisher publish`).
      Glama/mcp.so/PulseMCP re-crawl automatically.

## What I generated vs. what needs you

| Artifact | Status |
|---|---|
| npm wrapper, server.json, smithery.yaml, glama.json, manifest.json | ✅ generated, validated |
| client configs (Claude Desktop / Cursor / Codex / VS Code) | ✅ generated |
| 5 skills + plugin marketplace manifests | ✅ generated |
| `npm publish`, `mcp-publisher publish`, web-form submits, GitHub PRs | 🔑 needs your login |
