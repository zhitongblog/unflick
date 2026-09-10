//! Bilingual subtitles: the original and a translation on screen at once.
//!
//! The audience this exists for is a Chinese viewer watching a foreign film
//! who wants to read the translation and hear the original with its own text
//! underneath. Everything needed to *produce* the second file already
//! existed — whisper, `subtitle translate`, OpenSubtitles — and nothing
//! could show two of them together.
//!
//! ## What mpv actually does
//!
//! Measured against the bundled libmpv (0.41), not assumed:
//!
//!   * `secondary-sid` renders a second track. `sub-text` and
//!     `secondary-sub-text` return different lines at the same instant, and
//!     a screenshot shows two bands of ink. Merging the two files into one
//!     styled .srt — the fallback this design was allowed to reach for — is
//!     not needed.
//!   * mpv does **not** stack them. With `secondary-sub-pos` equal to
//!     `sub-pos` the frame collapses to a single band: one line hides the
//!     other. Positioning is a requirement, not a nicety.
//!   * Styling is shared. There is no `secondary-sub-scale` / `-color` /
//!     `-border-size`; only position, delay, visibility and ass-override are
//!     separate. So the two lines are always the same size, and the gap
//!     between them has to scale with `sub-scale`.
//!   * A file load resets `secondary-sid` to `no` and drops external subs.
//!     A remembered preference that isn't re-armed per file is a setting
//!     that does nothing — hence [`after_play_hooks`].
//!
//! ## Where the state lives
//!
//! In mpv, not in a struct here. "Bilingual is on" means `secondary-sid`
//! reads back as a real track id. The property round-trips honestly once the
//! writes in `Player::set_bilingual` are verified, so a mirror would only be
//! a second source of truth to go stale on the next `loadfile`. What *is*
//! persisted is the user's answer to "do I want this?", which no property
//! can hold across files.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{bail, Result};
use serde_json::{json, Value};

use super::player::Player;
use super::settings;
use super::types::SubtitleTrack;

/// The settings.json key. An object, so the layout travels with the switch.
const SETTINGS_KEY: &str = "subtitle_bilingual";

/// One rendered subtitle line measures about 4.2% of the frame height per
/// unit of `sub-scale` — the same figure at 360p and 720p, so this is
/// resolution-independent. 5.5 leaves a visible gap without wasting frame.
const LINE_HEIGHT_PERCENT: f64 = 5.5;

/// How long to wait for a second subtitle track to turn up after a play.
///
/// mpv's own `sub-auto`, the OpenSubtitles lookup in `auto_subs` and the
/// GUI's sidecar attach all feed the track list, and all three land after
/// the file does. A bound rather than a guarantee: an unbounded watcher
/// would leak a thread per play.
const REARM_WINDOW: Duration = Duration::from_secs(8);
const REARM_POLL: Duration = Duration::from_millis(400);

// ─── Layout ───────────────────────────────────────────────────────────────

/// Where the second line goes.
///
/// Two values on purpose. Which language ends up on top is already
/// expressible by choosing which track is primary, so an ordering knob would
/// be a third way to say the same thing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// Just above the first line, one line height clear of it.
    Stacked,
    /// At the top of the frame — mpv's own behaviour, for people who want
    /// the original out of the way.
    Top,
}

impl Layout {
    pub fn parse(raw: &str) -> Result<Layout> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "stacked" => Ok(Layout::Stacked),
            "top" => Ok(Layout::Top),
            other => bail!("unknown layout: {} (expected stacked | top)", other),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Layout::Stacked => "stacked",
            Layout::Top => "top",
        }
    }
}

/// Where the second line sits, given where the first one is.
///
/// A negative result means the first line is already at the top of the
/// frame. Clamping to 0 there would put both lines on the same row — the
/// exact collapse mpv produces when the two positions match — so the second
/// line goes *below* the first instead.
pub fn secondary_pos(sub_pos: f64, sub_scale: f64, layout: Layout) -> f64 {
    match layout {
        Layout::Top => 0.0,
        Layout::Stacked => {
            let gap = LINE_HEIGHT_PERCENT * sub_scale.clamp(0.1, 10.0);
            let above = sub_pos - gap;
            if above >= 0.0 {
                above
            } else {
                (sub_pos + gap).min(150.0)
            }
        }
    }
}

