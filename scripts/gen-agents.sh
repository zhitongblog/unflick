#!/usr/bin/env bash
#
# Generate AGENTS.md from CLAUDE.md.
#
# The two files carry the same instructions — they differ only in which agent
# is named, which in a 520-line document is four lines. The previous AGENTS.md
# was a hand-made copy, and it rotted exactly the way a hand-made copy of a
# 99%-identical document rots: it sat at the June text while CLAUDE.md grew
# through v0.11-v0.14, so the agent reading it was told about a project three
# versions out of date. Generating it is the only version of this that stays
# true.
#
#   ./scripts/gen-agents.sh            rewrite AGENTS.md
#   ./scripts/gen-agents.sh --check    fail if AGENTS.md is out of date (CI)
#
# `perl` rather than `sed` on purpose: \b is a GNU extension, and this has to
# produce byte-identical output on a maintainer's macOS (BSD sed) and on the
# Linux runner that checks it.

set -euo pipefail

cd "$(dirname "$0")/.."

SOURCE="CLAUDE.md"
TARGET="AGENTS.md"

render() {
  cat <<'HEADER'
<!--
  Generated from CLAUDE.md by scripts/gen-agents.sh — do not edit by hand.
  Edit CLAUDE.md and re-run the script; CI fails if the two drift apart.
-->
HEADER
  # "the agent", not "Codex": AGENTS.md is read by whatever coding agent is
  # in front of the repository, and naming one of them makes the sentence
  # wrong for the rest.
  perl -pe 's/\bClaude\b/the agent/g; s/^- the agent /- The agent /' "$SOURCE"
}

if [ "${1:-}" = "--check" ]; then
  if [ ! -f "$TARGET" ]; then
    echo "$TARGET is missing — run ./scripts/gen-agents.sh" >&2
    exit 1
  fi
  if ! diff -u "$TARGET" <(render) > /tmp/agents-drift.$$ 2>&1; then
    echo "$TARGET is out of date with $SOURCE:" >&2
    sed 's/^/  /' /tmp/agents-drift.$$ >&2
    rm -f /tmp/agents-drift.$$
    echo "" >&2
    echo "Run ./scripts/gen-agents.sh and commit the result." >&2
    exit 1
  fi
  rm -f /tmp/agents-drift.$$
  echo "$TARGET is current with $SOURCE"
elif [ $# -gt 0 ]; then
  echo "unknown argument: $1" >&2
  exit 1
else
  render > "$TARGET"
  echo "wrote $TARGET from $SOURCE"
fi
