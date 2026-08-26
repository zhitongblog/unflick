//! Audio processing: a 10-band equaliser, loudness normalisation, and the
//! preamp that keeps the two from clipping.
//!
//! ## Why this isn't just more mpv properties
//!
//! The video filters (`brightness`, `contrast`, …) are single mpv properties
//! we set and forget. Audio isn't: mpv has no equaliser property. What it has
//! is `af`, a filter *chain*, and every knob here is a different filter in it.
//! So the chain has to be built from the complete state every time any part of
//! it changes — which is what `build_chain` does, and why the state lives here
//! rather than being read back out of mpv.
//!
//! ## Every change rebuilds the chain — and why `af-command` isn't used
//!
//! Replacing `af` re-initialises the audio filter graph. On paper there is a
//! cheaper path: ffmpeg's `equalizer` marks its gain option runtime-settable,
//! and mpv exposes `af-command <label> <option> <value>` to reach it, so a
//! slider drag could retune a filter that is already running.
//!
//! It does not work on the libmpv we bundle. Measured against a live file
//! with a real audio track: `af-command <our-label> g <value>` fails for every
//! spelling of the option (`g`, `gain`), with and without the trailing
//! `target` argument, with the filter written as `lavfi=[equalizer=…]` and as
//! a bare `equalizer=…`, and with labels both namespaced and plain. mpv
//! returns a bare "error running command" each time. `af-command all …`
//! *appears* to succeed, but so does `vf-command all …` against a chain with
//! no video filters at all, so that is not evidence of anything.
//!
//! So: one code path, always a rebuild. The cost lands on anything that
//! changes a value continuously, which in practice is a GUI slider — that
//! debounces instead. The labels are kept anyway: they make the chain legible
//! in mpv's `af` output, and they mark which filters are ours.

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

/// Band centre frequencies, in Hz. The ISO 10-band set every hardware and
/// software equaliser has used for decades — users recognise the shape of a
/// curve drawn on these, and presets from elsewhere transfer directly.
pub const BANDS: [u32; 10] = [31, 62, 125, 250, 500, 1000, 2000, 4000, 8000, 16000];

/// Gain limit per band, in dB. ±12 is the conventional range; beyond it the
/// filter stops being an equaliser and starts being a distortion effect.
pub const MAX_GAIN_DB: f64 = 12.0;

/// Preamp range, in dB. Wider on the negative side because its main job is
/// making headroom for boosted bands.
pub const MIN_PREAMP_DB: f64 = -20.0;
pub const MAX_PREAMP_DB: f64 = 12.0;

/// Label prefix for every filter we install. Namespaced so we can recognise
/// our own chain and never clobber an `af` the user set by other means.
const LABEL_PREFIX: &str = "unflick";

/// Q factor for each band. 1.0 gives roughly one-octave bands, which is the
/// right width for 10 bands spaced an octave apart: narrower leaves audible
/// gaps between them, wider makes adjacent sliders fight each other.
const BAND_Q: f64 = 1.0;

/// The complete audio-processing state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioSettings {
    /// Whether the equaliser is in the chain at all. Separate from "all bands
    /// are zero" so a user can A/B their curve without losing it.
    pub equalizer: bool,
    /// Per-band gain in dB, in `BANDS` order.
    pub bands: Vec<f64>,
    /// Applied before the equaliser, in dB.
    pub preamp: f64,
    /// Dynamic loudness normalisation — evens out quiet dialogue against loud
    /// action without the user riding the volume knob.
    pub normalize: bool,
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            equalizer: false,
            bands: vec![0.0; BANDS.len()],
            preamp: 0.0,
            normalize: false,
        }
    }
}

impl AudioSettings {
    /// Whether every band sits at zero. A flat enabled equaliser still gets
    /// built: it is a no-op on the signal, and keeping it installed means
    /// dragging a slider is a live command rather than a rebuild.
    pub fn is_flat(&self) -> bool {
        self.bands.iter().all(|g| g.abs() < f64::EPSILON)
    }