// ─── Persistence ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Prefs {
    pub enabled: bool,
    pub layout: Layout,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            enabled: false,
            layout: Layout::Stacked,
        }
    }
}

/// Read the preference out of a settings blob, falling back per field.
///
/// Per field rather than all-or-nothing so a hand-edited settings.json with
/// one bad value still yields the other, and never panics.
pub fn prefs_from(all: &Value) -> Prefs {
    let mut out = Prefs::default();
    let Some(obj) = all.get(SETTINGS_KEY) else {
        return out;
    };
    if let Some(v) = obj.get("enabled").and_then(Value::as_bool) {
        out.enabled = v;
    }
    if let Some(v) = obj.get("layout").and_then(Value::as_str) {
        if let Ok(l) = Layout::parse(v) {
            out.layout = l;
        }
    }
    out
}

pub fn read_prefs() -> Prefs {
    match settings::read_all() {
        Ok(all) => prefs_from(&all),
        Err(_) => Prefs::default(),
    }
}

/// Written with `settings::merge`, never `write_all`: a whole-object write
/// drops every key the writer doesn't model, and this one is written from
/// the CLI while the settings panel holds its own idea of the file.
fn write_prefs(prefs: Prefs) -> Result<()> {
    settings::merge(&json!({
        SETTINGS_KEY: {
            "enabled": prefs.enabled,
            "layout": prefs.layout.as_str(),
        }
    }))
}

// ─── Language matching ────────────────────────────────────────────────────

/// The two-letter base of a language tag, with the ISO 639-2 spellings that
/// turn up in Matroska track headers folded onto the 639-1 ones the UI uses.
/// Without this an embedded `chi` track and a `zh-CN` locale never match.
fn base_lang(raw: &str) -> Option<String> {
    let head = raw
        .trim()
        .to_ascii_lowercase()
        .split(['-', '_'])
        .next()
        .unwrap_or("")
        .to_string();
    if head.is_empty() || head == "und" {
        return None;
    }
    Some(match head.as_str() {
        "zho" | "chi" | "cmn" | "yue" => "zh".into(),
        "eng" => "en".into(),
        "jpn" => "ja".into(),
        "kor" => "ko".into(),
        "deu" | "ger" => "de".into(),
        "fra" | "fre" => "fr".into(),
        "spa" => "es".into(),
        "rus" => "ru".into(),
        "por" => "pt".into(),
        "ita" => "it".into(),
        _ => head,
    })
}

fn track_lang(track: &SubtitleTrack) -> Option<String> {
    track.lang.as_deref().and_then(base_lang)
}

/// Whether two tracks are in different languages.
///
/// An unknown language is never evidence of a match: a bare `.srt` sidecar
/// carries no language at all, and treating two unknowns as "the same" would
/// rule out exactly the pairing this feature exists for.
fn langs_differ(a: Option<&String>, b: Option<&String>) -> bool {
    match (a, b) {
        (Some(x), Some(y)) => x != y,
        _ => true,
    }
}

// ─── Choosing the two tracks ──────────────────────────────────────────────

