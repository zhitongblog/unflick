/**
 * Turning a KeyboardEvent into the canonical chord string that
 * `core::keybind` stores and validates.
 *
 * ## Why `event.code` and not `event.key`
 *
 * `event.key` reports the character produced, which folds in the shift
 * state and the keyboard layout. `Shift+,` arrives as `"<"` on a US
 * layout and as something else again on a German one, so a binding
 * recorded on one machine wouldn't fire on another — and a default
 * written as `Shift+,` would never match at all.
 *
 * `event.code` names the physical key, so `KeyZ` is the key where Z sits
 * on a US board regardless of what the layout prints on it. That is the
 * same choice mpv and most games make: shortcuts follow finger position.
 * The consequence worth knowing is that on an AZERTY keyboard the key
 * labelled A reports `KeyQ` — bindings stay where the muscle memory is,
 * not where the legend is.
 */

/** Physical key code → the character we display and store. */
const CODE_TO_KEY: Record<string, string> = {
  Comma: ",",
  Period: ".",
  Slash: "/",
  Semicolon: ";",
  Quote: "'",
  BracketLeft: "[",
  BracketRight: "]",
  Backslash: "\\",
  Minus: "-",
  Equal: "=",
  Backquote: "`",
  Space: "Space",
};

/** Keys that are only ever modifiers — never a chord on their own. */
const MODIFIER_CODES = new Set([
  "ShiftLeft", "ShiftRight",
  "ControlLeft", "ControlRight",
  "AltLeft", "AltRight",
  "MetaLeft", "MetaRight",
  "CapsLock",
]);

/**
 * Canonical chord for an event, or `null` if the event isn't one — a bare
 * modifier press, or a key we can't name.
 *
 * Modifier order matches `core::keybind::normalize`: `Mod+Alt+Shift+key`.
 */
export function eventToKey(e: KeyboardEvent): string | null {
  if (MODIFIER_CODES.has(e.code)) return null;

  let key: string | undefined = CODE_TO_KEY[e.code];

  if (!key) {
    if (/^Key[A-Z]$/.test(e.code)) {
      key = e.code.slice(3).toLowerCase();
    } else if (/^Digit[0-9]$/.test(e.code)) {
      key = e.code.slice(5);
    } else if (/^(Arrow(Left|Right|Up|Down)|Page(Up|Down)|Home|End|Insert|Delete|Backspace|Enter|Tab|Escape|F\d{1,2})$/.test(e.code)) {
      key = e.code;
    }
  }

  // Numpad, media keys, and anything exotic: fall back to the character.
  // Better an imperfect binding than none at all.
  if (!key) {
    if (!e.key || e.key === "Unidentified") return null;
    key = e.key === " " ? "Space" : e.key.length === 1 ? e.key.toLowerCase() : e.key;
  }

  let chord = "";
  // Ctrl and Cmd are one modifier as far as bindings go, so a single
  // table works on all three platforms.
  if (e.ctrlKey || e.metaKey) chord += "Mod+";
  if (e.altKey) chord += "Alt+";
  if (e.shiftKey) chord += "Shift+";
  return chord + key;
}

/**
 * Chord formatted for display. Substitutes the symbols people expect on
 * their own platform — `⌘` reads as native on macOS, `Ctrl` everywhere
 * else — and spaces the parts out so `Mod+Shift+p` doesn't read as one
 * long token.
 */
export function formatKey(chord: string, isMac = detectMac()): string {
  const parts = chord.split("+").filter((p) => p !== "");
  // A trailing empty segment means the chord's key *is* "+".
  if (chord.endsWith("+") && !chord.endsWith("++")) parts.push("+");

  return parts
    .map((p) => {
      if (p === "Mod") return isMac ? "⌘" : "Ctrl";
      if (p === "Alt") return isMac ? "⌥" : "Alt";
      if (p === "Shift") return isMac ? "⇧" : "Shift";
      if (p === "ArrowLeft") return "←";
      if (p === "ArrowRight") return "→";
      if (p === "ArrowUp") return "↑";
      if (p === "ArrowDown") return "↓";
      if (p === "PageUp") return "PgUp";
      if (p === "PageDown") return "PgDn";
      if (p.length === 1) return p.toUpperCase();
      return p;
    })
    .join(isMac ? "" : " + ");
}

function detectMac(): boolean {
  if (typeof navigator === "undefined") return false;
  return /Mac|iPhone|iPad/i.test(navigator.userAgent);
}
