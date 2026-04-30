import { getVersion } from "@tauri-apps/api/app";

/**
 * Update check protocol — modeled after solomd.app's pattern.
 *
 * Two sources, in priority order:
 *   1. unflick.app /api/stats — Cloudflare Pages Function that proxies the
 *      GitHub Releases API server-side, edge-cached for ~5 min. Hitting our
 *      own domain means clients aren't subject to GitHub's per-IP rate limit
 *      (relevant when many installs share a NAT egress).
 *   2. Direct api.github.com/repos/zhitongblog/unflick/releases/latest —
 *      fallback if the proxy is unreachable (offline, blocked, etc).
 *
 * If both fail we surface error: true so the UI can show "couldn't check"
 * instead of silently lying with "you're up to date".
 */

const STATS_URL = "https://unflick.app/api/stats";
const GITHUB_FALLBACK_URL =
  "https://api.github.com/repos/zhitongblog/unflick/releases/latest";
const RELEASES_PAGE = "https://github.com/zhitongblog/unflick/releases";

export interface UpdateResult {
  current: string;
  latest: string | null;
  hasUpdate: boolean;
  url: string;
  /** True when neither source could be reached. UI should show
   *  "couldn't check, retry" rather than "up to date". */
  error: boolean;
}

/** Returns 1 if a > b, -1 if a < b, 0 if equal. Tolerates leading "v". */
function compareSemver(a: string, b: string): number {
  const pa = a.replace(/^v/, "").split(".").map(Number);
  const pb = b.replace(/^v/, "").split(".").map(Number);
  const len = Math.max(pa.length, pb.length);
  for (let i = 0; i < len; i++) {
    const na = pa[i] || 0;
    const nb = pb[i] || 0;
    if (na > nb) return 1;
    if (na < nb) return -1;
  }
  return 0;
}

async function fetchFromStatsProxy(): Promise<{ tag: string; url: string } | null> {
  try {
    const res = await fetch(STATS_URL, { cache: "no-store" });
    if (!res.ok) return null;
    const data = (await res.json()) as {
      latest_tag?: string | null;
      latest_url?: string | null;
    };
    if (!data.latest_tag) return null;
    return {
      tag: data.latest_tag,
      url: data.latest_url || RELEASES_PAGE,
    };
  } catch {
    return null;
  }
}

async function fetchFromGitHubDirect(): Promise<{ tag: string; url: string } | null> {
  try {
    const res = await fetch(GITHUB_FALLBACK_URL, { cache: "no-store" });
    if (!res.ok) return null;
    const data = (await res.json()) as {
      tag_name?: string;
      html_url?: string;
    };
    if (!data.tag_name) return null;
    return { tag: data.tag_name, url: data.html_url || RELEASES_PAGE };
  } catch {
    return null;
  }
}

export async function checkForUpdate(): Promise<UpdateResult> {
  const current = await getVersion().catch(() => "0.0.0");

  // Try our own proxy first (no rate limit, edge-cached).
  let info = await fetchFromStatsProxy();
  // Fall back to GitHub direct if the proxy is unreachable. Most users won't
  // share an IP that's already exhausted GitHub's anonymous quota.
  if (!info) info = await fetchFromGitHubDirect();

  if (!info) {
    return {
      current,
      latest: null,
      hasUpdate: false,
      url: RELEASES_PAGE,
      error: true,
    };
  }

  const hasUpdate = compareSemver(info.tag, current) > 0;
  return {
    current,
    latest: info.tag.replace(/^v/, ""),
    hasUpdate,
    url: info.url,
    error: false,
  };
}
