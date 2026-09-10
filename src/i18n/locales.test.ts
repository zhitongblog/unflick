import { describe, it, expect } from "vitest";
import { LOCALES, DEFAULT_LOCALE, type Locale } from "./config";

import en from "./en.json";
import zhCN from "./zh-CN.json";
import zhTW from "./zh-TW.json";
import ja from "./ja.json";
import ko from "./ko.json";
import de from "./de.json";
import fr from "./fr.json";
import es from "./es.json";

/**
 * `useStrings()` casts every non-English bundle to `typeof en`, so a key
 * missing from one of them is a lie TypeScript cannot catch: the label just
 * renders blank at runtime, in one language, on one screen. These three
 * assertions are the only thing standing between that and a release.
 */

const BUNDLES: Record<Locale, unknown> = {
  en,
  "zh-CN": zhCN,
  "zh-TW": zhTW,
  ja,
  ko,
  de,
  fr,
  es,
};

type Leaf = { path: string; value: string };

function leaves(node: unknown, prefix = ""): Leaf[] {
  if (typeof node === "string") return [{ path: prefix, value: node }];
  if (!node || typeof node !== "object") {
    throw new Error(`unexpected non-string leaf at ${prefix || "<root>"}`);
  }
  return Object.entries(node as Record<string, unknown>).flatMap(([k, v]) =>
    leaves(v, prefix ? `${prefix}.${k}` : k),
  );
}

function placeholders(value: string): string[] {
  return (value.match(/\{[a-zA-Z_][a-zA-Z0-9_]*\}/g) ?? []).slice().sort();
}

const enLeaves = leaves(en);
const enPaths = enLeaves.map((l) => l.path);
const enByPath = new Map(enLeaves.map((l) => [l.path, l.value]));

describe("locale bundles", () => {
  it("covers every locale the picker offers", () => {
    for (const locale of LOCALES) {
      expect(BUNDLES[locale], `no bundle imported for ${locale}`).toBeTruthy();
    }
    expect(LOCALES).toContain(DEFAULT_LOCALE);
  });

  for (const locale of LOCALES) {
    if (locale === "en") continue;

    it(`${locale} has exactly the keys en has`, () => {
      const paths = leaves(BUNDLES[locale]).map((l) => l.path);
      const missing = enPaths.filter((p) => !paths.includes(p));
      const orphan = paths.filter((p) => !enPaths.includes(p));
      expect(missing, `${locale} is missing keys`).toEqual([]);
      expect(orphan, `${locale} has keys en does not`).toEqual([]);
    });

    it(`${locale} has no blank strings`, () => {
      const blank = leaves(BUNDLES[locale])
        .filter((l) => l.value.trim() === "")
        .map((l) => l.path);
      expect(blank, `${locale} has empty strings`).toEqual([]);
    });

    it(`${locale} keeps every placeholder en uses`, () => {
      const wrong: string[] = [];
      for (const leaf of leaves(BUNDLES[locale])) {
        const source = enByPath.get(leaf.path);
        if (source === undefined) continue;
        const want = placeholders(source);
        const got = placeholders(leaf.value);
        if (want.join(",") !== got.join(",")) {
          wrong.push(`${leaf.path}: expected ${want.join(",") || "none"}, got ${got.join(",") || "none"}`);
        }
      }
      expect(wrong, `${locale} dropped or invented placeholders`).toEqual([]);
    });
  }
});
