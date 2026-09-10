/**
 * Turning a failed open into something worth reading.
 *
 * The backend hands the GUI a raw message — mpv's, or `core::player`'s
 * "could not open <path>". That string is the right thing to keep for a bug
 * report and the wrong thing to lead with, so this classifies it into the
 * handful of causes that have different fixes, and always carries the
 * original along in `detail`.
 *
 * ## Why the advice for smb:// lives here as well as in Rust
 *
 * `core::source::unsupported_message` already writes per-platform mount
 * advice, but it is only reached from the CLI/daemon path — `player_play`
 * (the Tauri command the window uses) goes straight to `Player::play`. So a
 * share URL dropped on the window or typed into the URL dialog gets mpv's
 * bare refusal and no advice at all. Until the Rust side exposes the check
 * as a command, the window needs its own copy. It is duplication, and it is
 * better than the alternative, which is silence.
 */

export type OpenErrorKind =
  | "unsupported_scheme"
  | "network"
  | "ytdlp"
  | "drm"
  | "unreadable";

export interface ClassifiedOpenError {
  kind: OpenErrorKind;
  /** Lowercased URL scheme of the target, when it had one. */
  scheme: string | null;
  /** The backend's own message, verbatim. Never empty. */
  detail: string;
}

/**
 * Schemes this build can actually hand to mpv. Anything else that looks like
 * a URL is a protocol we have no support for, which is a different failure
 * from "the server said no".
 */
const PLAYABLE_SCHEMES = new Set([
  "http",
  "https",
  "file",
  "rtsp",
  "rtsps",
  "rtmp",
  "rtmps",
  "srt",
  "udp",
  "rtp",
  "mms",
  "mmsh",
  "data",
  "av",
  "lavf",
  "edl",
]);

/**
 * The URL scheme of `input`, lowercased, or `null` for a plain path.
 *
 * Mirrors `core::source::scheme_of` deliberately, including its two rules:
 * `://` is required so `D:\media` is never a scheme, and a one-character
 * scheme is rejected so `d://media/film.mkv` reads as the mangled path it is.
 */
export function schemeOf(input: string): string | null {
  const at = input.indexOf("://");
  if (at <= 0) return null;
  const scheme = input.slice(0, at);
  if (scheme.length < 2) return null;
  if (!/^[A-Za-z][A-Za-z0-9+\-.]*$/.test(scheme)) return null;
  return scheme.toLowerCase();
}

/** A printable version of whatever the rejection carried. */
function toDetail(raw: unknown): string {
  if (typeof raw === "string" && raw.trim() !== "") return raw;
  if (raw instanceof Error && raw.message.trim() !== "") return raw.message;
  if (raw === null || raw === undefined) return "";
  if (typeof raw === "object") {
    // `String(obj)` is "[object Object]", which tells a bug report nothing.
    try {
      return JSON.stringify(raw);
    } catch {
      return "";
    }
  }
  return String(raw);
}

/**
 * What went wrong, as far as the renderer can honestly tell.
 *
 * Note the kind that is deliberately absent: there is no "file not found".
 * The WebView cannot stat a path, and a missing file, a permission denial
 * and an unreadable disk all arrive as the same `could not open <path>`.
 * Claiming the file was moved or deleted would be diagnosis by invention;
 * `unreadable` covers all three without lying about any of them.
 */
export function classifyOpenError(
  target: string | null | undefined,
  raw: unknown,
): ClassifiedOpenError {
  const detail = toDetail(raw);
  const lower = detail.toLowerCase();
  const scheme = target ? schemeOf(target) : null;

  // The message names the missing tool, so it wins over the target: a
  // YouTube link that failed for want of yt-dlp is not a network problem,
  // and telling someone to check their connection would waste their time.
  if (lower.includes("yt-dlp") || lower.includes("yt_dlp") || lower.includes("ytdlp")) {
    return { kind: "ytdlp", scheme, detail };
  }
  if (lower.includes("drm") || lower.includes("widevine") || lower.includes("encrypted")) {
    return { kind: "drm", scheme, detail };
  }
  if (scheme && !PLAYABLE_SCHEMES.has(scheme)) {
    return { kind: "unsupported_scheme", scheme, detail };
  }
  if (scheme === "http" || scheme === "https") {
    return { kind: "network", scheme, detail };
  }
  return { kind: "unreadable", scheme, detail };
}

export type Platform = "windows" | "mac" | "linux";

/** Which platform the advice should be written for. */
export function detectPlatform(ua: string | undefined): Platform {
  if (!ua) return "linux";
  if (/Win(dows|32|64)/i.test(ua)) return "windows";
  if (/Mac|iPhone|iPad/i.test(ua)) return "mac";
  return "linux";
}

/**
 * The `openError.*` i18n key holding the advice for this failure.
 *
 * Returned as a key rather than a sentence so the advice is translated like
 * everything else — the Rust copy of it is English-only.
 */
export function hintKeyFor(
  error: ClassifiedOpenError,
  platform: Platform,
): string {
  if (error.kind === "unsupported_scheme") {
    const suffix = platform === "windows" ? "Windows" : platform === "mac" ? "Mac" : "Linux";
    if (error.scheme === "smb" || error.scheme === "cifs") return `hintSmb${suffix}`;
    if (error.scheme === "nfs") return `hintNfs${suffix}`;
    return "hintSchemeOther";
  }
  if (error.kind === "network") return "hintNetwork";
  if (error.kind === "ytdlp") return "hintYtdlp";
  if (error.kind === "drm") return "hintDrm";
  return "hintUnreadable";
}

/** The `openError.*` i18n key holding the headline for this failure. */
export function titleKeyFor(error: ClassifiedOpenError): string {
  switch (error.kind) {
    case "unsupported_scheme":
      return "titleUnsupportedScheme";
    case "network":
      return "titleNetwork";
    case "ytdlp":
      return "titleYtdlp";
    case "drm":
      return "titleDrm";
    default:
      return "titleUnreadable";
  }
}
