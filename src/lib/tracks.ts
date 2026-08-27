/**
 * The shape `audio_list` actually returns, and the one way to label it.
 *
 * Both existed before, in two places that disagreed. The React audio menu
 * derived a label from `title` / `lang` / `id`; the Windows native menu
 * declared its own local type with `label` and `active` fields the backend
 * has never sent, so every track in it read "undefined" and none was ever
 * ticked. `invoke<T>()` is an unchecked cast, so nothing caught it.
 *
 * Mirrors `core::types::AudioTrack`.
 */
export interface AudioTrack {
  id: number;
  title: string | null;
  lang: string | null;
  codec: string | null;
  selected: boolean;
}

/**
 * What to call an audio track. Falls back through the same ladder the
 * subtitle list uses: the embedded title, then the language, then the bare
 * track number — a stream with no metadata still needs to be selectable.
 */
export function audioTrackLabel(track: AudioTrack): string {
  if (track.title) return track.title;
  if (track.lang) return `Track ${track.id} (${track.lang})`;
  return `Track ${track.id}`;
}
