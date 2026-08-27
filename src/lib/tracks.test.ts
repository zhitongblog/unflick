import { describe, it, expect } from "vitest";
import { audioTrackLabel, type AudioTrack } from "./tracks";

const track = (over: Partial<AudioTrack> = {}): AudioTrack => ({
  id: 1,
  title: null,
  lang: null,
  codec: "aac",
  selected: false,
  ...over,
});

describe("audioTrackLabel", () => {
  it("prefers the embedded title", () => {
    expect(audioTrackLabel(track({ title: "Director commentary", lang: "en" })))
      .toBe("Director commentary");
  });

  it("falls back to the language", () => {
    expect(audioTrackLabel(track({ id: 2, lang: "jpn" }))).toBe("Track 2 (jpn)");
  });

  // The case that shipped broken: a track with no metadata at all still has
  // to be nameable, or the menu row reads "undefined" and cannot be chosen
  // with any confidence.
  it("always produces something selectable", () => {
    expect(audioTrackLabel(track({ id: 3 }))).toBe("Track 3");
    expect(audioTrackLabel(track({ id: 4, title: "" }))).toBe("Track 4");
  });
});