/// Pick the two lines when the caller named neither.
///
/// The track in the user's own language becomes the *primary* — the bottom
/// line, where the eye rests. For the audience this was built for (a Chinese
/// viewer, an English film, a Chinese .srt) that puts 中文 underneath and the
/// original above it, which is the layout people ask for. `--primary` exists
/// to override it.
pub fn pick(
    tracks: &[SubtitleTrack],
    selected: Option<i64>,
    locale: &str,
    languages: &[String],
) -> Result<(i64, i64)> {
    if tracks.len() < 2 {
        bail!(
            "bilingual needs two subtitle tracks and this file has {} - add the \
             translation with `unflick subtitle load <file>`, or make one with \
             `unflick subtitle translate`",
            tracks.len()
        );
    }

    let want = base_lang(locale);
    let mine: Vec<&SubtitleTrack> = tracks
        .iter()
        .filter(|t| want.is_some() && track_lang(t) == want)
        .collect();

    // Exactly one match is a clear answer. Zero or several is not, and
    // guessing there would move the line the user is already reading.
    let primary_id = if mine.len() == 1 {
        mine[0].id
    } else {
        selected
            .filter(|id| tracks.iter().any(|t| t.id == *id))
            .unwrap_or(tracks[0].id)
    };
    let primary = tracks.iter().find(|t| t.id == primary_id).unwrap();
    let primary_lang = track_lang(primary);

    let wanted: HashSet<String> = languages.iter().filter_map(|l| base_lang(l)).collect();

    let secondary = tracks
        .iter()
        .filter(|t| t.id != primary_id)
        .max_by_key(|t| {
            let lang = track_lang(t);
            let differs = langs_differ(lang.as_ref(), primary_lang.as_ref());
            let asked_for = lang.as_ref().is_some_and(|l| wanted.contains(l)) && differs;
            let score = 4 * i32::from(asked_for)
                + 2 * i32::from(differs)
                // The translation you just generated is an external file,
                // and so is every sidecar. When language says nothing, that
                // is the only signal left.
                + i32::from(t.external_file.is_some());
            // Lowest id breaks a tie: `max_by_key` keeps the last maximum,
            // so the id is negated.
            (score, -t.id)
        })
        .expect("at least one track besides the primary");

    Ok((primary_id, secondary.id))
}

// ─── Naming a track ───────────────────────────────────────────────────────

/// What to call a track in a message.
///
/// The filename wins for an external track: mpv titles a sidecar it
/// auto-loaded with whatever the name has beyond the video's stem, so
/// `subtitled.srt` next to `subtitled.mp4` comes back titled "srt" — which
/// tells the user nothing about which of their two files it is.
fn label(track: &SubtitleTrack) -> String {
    if let Some(file) = track.external_file.as_deref() {
        if let Some(name) = file.rsplit(['/', '\\']).next().filter(|n| !n.is_empty()) {
            return name.to_string();
        }
    }
    if let Some(title) = track.title.as_deref().filter(|t| !t.is_empty()) {
        return title.to_string();
    }
    format!("track {}", track.id)
}

fn track_json(track: &SubtitleTrack) -> Value {
    json!({
        "id": track.id,
        "label": label(track),
        "lang": track.lang,
    })
}

fn normalize(path: &str) -> String {
    path.replace('\\', "/").to_lowercase()
}

/// A language guessed from a filename like `film.zh-CN.srt`, so a loaded
/// sidecar arrives with something the auto-pick can score on. Nothing is
/// guessed from a bare `film.srt` — a wrong language is worse than none.
fn lang_from_filename(name: &str) -> Option<String> {
    let stem = name.rsplit_once('.').map(|(head, _)| head).unwrap_or(name);
    let tag = stem.rsplit_once('.').map(|(_, tail)| tail)?;
    let (head, region) = match tag.split_once('-') {
        Some((h, r)) => (h, Some(r)),
        None => (tag, None),
    };
    // Letters only, and only where a language code could be. `1999` in
    // `the.matrix.1999.srt` is the case this rules out.
    let ok = (2..=3).contains(&head.len())
        && head.chars().all(|c| c.is_ascii_alphabetic())
        && region.is_none_or(|r| {
            (2..=4).contains(&r.len()) && r.chars().all(|c| c.is_ascii_alphanumeric())
        });
    ok.then(|| tag.to_string())
}

