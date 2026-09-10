/**
 * Asking for the welcome screen back.
 *
 * Two places offer it — the idle screen's "What is unflick?" and the
 * Settings row — and both are far enough from App's state that threading a
 * callback down would mean prop-drilling through components that care about
 * neither. Same shape as `FIND_SUBTITLES_EVENT` in `lib/subtitleSearch.ts`.
 */
export const SHOW_ONBOARDING_EVENT = "unflick:show-onboarding";
