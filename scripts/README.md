# Release scripts

How an unflick release is cut, and why it is split the way it is.

```
./scripts/release.sh 0.13.0        # test, bump, commit, tag, push
        ↓ (tag triggers CI)
  .github/workflows/release.yml    # Windows + Linux → draft release
        ↓
./scripts/build-mac.sh --upload    # signed + notarized .dmg → same draft
        ↓
  write the notes, publish the draft
```

## Why the split

Windows and Linux build unattended, so they belong in CI. macOS does not: the
`.dmg` has to be signed with a Developer ID and notarized by Apple, and those
credentials are on the maintainer's machine, not in the repository. Apple's
`notarytool` is also the step most likely to hang on a hosted runner without
explaining itself. So macOS is built here and uploaded into the draft the CI
run opened.

The release stays a **draft** until the macOS half arrives. A published
release that is one platform short is worse than no release at all — the
install page starts handing out 404s.

## The scripts

| Script | What it does |
|---|---|
| `release.sh <version>` | Refuses a dirty tree, a non-`master` branch, a diverged remote or an existing tag. Runs the frontend and Rust test suites. Bumps `package.json`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json` (and `Cargo.lock`), commits, tags, pushes. |
| `build-mac.sh [--upload]` | Universal `.app`, Developer ID signature, notarization, stapled `.dmg`. Verifies all three before it finishes. `--upload` attaches it to the draft. |
| `fetch-windows-deps.sh` | Downloads libmpv, ffmpeg, yt-dlp and whisper.cpp into `src-tauri/`. Used by CI; also what you run on a fresh Windows checkout. |

## Credentials

`build-mac.sh` reads `.env.local` (gitignored) or the environment:

```sh
APPLE_SIGNING_IDENTITY="Developer ID Application: xiangdong li (6NQM3XP5RF)"
# App Store Connect key — preferred, does not expire with a password change
APPLE_API_KEY=…            # key id
APPLE_API_ISSUER=…         # issuer uuid
APPLE_API_KEY_PATH=…       # path to AuthKey_<id>.p8
# or the fallback
APPLE_ID=… APPLE_PASSWORD=… APPLE_TEAM_ID=…
```

CI needs nothing beyond the default `GITHUB_TOKEN`.

## The bundled binaries

`src-tauri/{mpv-dev,ffmpeg,yt-dlp,whisper}` are not in git — together they are
~450 MB of third-party build output with their own release cadence.
`fetch-windows-deps.sh` pulls them from upstream, which is what made the
Windows installers buildable anywhere rather than only on one machine.

Only Windows bundles them. macOS and Linux load the system libmpv (Homebrew /
the distro package), which is why those downloads are ~15 MB rather than 80.
