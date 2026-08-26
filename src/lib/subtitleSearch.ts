/**
 * Ask App to open the online-subtitle dialog.
 *
 * An event rather than a prop because there are two callers that can't share
 * one: the React subtitle popover, and the native Win32 menu PlayerBar builds
 * imperatively on Windows. Threading a callback down to both would mean
 * giving the prop-less PlayerBar a prop purely to forward it.
 */
export const FIND_SUBTITLES_EVENT = "unflick:find-subtitles-online";

export function findSubtitlesOnline() {
  window.dispatchEvent(new CustomEvent(FIND_SUBTITLES_EVENT));
}
