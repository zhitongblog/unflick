/**
 * Sites whose URLs we recognise up-front so the UI can show a labelled
 * "Resolving YouTube link..." spinner during yt-dlp extraction. yt-dlp
 * itself supports 1500+ sites — anything not in this list still works,
 * it just falls back to a generic "Resolving link..." status.
 *
 * Patterns are matched case-insensitively against the full URL string.
 * Order matters: the first match wins.
 */

export interface StreamingSite {
  /** Display label, shown in the dialog spinner. */
  name: string;
  /** Case-insensitive host match. */
  regex: RegExp;
}

export const STREAMING_SITES: StreamingSite[] = [
  { name: "YouTube", regex: /(?:youtube\.com|youtu\.be)/i },
  { name: "Bilibili", regex: /(?:bilibili\.com|b23\.tv)/i },
  { name: "Twitch", regex: /twitch\.tv/i },
  { name: "Vimeo", regex: /vimeo\.com/i },
  { name: "Douyin", regex: /douyin\.com/i },
  { name: "TikTok", regex: /tiktok\.com/i },
  { name: "Weibo", regex: /(?:weibo\.com|weibo\.cn)/i },
  { name: "Reddit", regex: /(?:reddit\.com|redd\.it)/i },
  // Twitter/X share the same yt-dlp extractor; label as "X (Twitter)".
  { name: "X (Twitter)", regex: /(?:twitter\.com|x\.com)/i },
  { name: "Instagram", regex: /instagram\.com/i },
  { name: "Facebook", regex: /(?:facebook\.com|fb\.watch)/i },
  { name: "Dailymotion", regex: /(?:dailymotion\.com|dai\.ly)/i },
  { name: "SoundCloud", regex: /soundcloud\.com/i },
  { name: "NicoNico", regex: /(?:nicovideo\.jp|nico\.ms)/i },
  { name: "Streamable", regex: /streamable\.com/i },
  { name: "Kick", regex: /kick\.com/i },
  { name: "AcFun", regex: /acfun\.cn/i },
  { name: "西瓜视频", regex: /ixigua\.com/i },
  { name: "Xiaohongshu", regex: /(?:xiaohongshu\.com|xhslink\.com)/i },
  { name: "优酷", regex: /youku\.com/i },
  { name: "爱奇艺", regex: /iqiyi\.com/i },
  { name: "腾讯视频", regex: /v\.qq\.com/i },
  { name: "芒果TV", regex: /mgtv\.com/i },
  { name: "搜狐视频", regex: /(?:tv\.sohu\.com|my\.tv\.sohu\.com)/i },
  { name: "土豆", regex: /tudou\.com/i },
  { name: "VK", regex: /(?:vk\.com|vkvideo\.ru)/i },
  { name: "Odysee", regex: /odysee\.com/i },
  { name: "Rumble", regex: /rumble\.com/i },
  { name: "Pinterest", regex: /(?:pinterest\.com|pin\.it)/i },
];

/**
 * Return the display label for a recognised host, or `null` for unknown
 * URLs. Callers should still attempt extraction on unknowns — yt-dlp's
 * site list is much wider than ours.
 */
export function detectStreamingSite(url: string): string | null {
  for (const site of STREAMING_SITES) {
    if (site.regex.test(url)) return site.name;
  }
  return null;
}

/**
 * `true` for any string that looks like an HTTP(S) URL (the only kind we
 * pipe through yt-dlp). Local file paths and other schemes return `false`.
 */
export function isHttpUrl(url: string): boolean {
  return /^https?:/i.test(url);
}
