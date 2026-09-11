import { describe, it, expect } from "vitest";
import { conflictMessage, actionName } from "./keybindConflict";
import en from "../i18n/en.json";
import zhCN from "../i18n/zh-CN.json";
import ja from "../i18n/ja.json";

const conflict = {
  kind: "conflict" as const,
  key: "f",
  actionId: "fullscreen",
  // What the backend carries: the English label from the Rust action table.
  label: "Fullscreen",
};

describe("the refusal a rebind gets", () => {
  it("names the key and the action", () => {
    const msg = conflictMessage(en.keybinds, conflict);
    expect(msg).toContain("F");
    expect(msg).toContain(en.keybinds.fullscreen);
    expect(msg).not.toContain("{key}");
    expect(msg).not.toContain("{action}");
  });

  // The bug: the whole sentence came from Rust, so a Chinese interface showed
  // `f is already bound to "Fullscreen" — rebind or reset that first`.
  it("is written in the interface's language, label and all", () => {
    for (const bundle of [zhCN, ja]) {
      const msg = conflictMessage(bundle.keybinds, conflict);
      expect(msg).toContain(bundle.keybinds.fullscreen);
      expect(msg).not.toContain("Fullscreen");
      expect(msg).not.toContain("already bound");
    }
  });

  it("falls back to the backend's label for an action it does not know", () => {
    const msg = conflictMessage(en.keybinds, { ...conflict, actionId: "no_such_action" });
    expect(msg).toContain("Fullscreen");
  });

  it("keeps a non-conflict rejection verbatim — it is a bug report, not a choice", () => {
    const msg = conflictMessage(en.keybinds, {
      kind: "rejected",
      detail: "unknown action: frobnicate (see `unflick keybind list`)",
    });
    expect(msg).toBe("unknown action: frobnicate (see `unflick keybind list`)");
  });

  it("never returns a group object as an action name", () => {
    // `keybinds.groups` is an object sitting in the same bundle.
    expect(actionName(en.keybinds, "groups", "Fallback")).toBe("Fallback");
  });
});
