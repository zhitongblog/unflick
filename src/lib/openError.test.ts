import { describe, it, expect } from "vitest";
import {
  classifyOpenError,
  detectPlatform,
  hintKeyFor,
  schemeOf,
  titleKeyFor,
} from "./openError";

describe("schemeOf", () => {
  it("does not mistake a Windows path for a scheme", () => {
    // Same rules as core::source::scheme_of — a drive letter is one
    // character, and a UNC path has no "://" at all.
    expect(schemeOf(String.raw`D:\media\film.mkv`)).toBeNull();
    expect(schemeOf(String.raw`\\server\share\film.mkv`)).toBeNull();
    expect(schemeOf("d://media/film.mkv")).toBeNull();
    expect(schemeOf("/Volumes/media/film.mkv")).toBeNull();
  });

  it("lowercases a real scheme", () => {
    expect(schemeOf("SMB://server/share/f.mkv")).toBe("smb");
    expect(schemeOf("https://example.com/f.mp4")).toBe("https");
  });
});

describe("classifyOpenError", () => {
  it("calls a share URL an unsupported scheme and keeps the scheme", () => {
    const smb = classifyOpenError("smb://nas/media/film.mkv", "Failed to open smb://nas/media/film.mkv");
    expect(smb.kind).toBe("unsupported_scheme");
    expect(smb.scheme).toBe("smb");

    const nfs = classifyOpenError("nfs://nas/export/film.mkv", "loading failed");
    expect(nfs.kind).toBe("unsupported_scheme");
    expect(nfs.scheme).toBe("nfs");
  });

  it("calls an http failure a network failure", () => {
    const http = classifyOpenError("https://example.com/f.mp4", "Failed to recognize file format.");
    expect(http.kind).toBe("network");
  });

  it("names the missing tool whatever the target was", () => {
    const local = classifyOpenError("/tmp/a.mkv", "yt-dlp not found on PATH");
    expect(local.kind).toBe("ytdlp");
    const remote = classifyOpenError("https://youtube.com/watch?v=x", "yt-dlp exited with status 1");
    expect(remote.kind).toBe("ytdlp");
  });

  it("calls a local failure unreadable, never 'file not found'", () => {
    // The renderer cannot stat, and a missing file, a permission denial and
    // a dead disk all arrive as the same message. Guessing would be a lie.
    const e = classifyOpenError("/tmp/gone.mkv", "could not open /tmp/gone.mkv");
    expect(e.kind).toBe("unreadable");
    expect(e.scheme).toBeNull();
  });

  it("keeps the backend message verbatim for every kind", () => {
    const cases: Array<[string, string]> = [
      ["smb://nas/f.mkv", "no smb:// support in this build"],
      ["https://example.com/f.mp4", "tcp: connection refused"],
      ["https://youtube.com/x", "yt-dlp exited with status 1"],
      ["https://netflix.com/x", "stream is DRM protected"],
      ["/tmp/gone.mkv", "could not open /tmp/gone.mkv"],
    ];
    for (const [target, raw] of cases) {
      expect(classifyOpenError(target, raw).detail).toBe(raw);
    }
  });

  it("never renders [object Object] for a non-string rejection", () => {
    expect(classifyOpenError("/tmp/a.mkv", new Error("boom")).detail).toBe("boom");
    expect(classifyOpenError("/tmp/a.mkv", { code: 5 }).detail).toBe('{"code":5}');
    expect(classifyOpenError("/tmp/a.mkv", undefined).detail).toBe("");
    for (const raw of [new Error("boom"), { code: 5 }, undefined]) {
      expect(classifyOpenError("/tmp/a.mkv", raw).kind).toBe("unreadable");
      expect(classifyOpenError("/tmp/a.mkv", raw).detail).not.toContain("[object");
    }
  });
});

describe("hintKeyFor", () => {
  it("gives share advice for the platform in hand", () => {
    const smb = classifyOpenError("smb://nas/f.mkv", "nope");
    expect(hintKeyFor(smb, "windows")).toBe("hintSmbWindows");
    expect(hintKeyFor(smb, "mac")).toBe("hintSmbMac");
    expect(hintKeyFor(smb, "linux")).toBe("hintSmbLinux");

    const nfs = classifyOpenError("nfs://nas/f.mkv", "nope");
    expect(hintKeyFor(nfs, "mac")).toBe("hintNfsMac");

    // A protocol with no mount story of its own falls back to the generic
    // line rather than borrowing SMB's, which would send people nowhere.
    const other = classifyOpenError("gopher://h/f.mkv", "nope");
    expect(hintKeyFor(other, "mac")).toBe("hintSchemeOther");
  });

  it("pairs a title with every hint", () => {
    const kinds = [
      classifyOpenError("smb://nas/f.mkv", "nope"),
      classifyOpenError("https://h/f.mp4", "nope"),
      classifyOpenError("https://h/f.mp4", "yt-dlp missing"),
      classifyOpenError("https://h/f.mp4", "DRM protected"),
      classifyOpenError("/tmp/f.mkv", "nope"),
    ];
    for (const e of kinds) {
      expect(titleKeyFor(e)).toMatch(/^title/);
      expect(hintKeyFor(e, "mac")).toMatch(/^hint/);
    }
  });
});

describe("detectPlatform", () => {
  it("reads the three platforms out of a user agent", () => {
    expect(detectPlatform("Mozilla/5.0 (Windows NT 10.0; Win64; x64)")).toBe("windows");
    expect(detectPlatform("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)")).toBe("mac");
    expect(detectPlatform("Mozilla/5.0 (X11; Linux x86_64)")).toBe("linux");
    expect(detectPlatform(undefined)).toBe("linux");
  });
});