    /// Clamp everything into range and pad or truncate the band list.
    ///
    /// Called on load as well as on set: settings.json is user-editable and a
    /// hand-typed `"bands": [40, 40]` should produce a sane equaliser rather
    /// than a panic or a wall of distortion.
    pub fn normalized(mut self) -> Self {
        self.bands.resize(BANDS.len(), 0.0);
        for g in self.bands.iter_mut() {
            *g = clamp_finite(*g, -MAX_GAIN_DB, MAX_GAIN_DB);
        }
        self.preamp = clamp_finite(self.preamp, MIN_PREAMP_DB, MAX_PREAMP_DB);
        self
    }
}

/// NaN and infinity would reach mpv as the literal strings "NaN"/"inf" and
/// produce a filter that fails to initialise, taking the whole chain with it.
fn clamp_finite(v: f64, lo: f64, hi: f64) -> f64 {
    if v.is_nan() {
        return 0.0;
    }
    v.clamp(lo, hi)
}

/// Label for band `i`. Diagnostic only — see the note on `af-command` above.
pub fn band_label(i: usize) -> String {
    format!("{}_eq{}", LABEL_PREFIX, i)
}

/// Label for the preamp filter.
pub fn preamp_label() -> String {
    format!("{}_preamp", LABEL_PREFIX)
}

/// Build the mpv `af` chain for these settings.
///
/// Returns an empty string when nothing is enabled, which the caller turns
/// into `af clr` — leaving an all-zero equaliser installed would burn CPU on
/// every sample for no effect.
///
/// Order is deliberate: preamp, then equaliser, then normalisation. The preamp
/// makes headroom *before* the bands boost into it, and normalisation goes
/// last so it measures what the user will actually hear rather than the raw
/// signal.
pub fn build_chain(s: &AudioSettings) -> String {
    let mut parts: Vec<String> = Vec::new();

    if s.equalizer {
        // Only install a preamp when it does something. An extra filter in
        // the graph costs a copy of every sample.
        if s.preamp.abs() >= 0.01 {
            parts.push(format!(
                "@{}:lavfi=[volume={}dB]",
                preamp_label(),
                fmt(s.preamp)
            ));
        }
        for (i, freq) in BANDS.iter().enumerate() {
            let gain = s.bands.get(i).copied().unwrap_or(0.0);
            parts.push(format!(
                "@{}:equalizer=f={}:t=q:w={}:g={}",
                band_label(i),
                freq,
                fmt(BAND_Q),
                fmt(gain)
            ));
        }
    }

    if s.normalize {
        // `dynaudnorm` rather than `loudnorm`: loudnorm's single-pass mode
        // needs to look ahead over a long window, which adds latency you
        // notice when seeking. dynaudnorm works on a short sliding window and
        // is what other players use for a live "normalise volume" toggle.
        parts.push(format!("@{}_norm:lavfi=[dynaudnorm=f=250:g=15]", LABEL_PREFIX));
    }

    parts.join(",")
}

/// Format a float for mpv's filter syntax.
///
/// `to_string` on an f64 can emit `3.0000000000000004`; more importantly it
/// can emit exponent notation for small values, which ffmpeg's option parser
/// rejects. Fixed precision avoids both.
fn fmt(v: f64) -> String {
    let s = format!("{:.2}", v);
    // Trim a trailing `.00` / `.50` -> `.5` so chains stay readable when
    // logged or shown in `af` output.
    let s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    // "-0.00" trims down to "-0", and a negative zero gain is both ugly in a
    // filter string and a sign that something upstream is confused.
    if s.is_empty() || s == "-" || s == "-0" {
        "0".to_string()
    } else {
        s
    }
}

// --- Persistence -----------------------------------------------------------