/// Resolve a track named by the caller: an id from `subtitle_list`, or a
/// path to a subtitle file, which is loaded if it is not already.
///
/// Loading is deduplicated against the paths mpv already has, because
/// `sub-add` happily adds the same file twice and the second copy is
/// indistinguishable from the first in the menu.
pub fn resolve_track(player: &Player, spec: &Value) -> Result<i64> {
    if let Some(id) = spec.as_i64() {
        return Ok(id);
    }
    let Some(raw) = spec.as_str().map(str::trim).filter(|s| !s.is_empty()) else {
        bail!("a subtitle track must be a track id from `subtitle list` or a path to a subtitle file");
    };
    if let Ok(id) = raw.parse::<i64>() {
        return Ok(id);
    }

    let before = player.subtitle_list();
    let wanted = normalize(raw);
    if let Some(existing) = before
        .iter()
        .find(|t| t.external_file.as_deref().map(normalize) == Some(wanted.clone()))
    {
        return Ok(existing.id);
    }

    let name = raw.rsplit(['/', '\\']).next().unwrap_or(raw);
    let title = name.rsplit_once('.').map(|(head, _)| head).unwrap_or(name);
    let lang = lang_from_filename(name);
    player.subtitle_add(raw, false, Some(title), lang.as_deref())?;

    let known: HashSet<i64> = before.iter().map(|t| t.id).collect();
    player
        .subtitle_list()
        .iter()
        .filter(|t| !known.contains(&t.id))
        .map(|t| t.id)
        .max()
        .ok_or_else(|| {
            anyhow::anyhow!(
                "mpv accepted {} but produced no subtitle track from it - the file may be empty \
                 or in a format it cannot read",
                raw
            )
        })
}

// ─── Applying it ──────────────────────────────────────────────────────────

/// Turn bilingual on. Returns the human-readable message and the state.
pub fn enable(
    player: &Player,
    primary: Option<i64>,
    secondary: Option<i64>,
    layout: Layout,
) -> Result<(String, Value)> {
    let tracks = player.subtitle_list();

    // Check what the caller named before doing anything else, so a bad id is
    // reported as a bad id rather than as whatever the auto-pick made of the
    // rest. mpv answers a write of a nonexistent id with success and leaves
    // the property alone, which would let this claim a second line that is
    // not on screen.
    for id in [primary, secondary].into_iter().flatten() {
        if !tracks.iter().any(|t| t.id == id) {
            bail!(
                "there is no subtitle track {} - run `unflick subtitle list` for the ids \
                 this file has",
                id
            );
        }
    }

    let (primary_id, secondary_id) = match (primary, secondary) {
        (Some(p), Some(s)) => (p, s),
        _ => {
            let selected = tracks.iter().find(|t| t.selected).map(|t| t.id);
            let (auto_primary, auto_secondary) = pick(
                &tracks,
                selected,
                &super::i18n::read_locale_from_settings(),
                &super::url_post_play::read_settings_snapshot().subtitle_languages,
            )?;
            match (primary, secondary) {
                // One named, one picked. Naming the second line and having
                // the auto-pick hand back the same track for the first would
                // be a silent no-op, so fall back to what is selected.
                (Some(p), None) => {
                    let other = if auto_secondary == p { auto_primary } else { auto_secondary };
                    (p, other)
                }
                (None, Some(s)) => {
                    let other = if auto_primary == s { auto_secondary } else { auto_primary };
                    (other, s)
                }
                _ => (auto_primary, auto_secondary),
            }
        }
    };

    player.set_bilingual(primary_id, secondary_id)?;

    // Ask rather than assume: an older libmpv has `secondary-sid` but no
    // `secondary-sub-pos`, and quietly not stacking would look like the
    // second line had eaten the first.
    let (effective, note) = if layout == Layout::Stacked && !player.secondary_pos_supported() {
        (Layout::Top, " (layout: top — this libmpv has no secondary-sub-pos)")
    } else {
        (layout, "")
    };
    apply_layout(player, effective);

    // The two lines are the same dialogue; start them on the same offset.
    let _ = player.set_sub_delay(player.sub_delay());

    write_prefs(Prefs { enabled: true, layout: effective })?;

    let state = state(player);
    let message = format!(
        "bilingual on: {} + {}{}",
        state["primary"]["label"].as_str().unwrap_or("?"),
        state["secondary"]["label"].as_str().unwrap_or("?"),
        note
    );
    Ok((message, state))
}

