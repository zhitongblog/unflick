import { describe, it, expect } from "vitest";
import {
  castFileLabel,
  castProgress,
  castView,
  isPaused,
  parseRenderers,
  parseSession,
  sameRenderer,
  seekTarget,
  type CastSnapshot,
  type Renderer,
} from "./cast";

const renderer = (over: Partial<Renderer> = {}): Renderer => ({
  name: "Living room TV",
  id: "uuid:65f1b2fb-dmr",
  control_url: "http://192.168.1.66:1214/AVTransport/control.xml",
  address: "192.168.1.66:1214",
  ...over,
});

const snapshot = (over: Partial<CastSnapshot> = {}): CastSnapshot => ({
  discovering: false,
  searched: false,
  renderers: [],
  session: null,
  ...over,
});

const session = (over: Partial<ReturnType<typeof parseSession>> = {}) => ({
  renderer: renderer(),
  file: "/Users/me/Films/bili.mp4",
  url: "http://192.168.1.63:52834/bili.mp4",
  position: 30,
  duration: 120,
  state: "PLAYING",
  ...over,
});

describe("castView", () => {
  it("shows the search while it is running", () => {
    expect(castView(snapshot({ discovering: true }))).toBe("searching");
  });

  // The whole point of the `searched` flag. An empty list before the first
  // search has come back is not "no televisions on this network", it is
  // "nobody has looked yet" — and telling someone to go wake their TV
  // because a request is still in flight is the kind of wrong advice that
  // sends them to the router.
  it("never claims nothing was found before something was looked for", () => {
    expect(castView(snapshot())).toBe("searching");
    expect(castView(snapshot({ searched: false, renderers: [] }))).toBe("searching");
    expect(castView(snapshot({ searched: true, renderers: [] }))).toBe("empty");
  });

  it("lists what answered", () => {
    expect(castView(snapshot({ searched: true, renderers: [renderer()] }))).toBe("renderers");
  });

  // A cast in progress outranks everything: re-running discovery underneath
  // must not swap the controls out from under someone mid-film.
  it("keeps the controls up while a cast is live", () => {
    expect(castView(snapshot({ session: session(), discovering: true }))).toBe("casting");
    expect(
      castView(snapshot({ session: session(), searched: true, renderers: [] })),
    ).toBe("casting");
  });
});

describe("parseRenderers", () => {
  it("takes a well-formed list", () => {
    expect(parseRenderers([renderer(), renderer({ id: "uuid:b", name: "Bedroom" })])).toHaveLength(2);
  });

  it("survives anything else the backend could hand back", () => {
    expect(parseRenderers(null)).toEqual([]);
    expect(parseRenderers({})).toEqual([]);
    expect(parseRenderers([null, 7, "TV", { name: "no id" }])).toEqual([]);
  });
});

describe("parseSession", () => {
  it("reads a live cast", () => {
    const s = parseSession(session());
    expect(s?.renderer.name).toBe("Living room TV");
    expect(s?.position).toBe(30);
  });

  // `cast status` answers `{"success": true, "data": null}` when nothing is
  // casting. That null is the answer, not a failure to parse one.
  it("treats 'not casting' as an answer", () => {
    expect(parseSession(null)).toBeNull();
    expect(parseSession(undefined)).toBeNull();
  });

  it("fills in fields a renderer declined to report", () => {
    const s = parseSession({ renderer: renderer() });
    expect(s?.position).toBe(0);
    expect(s?.duration).toBe(0);
    expect(s?.state).toBe("UNKNOWN");
  });
});

describe("isPaused", () => {
  // AVTransport's vocabulary, not ours: a renderer says PAUSED_PLAYBACK.
  it("knows the renderer's word for it", () => {
    expect(isPaused("PAUSED_PLAYBACK")).toBe(true);
    expect(isPaused("paused_recording")).toBe(true);
    expect(isPaused("PLAYING")).toBe(false);
    expect(isPaused("TRANSITIONING")).toBe(false);
    expect(isPaused("UNKNOWN")).toBe(false);
  });
});

describe("castProgress", () => {
  it("is the fraction through", () => {
    expect(castProgress({ position: 30, duration: 120 })).toBe(0.25);
  });

  // A live stream reports TrackDuration NOT_IMPLEMENTED, which arrives as
  // zero. A bar that divides by it fills with NaN and renders as nothing.
  it("copes with a stream that has no duration", () => {
    expect(castProgress({ position: 30, duration: 0 })).toBe(0);
    expect(castProgress({ position: 0, duration: 0 })).toBe(0);
  });

  it("clamps a renderer that overshoots its own duration", () => {
    expect(castProgress({ position: 130, duration: 120 })).toBe(1);
    expect(castProgress({ position: -5, duration: 120 })).toBe(0);
  });
});

describe("seekTarget", () => {
  it("turns a position along the bar into seconds", () => {
    expect(seekTarget(0.5, 120)).toBe(60);
    expect(seekTarget(0, 120)).toBe(0);
    expect(seekTarget(1, 120)).toBe(120);
  });

  // AVTransport carries whole seconds, so rounding here is what keeps the
  // bar honest: asking for 93.5 and being told 93 looks like a bug.
  it("rounds to what AVTransport can carry", () => {
    expect(seekTarget(0.7792, 120)).toBe(94);
  });

  it("refuses to seek past either end", () => {
    expect(seekTarget(2, 120)).toBe(120);
    expect(seekTarget(-1, 120)).toBe(0);
    expect(seekTarget(0.5, 0)).toBe(0);
  });
});

describe("castFileLabel", () => {
  it("names the film, not the path", () => {
    expect(castFileLabel("/Users/me/Films/bili.mp4")).toBe("bili");
    expect(castFileLabel("C:\\Films\\The Thing.mkv")).toBe("The Thing");
    expect(castFileLabel("bili.mp4")).toBe("bili");
  });
});

describe("sameRenderer", () => {
  // Two televisions of the same model answer with the same name. The id is
  // what stays unique, and it is what the backend matches on first.
  it("compares identity, not the label", () => {
    expect(sameRenderer(renderer(), renderer({ name: "renamed" }))).toBe(true);
    expect(sameRenderer(renderer(), renderer({ id: "uuid:other" }))).toBe(false);
    expect(sameRenderer(null, renderer())).toBe(false);
    expect(sameRenderer(renderer(), null)).toBe(false);
  });
});
