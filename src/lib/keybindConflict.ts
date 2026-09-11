/**
 * The sentence shown when a rebind is refused.
 *
 * It lives here, apart from the panel, because the bug it fixes was not a
 * rendering bug: the backend composed this sentence in Rust, English label and
 * all, and the window printed it verbatim. A Chinese interface said
 * `f is already bound to "Fullscreen" — rebind or reset that first`. There was
 * nothing to translate because the string never passed through i18n at all.
 *
 * Composing it here means it is translated like everything else, and testable
 * without a window.
 */
import { formatKey } from "./keys";
import type { BindFailure } from "../stores/keybindStore";

/** Strings this module needs — a narrow slice of the i18n bundle. */
export interface ConflictStrings {
  /** `{key}` and `{action}` are substituted. */
  conflict: string;
  [actionId: string]: unknown;
}

/**
 * The action's name in the interface's language.
 *
 * `keybinds` holds both per-action names and a `groups` object, so the lookup
 * has to prove it found a string. The fallback is the backend's English label —
 * worse than a translation, better than an empty sentence.
 */
export function actionName(
  strings: ConflictStrings,
  actionId: string,
  fallback: string,
): string {
  const name = strings[actionId];
  return typeof name === "string" ? name : fallback;
}

export function conflictMessage(
  strings: ConflictStrings,
  failure: BindFailure,
  isMac?: boolean,
): string {
  if (failure.kind !== "conflict") return failure.detail;
  return strings.conflict
    .replace("{key}", formatKey(failure.key, isMac))
    .replace("{action}", actionName(strings, failure.actionId, failure.label));
}