/// Turn bilingual off, leaving the first line playing.
pub fn disable(player: &Player) -> Result<(String, Value)> {
    player.clear_bilingual()?;
    write_prefs(Prefs { enabled: false, layout: read_prefs().layout })?;
    Ok(("bilingual off".to_string(), state(player)))
}

fn apply_layout(player: &Player, layout: Layout) {
    let style = player.subtitle_style();
    let pos = style["pos"].as_f64().unwrap_or(100.0);
    let scale = style["scale"].as_f64().unwrap_or(1.0);
    let _ = player.set_secondary_sub_pos(secondary_pos(pos, scale, layout));
}

/// Re-apply the layout after the user moved or resized the subtitles.
/// A no-op when there is no second line, which is the common case — so it
/// costs one property read on the hot path and nothing else.
pub fn follow_style_change(player: &Player) {
    if player.secondary_subtitle().is_none() {
        return;
    }
    apply_layout(player, read_prefs().layout);
}

/// Everything "are the two lines right" needs answering: which tracks, where
/// they sit, and whether they are in sync. The delays are here because this
/// is the natural place to ask that question, and there is no generic
/// property read to ask it with.
pub fn state(player: &Player) -> Value {
    let tracks = player.subtitle_list();
    let primary = tracks.iter().find(|t| t.selected);
    let secondary = tracks.iter().find(|t| t.secondary);
    let style = player.subtitle_style();
    json!({
        "enabled": secondary.is_some(),
        "primary": primary.map(track_json),
        "secondary": secondary.map(track_json),
        "layout": read_prefs().layout.as_str(),
        "sub_pos": style["pos"].as_f64().unwrap_or(100.0),
        "secondary_sub_pos": player.secondary_sub_pos(),
        "delay": player.sub_delay(),
        "secondary_delay": player.secondary_sub_delay(),
    })
}

/// The whole `subtitle_bilingual` command, shared by the control server and
/// the Tauri command so the two can't drift.
///
/// Reading shape, the one `subtitle delay` established: no arguments at all
/// is a pure read and writes nothing. Naming tracks or a layout without
/// saying `enabled` is treated as switching it on — silently ignoring those
/// arguments would be the same class of lie as mpv's own no-op writes.
pub fn command(player: &Player, args: &Value) -> Result<(String, Value)> {
    let layout_arg = args.get("layout").and_then(Value::as_str);
    let layout = match layout_arg {
        Some(raw) => Layout::parse(raw)?,
        None => read_prefs().layout,
    };
    let named = |key: &str| args.get(key).filter(|v| !v.is_null()).cloned();
    let primary = named("primary");
    let secondary = named("secondary");
    let enabled = args.get("enabled").and_then(Value::as_bool);

    match enabled {
        Some(false) => disable(player),
        None if primary.is_none() && secondary.is_none() && layout_arg.is_none() => {
            let state = state(player);
            let message = if state["enabled"] == json!(true) {
                format!(
                    "bilingual on: {} + {}",
                    state["primary"]["label"].as_str().unwrap_or("?"),
                    state["secondary"]["label"].as_str().unwrap_or("?")
                )
            } else {
                "bilingual off".to_string()
            };
            Ok((message, state))
        }
        _ => {
            let primary = primary.map(|v| resolve_track(player, &v)).transpose()?;
            let secondary = secondary.map(|v| resolve_track(player, &v)).transpose()?;
            enable(player, primary, secondary, layout)
        }
    }
}

// ─── Re-arming on the next file ───────────────────────────────────────────

