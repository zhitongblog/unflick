import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

const host = process.env.TAURI_DEV_HOST;

export default defineConfig(async () => ({
  plugins: [react()],
  clearScreen: false,
  // A worktree is a full checkout, and `.claude/worktrees/` puts one *inside*
  // the project. Vitest's default include walks the root, so every test file
  // in every parked worktree was counted as well: this suite reported 358
  // tests across 29 files when it has 125 across 9, and the inflated number
  // reached three sets of release notes before anyone noticed. A test count
  // that silently triples is worse than no count at all.
  test: {
    exclude: [
      "**/node_modules/**",
      "**/dist/**",
      "**/.claude/**",
      "**/src-tauri/target/**",
    ],
  },
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
}));
