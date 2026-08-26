import { describe, it, expect } from "vitest";
import { eventToKey, formatKey } from "./keys";

/**
 * These chords have to match what `core::keybind::normalize` produces on
 * the Rust side, byte for byte — a mismatch means a binding that's stored
 * fine and never fires. That's the failure this file is guarding against.
 */

/**
 * A plain object rather than a real `KeyboardEvent`, so these tests need
 * no DOM environment. `eventToKey` reads six fields and nothing else —
 * standing up jsdom to supply them would be cost without coverage.
 */
function key(init: {
  code: string;
  key?: string;
  ctrlKey?: boolean;
  metaKey?: boolean;
  altKey?: boolean;
  shiftKey?: boolean;
}): KeyboardEvent {
  return { key: "", ctrlKey: false, metaKey: false, altKey: false, shiftKey: false, ...init } as KeyboardEvent;
}

describe("eventToKey", () => {
  it("names letters by their physical key, lower-case", () => {
    expect(eventToKey(key({ code: "KeyZ", key: "z" }))).toBe("z");
    // Shift is carried as a modifier rather than folded into the letter,
    // so `z` and `Shift+z` stay distinct bindings.
    expect(eventToKey(key({ code: "KeyZ", key: "Z", shiftKey: true }))).toBe("Shift+z");
  });

  it("survives a shifted punctuation key", () => {
    // The reason this file uses `code` at all: on a US layout Shift+, has
    // `key === "<"`, so a default written as `Shift+,` would never match.
    expect(eventToKey(key({ code: "Comma", key: "<", shiftKey: true }))).toBe("Shift+,");
    expect(eventToKey(key({ code: "Comma", key: "," }))).toBe(",");
  });

  it("folds Ctrl and Cmd into one modifier", () => {
    // One binding table has to work on Windows, Linux and macOS.
    expect(eventToKey(key({ code: "KeyO", key: "o", ctrlKey: true }))).toBe("Mod+o");
    expect(eventToKey(key({ code: "KeyO", key: "o", metaKey: true }))).toBe("Mod+o");
  });

  it("orders modifiers canonically", () => {
    expect(
      eventToKey(key({ code: "KeyP", key: "P", ctrlKey: true, shiftKey: true, altKey: true })),
    ).toBe("Mod+Alt+Shift+p");
  });

  it("handles named keys and space", () => {
    expect(eventToKey(key({ code: "ArrowLeft", key: "ArrowLeft" }))).toBe("ArrowLeft");
    expect(eventToKey(key({ code: "PageUp", key: "PageUp" }))).toBe("PageUp");
    expect(eventToKey(key({ code: "Space", key: " " }))).toBe("Space");
    expect(eventToKey(key({ code: "F5", key: "F5" }))).toBe("F5");
  });

  it("falls back to the character when there is no usable code", () => {
    // Synthetic input — on-screen keyboards, remote-desktop stacks, the
    // automation used to test this app — often arrives with an empty code.
    expect(eventToKey(key({ code: "", key: " " }))).toBe("Space");
    expect(eventToKey(key({ code: "", key: "z" }))).toBe("z");
    expect(eventToKey(key({ code: "Unidentified", key: "q" }))).toBe("q");
  });

  it("returns null for a bare modifier press", () => {
    // Holding Shift on the way to a chord must not fire anything.
    for (const code of ["ShiftLeft", "ControlRight", "AltLeft", "MetaLeft", "CapsLock"]) {
      expect(eventToKey(key({ code }))).toBeNull();
    }
  });

  it("returns null when there is nothing identifiable at all", () => {
    expect(eventToKey(key({ code: "", key: "Unidentified" }))).toBeNull();
    expect(eventToKey(key({ code: "", key: "" }))).toBeNull();
  });

  it("keeps digits as digits", () => {
    expect(eventToKey(key({ code: "Digit5", key: "5" }))).toBe("5");
  });
});

describe("formatKey", () => {
  it("spells modifiers the way each platform does", () => {
    expect(formatKey("Mod+Shift+p", false)).toBe("Ctrl + Shift + P");
    expect(formatKey("Mod+Shift+p", true)).toBe("⌘⇧P");
  });

  it("substitutes arrow glyphs and short page labels", () => {
    expect(formatKey("ArrowLeft", false)).toBe("←");
    expect(formatKey("PageDown", false)).toBe("PgDn");
  });

  it("renders a lone punctuation key", () => {
    expect(formatKey("[", false)).toBe("[");
    expect(formatKey("\\", false)).toBe("\\");
  });

  it("renders a chord whose key is the plus sign", () => {
    // "+" is both the separator and a legitimate key.
    expect(formatKey("+", false)).toBe("+");
  });
});