/// Put the second line back after a file load, if the user asked for one.
///
/// mpv resets `secondary-sid` and drops external subtitles on every
/// `loadfile`, so without this the persisted preference would be a setting
/// that does nothing from the second file onward. Returns immediately; the
/// waiting happens on its own thread, like the other post-play hooks.
pub fn after_play_hooks(player: Arc<Player>) {
    let prefs = read_prefs();
    if !prefs.enabled {
        return;
    }
    let layout = prefs.layout;
    // The path mpv actually has open, not the one the caller named: a URL
    // reaches `play` as a page address and mpv as a resolved stream, and
    // comparing the two would switch the re-arm off for every stream.
    let Some(path) = player.status().file else {
        return;
    };
    std::thread::spawn(move || {
        let deadline = Instant::now() + REARM_WINDOW;
        loop {
            std::thread::sleep(REARM_POLL);
            // The user may have moved on while we waited. Re-arming for the
            // previous file would put the wrong translation on screen.
            if !super::auto_subs::still_playing(&player, &path) {
                return;
            }
            if player.subtitle_list().len() >= 2 {
                break;
            }
            if Instant::now() >= deadline {
                // Not a failure worth interrupting playback over: one file
                // simply came up with a single track.
                return;
            }
        }
        if let Err(e) = enable(&player, None, None, layout) {
            eprintln!("[bilingual] could not re-arm for {}: {}", path, e);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(id: i64, lang: Option<&str>, external: bool, selected: bool) -> SubtitleTrack {
        SubtitleTrack {
            id,
            title: None,
            lang: lang.map(String::from),
            external_file: external.then(|| format!("/tmp/sub{}.srt", id)),
            selected,
            secondary: false,
        }
    }

    #[test]
    fn picks_the_users_language_for_the_bottom_line() {
        // A Chinese viewer, an English film, a Chinese .srt. The translation
        // belongs on the bottom line even though the English track is the
        // one currently selected.
        let tracks = vec![track(1, Some("eng"), false, true), track(2, Some("zh"), true, false)];
        let (primary, secondary) = pick(&tracks, Some(1), "zh-CN", &["zh-CN".into()]).unwrap();
        assert_eq!(primary, 2, "the zh track should be the bottom line");
        assert_eq!(secondary, 1);
    }

    #[test]
    fn prefers_a_language_the_user_asked_for_as_the_second_line() {
        let tracks = vec![
            track(1, Some("en"), false, true),
            track(2, Some("en"), false, false),
            track(3, Some("zh"), false, false),
        ];
        let languages = vec!["zh-CN".to_string(), "en".to_string()];
        let (primary, secondary) = pick(&tracks, Some(1), "en", &languages).unwrap();
        assert_eq!(primary, 1, "two en tracks match the locale, so nothing moves");
        assert_eq!(secondary, 3);
    }

    #[test]
    fn prefers_an_external_file_when_languages_say_nothing() {
        // Plain .srt sidecars carry no language at all. The external one is
        // the translation that was just generated; the embedded one came
        // with the film.
        let tracks = vec![
            track(1, None, false, true),
            track(2, None, false, false),
            track(3, None, true, false),
        ];
        let (primary, secondary) = pick(&tracks, Some(1), "en", &[]).unwrap();
        assert_eq!(primary, 1);
        assert_eq!(secondary, 3);
    }

    #[test]
    fn a_tie_goes_to_the_lower_track_id() {
        let tracks = vec![
            track(1, None, false, true),
            track(2, None, true, false),
            track(3, None, true, false),
        ];
        let (_, secondary) = pick(&tracks, Some(1), "en", &[]).unwrap();
        assert_eq!(secondary, 2);
    }

    #[test]
    fn never_returns_the_same_track_twice() {
        // The invariant mpv violates silently: a write putting one track on
        // both lines is answered with success and ignored.
        let all = vec![
            track(1, Some("en"), false, false),
            track(2, Some("zh"), true, false),
            track(3, None, false, false),
        ];
        for len in 2..=3 {
            for selected in [None, Some(1), Some(2), Some(3), Some(99)] {
                for locale in ["en", "zh-CN", "de"] {
                    let tracks: Vec<SubtitleTrack> = all[..len].to_vec();
                    let (p, s) = pick(&tracks, selected, locale, &["zh-CN".into()]).unwrap();
                    assert_ne!(p, s, "len={len} selected={selected:?} locale={locale}");
                }
            }
        }
    }

    #[test]
    fn refuses_when_only_one_track_is_loaded() {
        let tracks = vec![track(1, Some("en"), false, true)];
        let err = pick(&tracks, Some(1), "zh-CN", &[]).unwrap_err().to_string();
        assert!(
            err.contains("subtitle load"),
            "the message has to say what to do next: {err}"
        );
    }

    #[test]
    fn stacked_layout_clears_a_full_line() {
        assert_eq!(secondary_pos(100.0, 1.0, Layout::Stacked), 94.5);
        assert_eq!(secondary_pos(100.0, 1.5, Layout::Stacked), 91.75);
        // Each gap is at least one measured line height (4.2% × scale).
        assert!(100.0 - secondary_pos(100.0, 1.0, Layout::Stacked) >= 4.2);
        assert!(100.0 - secondary_pos(100.0, 1.5, Layout::Stacked) >= 4.2 * 1.5);
    }

    #[test]
    fn a_primary_near_the_top_pushes_the_translation_below_it() {
        // Clamping to 0 would stack both lines on the same row — the
        // collapse measured when the two positions match.
        assert_eq!(secondary_pos(2.0, 1.0, Layout::Stacked), 7.5);
    }

    #[test]
    fn top_layout_is_mpvs_own_top() {
        assert_eq!(secondary_pos(100.0, 1.0, Layout::Top), 0.0);
        assert_eq!(secondary_pos(40.0, 2.0, Layout::Top), 0.0);
    }

    #[test]
    fn layout_parse_rejects_junk_and_says_what_is_valid() {
        assert_eq!(Layout::parse("stacked").unwrap(), Layout::Stacked);
        assert_eq!(Layout::parse(" TOP ").unwrap(), Layout::Top);
        let err = Layout::parse("sideways").unwrap_err().to_string();
        assert!(err.contains("stacked"), "{err}");
        assert!(err.contains("top"), "{err}");
    }

    #[test]
    fn a_missing_or_malformed_settings_key_reads_as_off() {
        assert_eq!(prefs_from(&json!({})), Prefs::default());
        assert_eq!(prefs_from(&json!({ "subtitle_bilingual": 7 })), Prefs::default());
        assert_eq!(
            prefs_from(&json!({ "subtitle_bilingual": { "layout": "sideways" } })),
            Prefs::default()
        );
        assert_eq!(
            prefs_from(&json!({ "subtitle_bilingual": { "enabled": true, "layout": "top" } })),
            Prefs { enabled: true, layout: Layout::Top }
        );
        // One bad field must not take the other down with it.
        assert_eq!(
            prefs_from(&json!({ "subtitle_bilingual": { "enabled": true, "layout": 3 } })),
            Prefs { enabled: true, layout: Layout::Stacked }
        );
    }

    #[test]
    fn three_letter_track_languages_match_two_letter_locales() {
        // Matroska headers say `chi`; the locale picker says `zh-CN`. Without
        // the fold the auto-pick would never see them as the same language.
        assert_eq!(base_lang("chi"), base_lang("zh-CN"));
        assert_eq!(base_lang("eng"), base_lang("en"));
        assert_eq!(base_lang("und"), None);
    }

    #[test]
    fn a_language_tagged_filename_names_its_track() {
        assert_eq!(lang_from_filename("film.zh-CN.srt").as_deref(), Some("zh-CN"));
        assert_eq!(lang_from_filename("film.en.srt").as_deref(), Some("en"));
        // A bare name guesses nothing: a wrong language is worse than none.
        assert_eq!(lang_from_filename("film.srt"), None);
        assert_eq!(lang_from_filename("the.matrix.1999.srt"), None);
    }
}
