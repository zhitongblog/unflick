import { describe, it, expect } from "vitest";
import ts from "typescript";

/**
 * The other half of the i18n guard. `locales.test.ts` proves the eight
 * bundles agree with each other; this proves the components actually ask
 * them. Both failures look the same from the outside — an English label in
 * a Chinese window — but only one of them is visible to a key-parity check,
 * because a string that never went through `t()` is not a missing key. It
 * is a key that was never written.
 *
 * Nothing headless catches this either: a hardcoded label renders happily,
 * passes every unit test, and is only wrong to someone reading the window.
 * So the check is on the source instead: parse each component and look at
 * the three places user-visible text actually lives — JSX text, the
 * attributes a person can read (title, aria-label, placeholder, alt), and
 * the `label` / `message` fields that build native menus and toasts. A
 * string literal in one of those, outside the exemptions below, is a label
 * someone will read in the wrong language.
 *
 * Deliberately narrow. `className`, `style`, mpv property names and every
 * other string in a component are invisible to this test, because a check
 * that flags them gets muted within a week and then catches nothing.
 */

/**
 * Every component, as source text. Read through Vite rather than `fs` so
 * the test needs no Node type declarations and no path arithmetic: the
 * keys are the paths, relative to this file.
 */
const COMPONENTS = import.meta.glob(["../**/*.tsx", "!../**/*.test.tsx"], {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;

/** "../components/Player/PlayerBar.tsx" → "components/Player/PlayerBar.tsx" */
function displayPath(key: string): string {
  return key.replace(/^\.\.\//, "");
}

/** Attributes a person reads. `className` and friends are not text. */
const VISIBLE_ATTRS = new Set(["title", "aria-label", "placeholder", "alt"]);

/** Object fields that become native menu entries and toast bodies. */
const VISIBLE_PROPS = new Set(["label", "message", "hint", "tooltip", "caption"]);

/**
 * Exempt strings — each one is a thing that would be *worse* translated.
 * Keep this list short. If it starts growing, the rule above is wrong, not
 * the code being flagged.
 */
const ALLOWED = new Set([
  // Brand. The product is called unflick in every language.
  "unflick",
  "unflick v",
  // Proper nouns: the browsers the cookie picker reads from.
  "Firefox", "Chrome", "Chromium", "Safari", "Edge", "Brave",
  // Shown verbatim so they can be typed or clicked — a translated URL or
  // example path is a broken one.
  "opensubtitles.com",
  "github.com/yt-dlp/yt-dlp",
  "path/to/whisper-cli.exe",
  "path/to/ggml-base.en.bin",
  // Example locale codes, in the field that wants literal locale codes.
  "en", "en,zh-CN", "zh-CN,en",
  // Units and format tokens.
  "dB", "2160p", "1440p", "1080p", "720p", "480p",
]);

/** Modifier and named keys, for the shortcut hints spliced onto tooltips. */
const KEY_NAMES = new Set([
  "ctrl", "shift", "alt", "cmd", "meta", "super", "opt", "option",
  "esc", "escape", "enter", "return", "space", "tab", "backspace", "del",
  "delete", "home", "end", "pgup", "pgdn", "up", "down", "left", "right",
]);

function isKeyToken(token: string): boolean {
  const t = token.trim().toLowerCase();
  if (!t) return false;
  if (KEY_NAMES.has(t)) return true;
  if (/^f\d{1,2}$/.test(t)) return true;
  return t.length === 1; // a single letter, digit or symbol: a key
}

/**
 * `Bookmarks (B / Shift+B)` — the tooltip is translated, the hint after it
 * is a list of keys and stays put. By the time this sees it, the `${t.…}`
 * part is gone and only the parenthesised remainder is left.
 */
function isShortcutHint(value: string): boolean {
  const inner = value.trim().replace(/^\(/, "").replace(/\)$/, "").trim();
  if (!inner) return false;
  return inner
    .split("/")
    .every((chord) => chord.split("+").every(isKeyToken));
}

/** HTML entities are markup, not words: `&middot;` is a dot. */
function hasWords(value: string): boolean {
  const text = value.replace(/&[a-zA-Z]+;|&#\d+;/g, "");
  return /[A-Za-z]{2,}/.test(text);
}

function isExempt(value: string): boolean {
  const v = value.trim();
  if (!v) return true;
  if (ALLOWED.has(v)) return true;
  if (!hasWords(v)) return true; // punctuation, ticks, numbers, timestamps
  if (v.length === 1) return true; // a marker: the A and B of an A-B loop
  if (isShortcutHint(v)) return true;
  return false;
}

const COMPARISON = new Set([
  ts.SyntaxKind.EqualsEqualsToken,
  ts.SyntaxKind.EqualsEqualsEqualsToken,
  ts.SyntaxKind.ExclamationEqualsToken,
  ts.SyntaxKind.ExclamationEqualsEqualsToken,
]);

/**
 * Every literal in `node` that ends up on screen. Skips comparison
 * operands (`state === "playing"` is a state name), call arguments (an mpv
 * property, a Tauri command) and nested JSX, which the walker reaches on
 * its own. Template literals contribute their fixed parts, so
 * `` `${t.music.toggle} (Ctrl+M)` `` is judged on " (Ctrl+M)" alone.
 */
function renderedLiterals(node: ts.Node, out: string[]): void {
  const visit = (n: ts.Node): void => {
    if (ts.isJsxElement(n) || ts.isJsxSelfClosingElement(n) || ts.isJsxFragment(n)) return;
    if (ts.isBinaryExpression(n) && COMPARISON.has(n.operatorToken.kind)) return;
    if (ts.isCallExpression(n)) return;
    if (ts.isStringLiteral(n) || ts.isNoSubstitutionTemplateLiteral(n)) {
      out.push(n.text);
      return;
    }
    if (ts.isTemplateExpression(n)) {
      out.push(n.head.text);
      for (const span of n.templateSpans) {
        out.push(span.literal.text);
        visit(span.expression);
      }
      return;
    }
    n.forEachChild(visit);
  };
  visit(node);
}

/** `invoke("cmd", { … })` carries a Rust payload, not anything on screen. */
function insideInvokeArgs(node: ts.Node): boolean {
  for (let n: ts.Node | undefined = node; n; n = n.parent) {
    if (
      ts.isCallExpression(n) &&
      ts.isIdentifier(n.expression) &&
      n.expression.text === "invoke"
    ) {
      return true;
    }
  }
  return false;
}

/** A command line is not a sentence; a translation can only break it. */
function insideCodeElement(node: ts.JsxText): boolean {
  const parent = node.parent;
  return (
    ts.isJsxElement(parent) &&
    ts.isIdentifier(parent.openingElement.tagName) &&
    parent.openingElement.tagName.text === "code"
  );
}

type Finding = { where: string; what: string; value: string };

function scan(file: string, source: string): Finding[] {
  const sf = ts.createSourceFile(file, source, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
  const findings: Finding[] = [];

  const record = (node: ts.Node, what: string, values: string[]) => {
    const line = sf.getLineAndCharacterOfPosition(node.getStart(sf)).line + 1;
    for (const value of values) {
      if (isExempt(value)) continue;
      findings.push({
        where: `${file}:${line}`,
        what,
        value: value.replace(/\s+/g, " ").trim(),
      });
    }
  };

  const visit = (node: ts.Node): void => {
    if (ts.isJsxText(node)) {
      if (!insideCodeElement(node)) record(node, "JSX text", [node.text]);
    } else if (ts.isJsxAttribute(node) && VISIBLE_ATTRS.has(node.name.getText(sf))) {
      const values: string[] = [];
      if (node.initializer) renderedLiterals(node.initializer, values);
      record(node, `${node.name.getText(sf)}=`, values);
    } else if (
      ts.isJsxExpression(node) &&
      node.parent &&
      (ts.isJsxElement(node.parent) || ts.isJsxFragment(node.parent))
    ) {
      const values: string[] = [];
      if (node.expression) renderedLiterals(node.expression, values);
      record(node, "JSX child", values);
    } else if (
      ts.isPropertyAssignment(node) &&
      ts.isIdentifier(node.name) &&
      VISIBLE_PROPS.has(node.name.text) &&
      !insideInvokeArgs(node)
    ) {
      const values: string[] = [];
      renderedLiterals(node.initializer, values);
      record(node, `${node.name.text}:`, values);
    }
    node.forEachChild(visit);
  };

  visit(sf);
  return findings;
}

describe("no hardcoded user-visible strings", () => {
  const entries = Object.entries(COMPONENTS).sort(([a], [b]) => a.localeCompare(b));

  it("finds the components to check", () => {
    expect(entries.length).toBeGreaterThan(20);
  });

  for (const [key, source] of entries) {
    const file = displayPath(key);
    it(`${file} reads its labels from the bundle`, () => {
      const findings = scan(file, source).map(
        (f) => `${f.where}  ${f.what} ${JSON.stringify(f.value)}`,
      );
      expect(
        findings,
        "hardcoded text — wire it through useStrings(), or add it to ALLOWED in src/i18n/hardcoded.test.ts if it genuinely must not be translated",
      ).toEqual([]);
    });
  }
});