/// settings.json key holding the audio state.
///
/// One nested object rather than five top-level keys: it is one coherent
/// thing, and it keeps `settings.json` readable for the people who edit it by
/// hand.
pub const SETTINGS_KEY: &str = "audio";

/// Read the stored audio state, falling back to defaults.
///
/// Never fails: a corrupt or hand-mangled `audio` object should leave the user
/// with a flat equaliser, not a player that won't start. `normalized` also
/// runs here, so a hand-typed out-of-range gain is clamped on the way in
/// rather than reaching mpv.
pub fn load() -> AudioSettings {
    let raw = match super::settings::get(SETTINGS_KEY) {
        Ok(Some(v)) => v,
        _ => return AudioSettings::default(),
    };
    serde_json::from_value::<AudioSettings>(raw)
        .map(AudioSettings::normalized)
        .unwrap_or_default()
}

/// Persist the audio state.
pub fn save(s: &AudioSettings) -> anyhow::Result<()> {
    super::settings::set(SETTINGS_KEY, serde_json::to_value(s)?)
}

// --- Presets ---------------------------------------------------------------

/// A named curve. Kept small on purpose: a list of forty presets is a list
/// nobody reads. Each of these answers a complaint someone actually has.
#[derive(Debug, Clone, Serialize)]
pub struct Preset {
    pub name: &'static str,
    pub description: &'static str,
    pub bands: [f64; 10],
}

pub const PRESETS: &[Preset] = &[
    Preset {
        name: "flat",
        description: "No adjustment",
        bands: [0.0; 10],
    },
    Preset {
        name: "speech",
        description: "Lifts dialogue out of a loud mix",
        bands: [-4.0, -3.0, -1.0, 2.0, 4.0, 4.0, 3.0, 1.0, -1.0, -2.0],
    },
    Preset {
        name: "bass",
        description: "More low end",
        bands: [6.0, 5.0, 4.0, 2.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    },
    Preset {
        name: "treble",
        description: "More detail up top",
        bands: [0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 2.0, 4.0, 5.0, 6.0],
    },
    Preset {
        name: "night",
        description: "Tames explosions, keeps voices — pair with normalize",
        bands: [-6.0, -5.0, -3.0, 0.0, 3.0, 3.0, 2.0, 0.0, -2.0, -4.0],
    },
    Preset {
        name: "headphones",
        description: "Compensates for typical headphone dip",
        bands: [3.0, 2.0, 0.0, -1.0, -2.0, -1.0, 0.0, 2.0, 3.0, 2.0],
    },
];

/// Look up a preset by name, case-insensitively.
pub fn preset(name: &str) -> Result<&'static Preset> {
    let want = name.trim().to_ascii_lowercase();
    PRESETS
        .iter()
        .find(|p| p.name == want)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "unknown preset: {} (expected {})",
                name,
                PRESETS
                    .iter()
                    .map(|p| p.name)
                    .collect::<Vec<_>>()
                    .join(" | ")
            )
        })
}

/// Parse a band index, rejecting the off-by-one that `--band 10` invites.
pub fn parse_band(index: i64) -> Result<usize> {
    if index < 0 || index as usize >= BANDS.len() {
        bail!(
            "band must be 0-{} ({} Hz to {} Hz)",
            BANDS.len() - 1,
            BANDS[0],
            BANDS[BANDS.len() - 1]
        );
    }
    Ok(index as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_enabled_builds_an_empty_chain() {
        // Empty means "clear the chain", not "install a no-op" - an all-zero
        // equaliser would still cost a pass over every sample.
        assert_eq!(build_chain(&AudioSettings::default()), "");
    }

    #[test]
    fn enabled_equalizer_installs_one_labelled_filter_per_band() {
        let s = AudioSettings {
            equalizer: true,
            ..Default::default()
        };
        let chain = build_chain(&s);
        for i in 0..BANDS.len() {
            assert!(
                chain.contains(&format!("@{}:", band_label(i))),
                "band {} missing from {}",
                i,
                chain
            );
        }
        // A flat equaliser stays installed: keeping it there is what makes a
        // slider drag a live command instead of a rebuild.
        assert_eq!(chain.matches("equalizer=").count(), BANDS.len());
    }

    #[test]
    fn each_band_carries_its_own_frequency_and_gain() {
        let mut s = AudioSettings {
            equalizer: true,
            ..Default::default()
        };
        s.bands[0] = 6.0;
        s.bands[9] = -3.5;
        let chain = build_chain(&s);

        assert!(chain.contains("equalizer=f=31:t=q:w=1:g=6"), "{}", chain);
        assert!(chain.contains("equalizer=f=16000:t=q:w=1:g=-3.5"), "{}", chain);
    }

    #[test]
    fn preamp_is_omitted_when_it_would_do_nothing() {
        let s = AudioSettings {
            equalizer: true,
            ..Default::default()
        };
        assert!(!build_chain(&s).contains("volume="));

        let s = AudioSettings {
            equalizer: true,
            preamp: -3.0,
            ..Default::default()
        };
        let chain = build_chain(&s);
        assert!(chain.contains("volume=-3dB"), "{}", chain);
        // Headroom has to come before the boost it makes room for.
        assert!(
            chain.find("volume=").unwrap() < chain.find("equalizer=").unwrap(),
            "preamp must precede the bands: {}",
            chain
        );
    }

    #[test]
    fn normalization_goes_last_so_it_measures_the_processed_signal() {
        let s = AudioSettings {
            equalizer: true,
            normalize: true,
            ..Default::default()
        };
        let chain = build_chain(&s);
        assert!(
            chain.find("dynaudnorm").unwrap() > chain.rfind("equalizer=").unwrap(),
            "{}",
            chain
        );
    }

    #[test]
    fn normalization_works_without_the_equalizer() {
        let s = AudioSettings {
            normalize: true,
            ..Default::default()
        };
        let chain = build_chain(&s);
        assert!(chain.contains("dynaudnorm"));
        assert!(!chain.contains("equalizer="));
    }

    #[test]
    fn disabled_equalizer_drops_out_even_with_a_curve_set() {
        // The A/B case: turning it off must not discard the bands, but must
        // take them out of the chain.
        let mut s = AudioSettings {
            equalizer: false,
            ..Default::default()
        };
        s.bands[3] = 8.0;
        assert_eq!(build_chain(&s), "");
        assert_eq!(s.bands[3], 8.0);
    }

    #[test]
    fn normalized_clamps_and_resizes() {
        let s = AudioSettings {
            equalizer: true,
            bands: vec![99.0, -99.0],
            preamp: 500.0,
            normalize: false,
        }
        .normalized();

        assert_eq!(s.bands.len(), BANDS.len());
        assert_eq!(s.bands[0], MAX_GAIN_DB);
        assert_eq!(s.bands[1], -MAX_GAIN_DB);
        assert_eq!(s.bands[2], 0.0, "missing bands should pad with flat");
        assert_eq!(s.preamp, MAX_PREAMP_DB);
    }

    #[test]
    fn normalized_neutralises_nan() {
        // A NaN would reach mpv as "NaN" and kill the whole filter graph,
        // silencing audio rather than just misconfiguring one band.
        let s = AudioSettings {
            bands: vec![f64::NAN; 10],
            preamp: f64::INFINITY,
            ..Default::default()
        }
        .normalized();
        assert!(s.bands.iter().all(|g| *g == 0.0));
        assert_eq!(s.preamp, MAX_PREAMP_DB);
    }

    #[test]
    fn chain_never_contains_exponent_notation() {
        // ffmpeg's option parser rejects `1e-7`; Rust's float formatting
        // reaches for it readily.
        let mut s = AudioSettings {
            equalizer: true,
            preamp: 0.0000001,
            ..Default::default()
        };
        s.bands[0] = 0.0000001;
        let chain = build_chain(&s);
        // Not a bare `contains('e')` - "equalizer" is full of them. Exponent
        // notation is specifically a digit, then e/E, then a sign or digit.
        let bytes: Vec<char> = chain.chars().collect();
        for w in bytes.windows(3) {
            let exponent = w[0].is_ascii_digit()
                && (w[1] == 'e' || w[1] == 'E')
                && (w[2].is_ascii_digit() || w[2] == '-' || w[2] == '+');
            assert!(!exponent, "exponent notation in {}", chain);
        }
    }

    #[test]
    fn fmt_trims_without_mangling() {
        assert_eq!(fmt(0.0), "0");
        assert_eq!(fmt(-0.0), "0");
        assert_eq!(fmt(3.0), "3");
        assert_eq!(fmt(3.5), "3.5");
        assert_eq!(fmt(-3.25), "-3.25");
        assert_eq!(fmt(12.0), "12");
    }

    #[test]
    fn is_flat_tracks_the_bands_only() {
        let mut s = AudioSettings::default();
        assert!(s.is_flat());
        s.preamp = -6.0;
        assert!(s.is_flat(), "preamp is not part of the curve");
        s.bands[5] = 0.5;
        assert!(!s.is_flat());
    }

    #[test]
    fn a_mangled_stored_object_falls_back_to_flat() {
        // The point is that nothing here panics or propagates: a bad settings
        // file must not stop the player from starting.
        let junk = serde_json::json!({"bands": "not an array", "equalizer": 3});
        let parsed = serde_json::from_value::<AudioSettings>(junk)
            .map(AudioSettings::normalized)
            .unwrap_or_default();
        assert_eq!(parsed, AudioSettings::default());
    }

    #[test]
    fn stored_settings_round_trip() {
        let mut s = AudioSettings {
            equalizer: true,
            preamp: -3.0,
            normalize: true,
            ..Default::default()
        };
        s.bands[2] = 4.5;

        let back: AudioSettings =
            serde_json::from_value(serde_json::to_value(&s).unwrap()).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn stored_out_of_range_values_are_clamped_on_load() {
        let raw = serde_json::json!({
            "equalizer": true,
            "bands": [99.0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            "preamp": -900.0,
            "normalize": false
        });
        let parsed = serde_json::from_value::<AudioSettings>(raw)
            .map(AudioSettings::normalized)
            .unwrap();
        assert_eq!(parsed.bands[0], MAX_GAIN_DB);
        assert_eq!(parsed.preamp, MIN_PREAMP_DB);
    }

    #[test]
    fn presets_are_all_well_formed() {
        assert!(preset("flat").unwrap().bands.iter().all(|g| *g == 0.0));
        // Every preset must fit the range it will be clamped to, or the
        // curve a user sees is not the curve we shipped.
        for p in PRESETS {
            for g in p.bands {
                assert!(
                    g.abs() <= MAX_GAIN_DB,
                    "preset {} exceeds the gain range",
                    p.name
                );
            }
            assert!(!p.description.trim().is_empty(), "{} has no description", p.name);
        }
    }

    #[test]
    fn preset_lookup_is_case_insensitive_and_names_the_alternatives() {
        assert_eq!(preset("SPEECH").unwrap().name, "speech");
        assert_eq!(preset("  bass  ").unwrap().name, "bass");
        let err = preset("loud").unwrap_err().to_string();
        assert!(err.contains("speech"), "got: {}", err);
    }

    #[test]
    fn band_index_is_bounds_checked() {
        assert_eq!(parse_band(0).unwrap(), 0);
        assert_eq!(parse_band(9).unwrap(), 9);
        assert!(parse_band(10).is_err(), "10 bands means indices 0-9");
        assert!(parse_band(-1).is_err());
    }
}
