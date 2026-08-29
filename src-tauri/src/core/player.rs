use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use anyhow::{anyhow, bail, Result};

use super::audio::{self, AudioSettings};
use super::disc;
use super::source;
use super::sponsorblock::Segment;
use super::types::{
    AbLoop, AudioTrack, Chapter, FileInfo, PlaybackState, PlayerStatus, SubtitleTrack,
};
use crate::mpv::ffi::{
    MPV_EVENT_END_FILE, MPV_EVENT_FILE_LOADED, MPV_EVENT_NONE, MPV_EVENT_START_FILE,
};
use crate::mpv::MpvHandle;

/// How long `play` waits for mpv to confirm the source actually opened.
///
/// Long enough for a file on a share across a slow LAN, short enough that a
/// caller pointed at a dead host gets an answer instead of hanging. A source
/// still opening when this runs out is reported as pending rather than
/// failed — it may well come up a second later.
const LOAD_TIMEOUT: Duration = Duration::from_secs(10);

/// What `play` learned about the source before it returned.
///
/// `loadfile` is asynchronous and never fails on its own, so without waiting
/// for mpv's verdict every call looks like a success — including a typo'd
/// path and an unreachable share. Callers need to be able to tell those
/// apart from a file that really is on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadOutcome {
    /// mpv reported the file open. There is picture (or sound) now.
    Loaded,
    /// Still opening when the deadline ran out. Usually a slow network
    /// source; it may still come up, so this is not an error.
    Pending,
}

/// Playback rate limits, as enforced by mpv itself.
pub const SPEED_MIN: f64 = 0.01;
pub const SPEED_MAX: f64 = 100.0;

/// Core player logic backed by libmpv.
pub struct Player {
    mpv: MpvHandle,
    /// Cache the last known file path since mpv may not have "path" available after stop.
    current_file: Mutex<Option<String>>,
    /// Equaliser / normalisation state.
    ///
    /// Held here rather than read back from mpv because mpv's `af` is a flat
    /// string: recovering "band 4 is at +3 dB" from it would mean parsing our
    /// own filter syntax back out, and any hand-edited `af` would then be
    /// misread as ours. This is the source of truth; mpv holds a projection
    /// of it. See `core::audio`.
    audio: Mutex<AudioSettings>,

    /// SponsorBlock segments for the currently loaded file. Cleared on stop.
    /// Driven by the auto-skip polling task; safe to mutate while the polling
    /// task reads.
    sponsor_segments: Mutex<Vec<Segment>>,
    /// Chapters synthesised for a file that has none of its own — derived
    /// from its transcript, or handed to us by an AI agent that read it.
    ///
    /// mpv can't be given chapters at runtime, so these live alongside it
    /// and `chapter_list` merges them in. Everything downstream (the CLI,
    /// the progress bar ticks, PgUp/PgDn) then works on a file that shipped
    /// without chapters. Cleared whenever the file changes, since they
    /// describe one specific recording.
    virtual_chapters: Mutex<Vec<Chapter>>,
}

impl Player {
    pub fn new() -> Result<Self> {
        let mpv = MpvHandle::new("null")?;
        Ok(Self {
            mpv,
            current_file: Mutex::new(None),
            audio: Mutex::new(audio::load()),
            sponsor_segments: Mutex::new(Vec::new()),
            virtual_chapters: Mutex::new(Vec::new()),
        })
    }

    /// Build an mpv handle wired for render-context output. Sets vo=libmpv so
    /// frames are NOT written to a window mpv owns — they're held until
    /// our render thread calls `mpv_render_context_render()` into the GL
    /// context we provide. This is the v0.8 GUI playback path.
    ///
    /// If no render context is bound, frames silently pile up. Callers must
    /// pair this with an MpvRenderContext on a live GL thread.
    pub fn new_for_render() -> Result<Self> {
        let mpv = MpvHandle::new("libmpv")?;
        Ok(Self {
            mpv,
            current_file: Mutex::new(None),
            audio: Mutex::new(audio::load()),
            sponsor_segments: Mutex::new(Vec::new()),
            virtual_chapters: Mutex::new(Vec::new()),
        })
    }

    /// Borrow the underlying mpv handle. Used by the render loop to bind a
    /// render context to this player's mpv instance.
    pub fn mpv_handle(&self) -> &MpvHandle {
        &self.mpv
    }

    /// Create a player with video output (mpv opens its own window).
    pub fn new_with_video() -> Result<Self> {
        let mpv = MpvHandle::new_with_video()?;
        Ok(Self {
            mpv,
            current_file: Mutex::new(None),
            audio: Mutex::new(audio::load()),
            sponsor_segments: Mutex::new(Vec::new()),
            virtual_chapters: Mutex::new(Vec::new()),
        })
    }

    /// Create a player that renders video into an existing window handle (HWND on Windows).
    pub fn new_with_wid(wid: i64) -> Result<Self> {
        let mpv = MpvHandle::new_with_wid(wid)?;
        Ok(Self {
            mpv,
            current_file: Mutex::new(None),
            audio: Mutex::new(audio::load()),
            sponsor_segments: Mutex::new(Vec::new()),
            virtual_chapters: Mutex::new(Vec::new()),
        })
    }

    /// Linux variant: mpv embeds into the given X11 Window XID and uses
    /// `vo=x11` for CPU-side XPutImage rendering. Skips GL entirely so
    /// playback works on every X11 system regardless of GPU support
    /// (VMs, llvmpipe, remote X, etc.).
    #[cfg(target_os = "linux")]
    pub fn new_with_wid_x11(wid: i64) -> Result<Self> {
        let mpv = MpvHandle::new_with_wid_x11(wid)?;
        Ok(Self {
            mpv,
            current_file: Mutex::new(None),
            audio: Mutex::new(audio::load()),
            sponsor_segments: Mutex::new(Vec::new()),
            virtual_chapters: Mutex::new(Vec::new()),
        })
    }

    pub fn play(
        &self,
        path: &str,
        seek: Option<f64>,
        volume: Option<i64>,
        speed: Option<f64>,
    ) -> Result<LoadOutcome> {
        // Set volume/speed before loading file
        if let Some(v) = volume {
            self.mpv.set_property_i64("volume", v.clamp(0, 100))?;
        }
        if let Some(s) = speed {
            self.mpv.set_property_f64("speed", s)?;
        }

        // A share is a file as far as mpv is concerned, so its cache stays
        // off — and a 4K remux read over Wi-Fi stutters at every seek. Turn
        // the cache on for the one source we can identify from the path
        // alone, and hand everything else back to mpv's own judgement.
        let _ = self.mpv.set_property_string(
            "cache",
            if source::is_unc_path(path) { "yes" } else { "auto" },
        );

        // A disc is not a file. `D:ilm.iso` handed to mpv as a path gets
        // an ISO9660 image demuxed as if it were a container; what mpv wants
        // is `dvd://` plus a device pointing at the image. Deciding it here
        // rather than in the daemon means the window's drag-and-drop, which
        // reaches `player_play` directly, gets it too.
        let target = match disc::detect(path) {
            Some(d) => {
                // Before libdvdnav gets anywhere near the disc.
                disc::ensure_console();
                if !d.device.is_empty() {
                    self.mpv
                        .set_property_string(d.kind.device_property(), &d.device)?;
                }
                d.url
            }
            None => path.to_string(),
        };

        // Nothing else in unflick reads mpv's event queue — auto-advance
        // polls `eof-reached` instead — so whatever the last file left in
        // there is still sitting there. Clear it first, or the end-of-file
        // event from the *previous* file gets read as this one failing.
        while self.mpv.wait_event(0.0).0 != MPV_EVENT_NONE {}

        // Load the file
        self.mpv.command(&["loadfile", &target])?;

        // Report against what the caller asked for, not what mpv was told:
        // "could not open dvd://" names nothing the user can act on.
        let outcome = self.await_load(path)?;

        // `pause` is a global mpv property, not per-file, and `keep-open=yes`
        // leaves it set after a file reaches its end. Without clearing it
        // here, anything loaded next comes up paused at 0:00 — which the
        // playlist auto-advance hits every single time, and a plain
        // `unflick play <file>` hits too once the previous file ran out.
        // A command named "play" should play.
        let _ = self.mpv.set_property_bool("pause", false);

        // Re-apply the audio chain for the new file. `af` is a global option
        // that in principle survives a file change, but re-applying is what
        // makes a restored-from-settings equaliser take effect at all: the
        // state is loaded when the Player is built, long before mpv has an
        // audio chain to put filters in. Doing it here also means one code
        // path for "restore on startup" and "keep it across files".
        let _ = self.apply_chain(&self.audio_settings());

        *self.current_file.lock().unwrap() = Some(path.to_string());
        // Clear stale SponsorBlock segments — they were for the previous
        // file. The URL play path will re-arm via after_play_url_hooks.
        if let Ok(mut segs) = self.sponsor_segments.lock() {
            segs.clear();
        }
        // Synthesised chapters describe one specific recording, so they go
        // with it. Leaving them would put another file's chapter marks on
        // this one's timeline.
        if let Ok(mut ch) = self.virtual_chapters.lock() {
            ch.clear();
        }

        if let Some(pos) = seek {
            // A loaded file can be seeked immediately. One that is still
            // opening cannot, so fall back to the old guess of a moment —
            // it is the best available when mpv hasn't said anything yet.
            if outcome == LoadOutcome::Pending {
                thread::sleep(Duration::from_millis(200));
            }
            let _ = self.mpv.set_property_f64("time-pos", pos);
        }

        Ok(outcome)
    }

    /// Block until mpv says the source opened, failed, or the deadline passes.
    ///
    /// An end-of-file event arriving before file-loaded means the source never
    /// opened: a wrong path, an unreachable host, a codec mpv can't read. That
    /// is the case worth reporting — a `play` that returns "playing" for a file
    /// nobody can see is the kind of lie that sends people looking for the bug
    /// in their own script.
    fn await_load(&self, path: &str) -> Result<LoadOutcome> {
        let deadline = std::time::Instant::now() + LOAD_TIMEOUT;
        // `loadfile` over a file that is already playing ends that file
        // first, so an end-of-file event only speaks for the new file once
        // mpv has said it started on it.
        let mut started = false;
        while std::time::Instant::now() < deadline {
            match self.mpv.wait_event(0.1).0 {
                MPV_EVENT_START_FILE => started = true,
                MPV_EVENT_FILE_LOADED => return Ok(LoadOutcome::Loaded),
                MPV_EVENT_END_FILE if started => {
                    // mpv has unloaded whatever was playing before, so the
                    // state that described it has to go with it.
                    self.forget_current();
                    bail!("could not open {}", path);
                }
                _ => {}
            }
        }
        Ok(LoadOutcome::Pending)
    }

    /// Drop everything that described the file mpv was playing.
    fn forget_current(&self) {
        *self.current_file.lock().unwrap() = None;
        if let Ok(mut segs) = self.sponsor_segments.lock() {
            segs.clear();
        }
        if let Ok(mut ch) = self.virtual_chapters.lock() {
            ch.clear();
        }
    }

    pub fn pause(&self) -> Result<()> {
        self.mpv.set_property_bool("pause", true)
    }

    pub fn resume(&self) -> Result<()> {
        // After EOF mpv stays paused at duration. A naive unpause is a
        // no-op there, which feels like "the play button is broken" —
        // so when the user hits resume from end-of-file, rewind to the
        // start. Half-second tolerance handles float drift between
        // time-pos and duration.
        let pos = self.mpv.get_property_f64("time-pos").unwrap_or(0.0);
        let dur = self.mpv.get_property_f64("duration").unwrap_or(0.0);
        if dur > 0.0 && pos >= dur - 0.5 {
            let _ = self.mpv.set_property_f64("time-pos", 0.0);
        }
        self.mpv.set_property_bool("pause", false)
    }

    pub fn stop(&self) -> Result<()> {
        self.mpv.command(&["stop"])?;
        *self.current_file.lock().unwrap() = None;
        if let Ok(mut segs) = self.sponsor_segments.lock() {
            segs.clear();
        }
        if let Ok(mut ch) = self.virtual_chapters.lock() {
            ch.clear();
        }
        Ok(())
    }

    /// Replace the SponsorBlock segment list for the currently-loaded file.
    /// Called by `after_play_url_hooks` once segments have been fetched.
    pub fn enable_sponsorblock(&self, segments: Vec<Segment>) {
        if let Ok(mut s) = self.sponsor_segments.lock() {
            *s = segments;
        }
    }

    /// Returns `Some(end)` if `current_time` is inside any "skip" segment.
    /// Caller seeks to the returned end-time. `None` if no segment matches.
    ///
    /// We only auto-skip segments whose `action_type == "skip"`. Mute / poi
    /// segments are intentionally ignored — those need different UX.
    ///
    /// Tolerance: skip when current_time is within [start, end - 0.05]. The
    /// 50 ms slack at the tail prevents an immediate re-trigger after we
    /// seek to `end` (mpv's reported time-pos may briefly land just inside
    /// the segment again due to float rounding / decoder catch-up).
    pub fn check_sponsor_skip(&self, current_time: f64) -> Option<f64> {
        let segs = self.sponsor_segments.lock().ok()?;
        for seg in segs.iter() {
            if seg.action_type != "skip" {
                continue;
            }
            if current_time >= seg.start && current_time < seg.end - 0.05 {
                return Some(seg.end);
            }
        }
        None
    }

    pub fn seek(&self, position: f64) -> Result<()> {
        // Retry a few times if the property isn't available yet (file still loading)
        for _ in 0..5 {
            match self.mpv.set_property_f64("time-pos", position.max(0.0)) {
                Ok(()) => return Ok(()),
                Err(_) => thread::sleep(Duration::from_millis(200)),
            }
        }
        self.mpv.set_property_f64("time-pos", position.max(0.0))
    }

    pub fn set_volume(&self, volume: i64) -> Result<()> {
        self.mpv.set_property_i64("volume", volume.clamp(0, 100))
    }

    /// URL schemes this mpv build can actually open, straight from mpv.
    ///
    /// Read at call time rather than cached: the bundled libmpv on Windows and
    /// a distro's libmpv on Linux are built against different ffmpeg options,
    /// and the difference is exactly what the caller is asking about.
    pub fn supported_protocols(&self) -> Vec<String> {
        self.mpv
            .get_property_string("protocol-list")
            .map(|list| {
                let mut names: Vec<String> = list
                    .split(',')
                    .map(|s| s.trim().to_ascii_lowercase())
                    .filter(|s| !s.is_empty())
                    .collect();
                names.sort();
                names.dedup();
                names
            })
            .unwrap_or_default()
    }

    pub fn speed(&self) -> f64 {
        self.mpv.get_property_f64("speed").unwrap_or(1.0)
    }

    /// mpv refuses anything outside this band and reports the rejection as a
    /// generic property error, which reads like a bug rather than a bad
    /// argument. Reject it here so the message says what is wrong.
    pub fn set_speed(&self, speed: f64) -> Result<()> {
        if !speed.is_finite() || !(SPEED_MIN..=SPEED_MAX).contains(&speed) {
            bail!("speed must be between {} and {}", SPEED_MIN, SPEED_MAX);
        }
        self.mpv.set_property_f64("speed", speed)
    }

    pub fn get_property_i64(&self, name: &str) -> Result<i64> {
        self.mpv.get_property_i64(name)
    }

    pub fn set_property_i64(&self, name: &str, value: i64) -> Result<()> {
        self.mpv.set_property_i64(name, value)
    }

    pub fn get_property_string(&self, name: &str) -> Result<String> {
        self.mpv.get_property_string(name)
    }

    pub fn get_property_bool(&self, name: &str) -> Result<bool> {
        self.mpv.get_property_bool(name)
    }

    pub fn status(&self) -> PlayerStatus {
        let file = self.current_file.lock().unwrap().clone()
            .or_else(|| self.mpv.get_property_string("path").ok());

        let position = self.mpv.get_property_f64("time-pos").unwrap_or(0.0);
        let duration = self.mpv.get_property_f64("duration").unwrap_or(0.0);
        let volume = self.mpv.get_property_i64("volume").unwrap_or(100);
        let speed = self.mpv.get_property_f64("speed").unwrap_or(1.0);
        let paused = self.mpv.get_property_bool("pause").unwrap_or(true);
        let idle = self.mpv.get_property_bool("idle-active").unwrap_or(true);

        let state = if idle && file.is_none() {
            PlaybackState::Stopped
        } else if paused {
            PlaybackState::Paused
        } else {
            PlaybackState::Playing
        };

        PlayerStatus {
            state,
            file,
            position,
            duration,
            volume,
            speed,
        }
    }

    /// Take a screenshot of the current video frame
    pub fn screenshot(&self, path: &str) -> Result<()> {
        self.mpv.command(&["screenshot-to-file", path, "video"])
    }

    /// Load an external subtitle file
    pub fn subtitle_load(&self, path: &str) -> Result<()> {
        self.mpv.command(&["sub-add", path])
    }

    /// List all subtitle tracks
    pub fn subtitle_list(&self) -> Vec<SubtitleTrack> {
        let count = self.mpv.get_property_i64("track-list/count").unwrap_or(0);
        let mut subs = Vec::new();
        for i in 0..count {
            let track_type = self.mpv.get_property_string(&format!("track-list/{}/type", i)).unwrap_or_default();
            if track_type != "sub" {
                continue;
            }
            let id = self.mpv.get_property_i64(&format!("track-list/{}/id", i)).unwrap_or(0);
            let title = self.mpv.get_property_string(&format!("track-list/{}/title", i)).ok();
            let lang = self.mpv.get_property_string(&format!("track-list/{}/lang", i)).ok();
            let external = self.mpv.get_property_string(&format!("track-list/{}/external-filename", i)).ok();
            let selected = self.mpv.get_property_bool(&format!("track-list/{}/selected", i)).unwrap_or(false);
            subs.push(SubtitleTrack { id, title, lang, external_file: external, selected });
        }
        subs
    }

    /// Select a subtitle track by ID (0 to disable)
    pub fn subtitle_select(&self, id: i64) -> Result<()> {
        self.mpv.set_property_i64("sid", id)
    }

    /// List all audio tracks
    pub fn audio_list(&self) -> Vec<AudioTrack> {
        let count = self.mpv.get_property_i64("track-list/count").unwrap_or(0);
        let mut tracks = Vec::new();
        for i in 0..count {
            let track_type = self.mpv.get_property_string(&format!("track-list/{}/type", i)).unwrap_or_default();
            if track_type != "audio" {
                continue;
            }
            let id = self.mpv.get_property_i64(&format!("track-list/{}/id", i)).unwrap_or(0);
            let title = self.mpv.get_property_string(&format!("track-list/{}/title", i)).ok();
            let lang = self.mpv.get_property_string(&format!("track-list/{}/lang", i)).ok();
            let codec = self.mpv.get_property_string(&format!("track-list/{}/codec", i)).ok();
            let selected = self.mpv.get_property_bool(&format!("track-list/{}/selected", i)).unwrap_or(false);
            tracks.push(AudioTrack { id, title, lang, codec, selected });
        }
        tracks
    }

    /// Select an audio track by ID (0 to disable)
    pub fn audio_select(&self, id: i64) -> Result<()> {
        self.mpv.set_property_i64("aid", id)
    }

    // ─── Subtitle / audio timing ──────────────────────────────────────────
    //
    // Positive `sub-delay` shows subtitles *later*. This matters most for
    // whisper-generated tracks, which routinely land a few hundred ms off
    // the dialogue — before this existed there was no way to correct them.

    pub fn sub_delay(&self) -> f64 {
        self.mpv.get_property_f64("sub-delay").unwrap_or(0.0)
    }

    pub fn set_sub_delay(&self, seconds: f64) -> Result<()> {
        self.mpv.set_property_f64("sub-delay", seconds)
    }

    pub fn audio_delay(&self) -> f64 {
        self.mpv.get_property_f64("audio-delay").unwrap_or(0.0)
    }

    pub fn set_audio_delay(&self, seconds: f64) -> Result<()> {
        self.mpv.set_property_f64("audio-delay", seconds)
    }

    // ─── Chapters ─────────────────────────────────────────────────────────

    /// Chapters for the current file: the container's own, or the
    /// synthesised ones if it has none. Empty when there are neither.
    pub fn chapter_list(&self) -> Vec<Chapter> {
        let real = self.container_chapters();
        if !real.is_empty() {
            return real;
        }
        self.virtual_chapter_list()
    }

    /// Replace the synthesised chapters. Times are clamped to the file and
    /// sorted, so a caller can hand over a rough list without pre-cleaning
    /// it. Passing an empty list clears them.
    pub fn set_virtual_chapters(&self, mut entries: Vec<(f64, String)>) -> Result<usize> {
        if !self.container_chapters().is_empty() {
            bail!("this file already has its own chapters");
        }
        let duration = self.mpv.get_property_f64("duration").unwrap_or(0.0);

        entries.retain(|(time, _)| time.is_finite() && *time >= 0.0);
        if duration > 0.0 {
            entries.retain(|(time, _)| *time < duration);
        }
        entries.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        entries.dedup_by(|a, b| (a.0 - b.0).abs() < 0.5);

        let chapters: Vec<Chapter> = entries
            .into_iter()
            .enumerate()
            .map(|(index, (time, title))| Chapter {
                index: index as i64,
                title: Some(title),
                time,
                current: false,
            })
            .collect();

        let count = chapters.len();
        *self.virtual_chapters.lock().unwrap() = chapters;
        Ok(count)
    }

    pub fn clear_virtual_chapters(&self) {
        self.virtual_chapters.lock().unwrap().clear();
    }

    /// Synthesised chapters with `current` resolved against playback position.
    fn virtual_chapter_list(&self) -> Vec<Chapter> {
        let mut chapters = self.virtual_chapters.lock().unwrap().clone();
        if chapters.is_empty() {
            return chapters;
        }
        // mpv owns `current` for real chapters; for ours we work it out from
        // the clock — the last chapter whose start we've passed.
        let pos = self.mpv.get_property_f64("time-pos").unwrap_or(0.0);
        let current = chapters
            .iter()
            .rposition(|c| pos >= c.time)
            .unwrap_or(0);
        for (i, c) in chapters.iter_mut().enumerate() {
            c.current = i == current;
        }
        chapters
    }

    /// Read mpv's `chapter-list`. Empty when the container has no chapters
    /// (most streaming sources).
    fn container_chapters(&self) -> Vec<Chapter> {
        let count = self.mpv.get_property_i64("chapter-list/count").unwrap_or(0);
        let current = self.mpv.get_property_i64("chapter").unwrap_or(-1);
        let mut chapters = Vec::new();
        for i in 0..count {
            let title = self
                .mpv
                .get_property_string(&format!("chapter-list/{}/title", i))
                .ok()
                .filter(|s| !s.is_empty());
            let time = self
                .mpv
                .get_property_f64(&format!("chapter-list/{}/time", i))
                .unwrap_or(0.0);
            chapters.push(Chapter {
                index: i,
                title,
                time,
                current: i == current,
            });
        }
        chapters
    }

    /// Jump to a chapter by 0-based index. Works for container chapters and
    /// synthesised ones alike — the latter seek by time, since mpv doesn't
    /// know about them.
    pub fn chapter_seek(&self, index: i64) -> Result<()> {
        let count = self.mpv.get_property_i64("chapter-list/count").unwrap_or(0);
        if count > 0 {
            if index < 0 || index >= count {
                bail!("chapter {} out of range (0..{})", index, count - 1);
            }
            return self.mpv.set_property_i64("chapter", index);
        }

        let virtual_chapters = self.virtual_chapters.lock().unwrap();
        if virtual_chapters.is_empty() {
            bail!("this file has no chapters");
        }
        let target = virtual_chapters
            .get(index.max(0) as usize)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "chapter {} out of range (0..{})",
                    index,
                    virtual_chapters.len() - 1
                )
            })?;
        let time = target.time;
        drop(virtual_chapters);
        self.seek(time)
    }

    /// Step one chapter forward (`delta = 1`) or back (`delta = -1`).
    /// Clamps at both ends rather than wrapping or erroring at the edges.
    pub fn chapter_step(&self, delta: i64) -> Result<i64> {
        let count = self.mpv.get_property_i64("chapter-list/count").unwrap_or(0);
        if count > 0 {
            let current = self.mpv.get_property_i64("chapter").unwrap_or(0);
            let target = (current + delta).clamp(0, count - 1);
            self.mpv.set_property_i64("chapter", target)?;
            return Ok(target);
        }

        let chapters = self.virtual_chapter_list();
        if chapters.is_empty() {
            bail!("this file has no chapters");
        }
        let current = chapters.iter().position(|c| c.current).unwrap_or(0) as i64;
        let target = (current + delta).clamp(0, chapters.len() as i64 - 1);
        self.chapter_seek(target)?;
        Ok(target)
    }

    // ─── A-B loop ─────────────────────────────────────────────────────────
    //
    // mpv stores the bounds in `ab-loop-a` / `ab-loop-b`, which hold either
    // a time in seconds or the literal string "no" when unset. We read them
    // as strings for exactly that reason — a get_property_f64 on an unset
    // bound fails, which is indistinguishable from a real error.

    fn read_ab_bound(&self, prop: &str) -> Option<f64> {
        let raw = self.mpv.get_property_string(prop).ok()?;
        if raw == "no" {
            return None;
        }
        raw.parse::<f64>().ok()
    }

    pub fn ab_loop_status(&self) -> AbLoop {
        let a = self.read_ab_bound("ab-loop-a");
        let b = self.read_ab_bound("ab-loop-b");
        AbLoop {
            active: a.is_some() && b.is_some(),
            a,
            b,
        }
    }

    /// Set the A bound. `None` uses the current playback position, which is
    /// what the `[` hotkey does.
    pub fn ab_loop_set_a(&self, position: Option<f64>) -> Result<AbLoop> {
        let pos = position.unwrap_or_else(|| self.mpv.get_property_f64("time-pos").unwrap_or(0.0));
        self.mpv.set_property_f64("ab-loop-a", pos.max(0.0))?;
        Ok(self.ab_loop_status())
    }

    /// Set the B bound. `None` uses the current playback position.
    pub fn ab_loop_set_b(&self, position: Option<f64>) -> Result<AbLoop> {
        let pos = position.unwrap_or_else(|| self.mpv.get_property_f64("time-pos").unwrap_or(0.0));
        self.mpv.set_property_f64("ab-loop-b", pos.max(0.0))?;
        Ok(self.ab_loop_status())
    }

    pub fn ab_loop_clear(&self) -> Result<()> {
        self.mpv.set_property_string("ab-loop-a", "no")?;
        self.mpv.set_property_string("ab-loop-b", "no")
    }

    // ─── Frame stepping ───────────────────────────────────────────────────
    //
    // Both mpv commands pause playback as a side effect, which is the
    // expected behaviour — you step frames to inspect a still.

    pub fn frame_step(&self) -> Result<()> {
        self.mpv.command(&["frame-step"])
    }

    pub fn frame_back_step(&self) -> Result<()> {
        self.mpv.command(&["frame-back-step"])
    }

    // ─── Subtitle styling ─────────────────────────────────────────────────

    /// Read every styling property as JSON, using mpv's own defaults for
    /// anything unavailable.
    pub fn subtitle_style(&self) -> serde_json::Value {
        serde_json::json!({
            "scale": self.mpv.get_property_f64("sub-scale").unwrap_or(1.0),
            "pos": self.mpv.get_property_i64("sub-pos").unwrap_or(100),
            "color": self.mpv.get_property_string("sub-color").unwrap_or_else(|_| "#FFFFFFFF".into()),
            "border_size": self.mpv.get_property_f64("sub-border-size").unwrap_or(3.0),
            "bold": self.mpv.get_property_bool("sub-bold").unwrap_or(false),
        })
    }

    /// Set one styling property. Values are clamped to ranges that stay
    /// legible — an unclamped `sub-scale` of 0 makes subtitles vanish with
    /// no obvious way back.
    pub fn set_subtitle_style(&self, name: &str, value: &serde_json::Value) -> Result<()> {
        match name {
            "scale" => {
                let v = value.as_f64().unwrap_or(1.0).clamp(0.1, 10.0);
                self.mpv.set_property_f64("sub-scale", v)
            }
            "pos" => {
                let v = value.as_i64().unwrap_or(100).clamp(0, 150);
                self.mpv.set_property_i64("sub-pos", v)
            }
            "color" => {
                let v = value.as_str().unwrap_or("#FFFFFFFF");
                self.mpv.set_property_string("sub-color", v)
            }
            "border_size" => {
                let v = value.as_f64().unwrap_or(3.0).clamp(0.0, 20.0);
                self.mpv.set_property_f64("sub-border-size", v)
            }
            "bold" => {
                let v = value.as_bool().unwrap_or(false);
                self.mpv.set_property_bool("sub-bold", v)
            }
            _ => bail!(
                "unknown subtitle style: {} (expected scale | pos | color | border_size | bold)",
                name
            ),
        }
    }
}

/// Styling keys accepted by `set_subtitle_style`, for CLI/MCP validation
/// and help text.
pub const SUBTITLE_STYLE_KEYS: &[&str] = &["scale", "pos", "color", "border_size", "bold"];

/// Geometry knobs accepted by `set_video_transform`.
// --- Audio processing ------------------------------------------------------

impl Player {
    /// Current equaliser / normalisation state.
    pub fn audio_settings(&self) -> AudioSettings {
        self.audio.lock().unwrap().clone()
    }

    /// Replace the whole audio state and rebuild the filter chain.
    pub fn set_audio_settings(&self, next: AudioSettings) -> Result<AudioSettings> {
        let next = next.normalized();
        let applied = self.apply_chain(&next)?;
        *self.audio.lock().unwrap() = applied.clone();
        Ok(applied)
    }

    /// Set one band's gain.
    ///
    /// Rebuilds the chain, like every other change. See the note on
    /// `af-command` in `core::audio` for why there is no cheaper path.
    pub fn set_band(&self, index: usize, gain_db: f64) -> Result<AudioSettings> {
        let mut next = self.audio_settings();
        if next.bands.len() != audio::BANDS.len() {
            next.bands.resize(audio::BANDS.len(), 0.0);
        }
        next.bands[index] = gain_db;
        self.set_audio_settings(next)
    }

    /// Apply a named preset's curve, leaving preamp and normalisation alone.
    pub fn set_audio_preset(&self, name: &str) -> Result<AudioSettings> {
        let preset = audio::preset(name)?;
        let mut next = self.audio_settings();
        next.bands = preset.bands.to_vec();
        // Choosing a curve means wanting to hear it.
        next.equalizer = true;
        self.set_audio_settings(next)
    }

    /// Drop every audio filter and return to a flat, unprocessed signal.
    pub fn reset_audio(&self) -> Result<AudioSettings> {
        self.set_audio_settings(AudioSettings::default())
    }

    /// Push a chain to mpv, or clear it when there is nothing to apply.
    ///
    /// A filter that fails to initialise takes the rest of the chain with it
    /// and can leave playback silent, so a rejected chain is rolled back to
    /// no filters rather than left half-applied.
    fn apply_chain(&self, settings: &AudioSettings) -> Result<AudioSettings> {
        let chain = audio::build_chain(settings);
        let result = if chain.is_empty() {
            self.mpv.command(&["af", "clr", ""])
        } else {
            self.mpv.command(&["af", "set", &chain])
        };

        match result {
            Ok(()) => Ok(settings.clone()),
            Err(e) => {
                let _ = self.mpv.command(&["af", "clr", ""]);
                Err(anyhow!(
                    "mpv rejected the audio filter chain ({}); filters cleared. \
                     This usually means the bundled ffmpeg lacks a filter: {}",
                    e,
                    chain
                ))
            }
        }
    }

    /// Whether mpv is correcting pitch when playback speed changes.
    ///
    /// Without it, 1.5x speech sounds like a chipmunk. mpv defaults this on,
    /// but people who want the pitch to move — musicians checking a tempo
    /// change — need it off, and nothing exposed the switch.
    /// The filter chain mpv is actually running, as mpv reports it.
    ///
    /// Ground truth, deliberately separate from `audio_settings`: that one is
    /// what we asked for, this is what took effect. When they disagree the
    /// chain failed to initialise, and without this the two are
    /// indistinguishable from the outside.
    pub fn audio_chain(&self) -> String {
        self.mpv.get_property_string("af").unwrap_or_default()
    }

    pub fn pitch_correction(&self) -> bool {
        self.mpv
            .get_property_bool("audio-pitch-correction")
            .unwrap_or(true)
    }

    pub fn set_pitch_correction(&self, on: bool) -> Result<()> {
        self.mpv.set_property_bool("audio-pitch-correction", on)
    }
}

pub const VIDEO_TRANSFORM_KEYS: &[&str] =
    &["aspect", "rotate", "zoom", "panscan", "deinterlace"];

impl Player {
    /// How the picture is currently being fitted to the window.
    ///
    /// Separate from `filter_*` (brightness, contrast, …): those change
    /// what the pixels look like, these change where they go. Users reach
    /// for them for different reasons — a squashed 4:3 broadcast, a phone
    /// video recorded sideways, black bars they want cropped away.
    pub fn video_transform(&self) -> serde_json::Value {
        let aspect = self
            .mpv
            .get_property_f64("video-aspect-override")
            .unwrap_or(-1.0);
        serde_json::json!({
            // mpv reports -1 for "use whatever the container says".
            "aspect": if aspect <= 0.0 { "auto".to_string() } else { format!("{:.4}", aspect) },
            "rotate": self.mpv.get_property_i64("video-rotate").unwrap_or(0),
            // Exposed as a linear multiplier; mpv stores log2 of it.
            "zoom": 2f64.powf(self.mpv.get_property_f64("video-zoom").unwrap_or(0.0)),
            "panscan": self.mpv.get_property_f64("panscan").unwrap_or(0.0),
            "deinterlace": self.mpv.get_property_bool("deinterlace").unwrap_or(false),
        })
    }

    /// Set one geometry knob.
    ///
    /// `aspect` takes `auto`, a ratio like `16:9`, or a decimal. `zoom` is
    /// a plain multiplier — 1 is fit-to-window, 2 is twice as large —
    /// which is converted to the log2 scale mpv actually stores, because
    /// nobody thinks about zoom in logarithms.
    pub fn set_video_transform(&self, name: &str, value: &serde_json::Value) -> Result<()> {
        match name {
            "aspect" => {
                let raw = value
                    .as_str()
                    .map(|s| s.to_string())
                    .or_else(|| value.as_f64().map(|f| f.to_string()))
                    .unwrap_or_default();
                let ratio = parse_aspect(&raw)?;
                self.mpv.set_property_f64("video-aspect-override", ratio)
            }
            "rotate" => {
                // mpv accepts 0-359 but only right angles are meaningful
                // for video, and anything else looks like a mistake.
                let raw = value.as_i64().unwrap_or(0);
                let degrees = raw.rem_euclid(360);
                if degrees % 90 != 0 {
                    bail!("rotate must be 0, 90, 180 or 270 (got {})", raw);
                }
                self.mpv.set_property_i64("video-rotate", degrees)
            }
            "zoom" => {
                let scale = value.as_f64().unwrap_or(1.0);
                if !(scale.is_finite() && scale > 0.0) {
                    bail!("zoom must be a positive multiplier");
                }
                let clamped = scale.clamp(0.25, 8.0);
                self.mpv.set_property_f64("video-zoom", clamped.log2())
            }
            "panscan" => {
                let v = value.as_f64().unwrap_or(0.0).clamp(0.0, 1.0);
                self.mpv.set_property_f64("panscan", v)
            }
            "deinterlace" => {
                let on = value.as_bool().unwrap_or(false);
                self.mpv.set_property_bool("deinterlace", on)
            }
            _ => bail!(
                "unknown video transform: {} (expected {})",
                name,
                VIDEO_TRANSFORM_KEYS.join(" | ")
            ),
        }
    }

    /// Back to "show it the way the file says".
    pub fn reset_video_transform(&self) -> Result<()> {
        self.mpv.set_property_f64("video-aspect-override", -1.0)?;
        self.mpv.set_property_i64("video-rotate", 0)?;
        self.mpv.set_property_f64("video-zoom", 0.0)?;
        self.mpv.set_property_f64("panscan", 0.0)?;
        self.mpv.set_property_bool("deinterlace", false)
    }
}

/// `auto` → -1 (mpv's "use the container"), `16:9` → 1.777…, `1.85` → 1.85.
fn parse_aspect(raw: &str) -> Result<f64> {
    let raw = raw.trim();
    if raw.is_empty() || raw.eq_ignore_ascii_case("auto") || raw == "-1" {
        return Ok(-1.0);
    }
    if let Some((w, h)) = raw.split_once(':') {
        let w: f64 = w.trim().parse().map_err(|_| anyhow::anyhow!("bad aspect: {}", raw))?;
        let h: f64 = h.trim().parse().map_err(|_| anyhow::anyhow!("bad aspect: {}", raw))?;
        if !(w > 0.0 && h > 0.0) {
            bail!("aspect ratio parts must be positive: {}", raw);
        }
        return Ok(w / h);
    }
    let value: f64 = raw
        .parse()
        .map_err(|_| anyhow::anyhow!("bad aspect: {} (try auto, 16:9, or 1.78)", raw))?;
    if value <= 0.0 {
        bail!("aspect must be positive: {}", raw);
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::parse_aspect;

    #[test]
    fn aspect_accepts_ratios_decimals_and_auto() {
        assert_eq!(parse_aspect("auto").unwrap(), -1.0);
        assert_eq!(parse_aspect("").unwrap(), -1.0);
        assert_eq!(parse_aspect("-1").unwrap(), -1.0);
        assert!((parse_aspect("16:9").unwrap() - 16.0 / 9.0).abs() < 1e-9);
        assert!((parse_aspect(" 4 : 3 ").unwrap() - 4.0 / 3.0).abs() < 1e-9);
        assert!((parse_aspect("1.85").unwrap() - 1.85).abs() < 1e-9);
    }

    #[test]
    fn aspect_rejects_nonsense() {
        // A zero or negative side would make mpv squash the picture to
        // nothing, with no obvious way back.
        assert!(parse_aspect("16:0").is_err());
        assert!(parse_aspect("0:9").is_err());
        assert!(parse_aspect("-2").is_err());
        assert!(parse_aspect("widescreen").is_err());
        assert!(parse_aspect("16:9:4").is_err());
    }
}

/// Probe a media file's metadata without affecting any active playback.
/// Spins up an isolated headless mpv with `pause=yes`, loads the file, waits for
/// the FILE_LOADED event, then reads metadata properties.
pub fn probe_file(path: &str) -> Result<FileInfo> {
    let mpv = MpvHandle::new("null")?;
    mpv.set_property_bool("pause", true)?;
    mpv.command(&["loadfile", path])?;

    // Drain events until file-loaded or end-of-file (load failure), capped by a deadline.
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let mut loaded = false;
    while std::time::Instant::now() < deadline {
        let (event_id, _err) = mpv.wait_event(0.1);
        if event_id == MPV_EVENT_FILE_LOADED {
            loaded = true;
            break;
        }
        if event_id == MPV_EVENT_END_FILE {
            // Load failed before file-loaded fired
            break;
        }
    }

    if !loaded {
        bail!("failed to probe {}: file did not load within 5s", path);
    }

    let duration = mpv.get_property_f64("duration").unwrap_or(0.0);
    let width = mpv.get_property_i64("width").ok();
    let height = mpv.get_property_i64("height").ok();
    let video_codec = mpv.get_property_string("video-codec").ok();
    let audio_codec = mpv.get_property_string("audio-codec").ok();
    let fps = mpv.get_property_f64("container-fps").ok();
    let container = mpv.get_property_string("file-format").ok();

    Ok(FileInfo {
        path: path.to_string(),
        duration,
        width,
        height,
        video_codec,
        audio_codec,
        fps,
        container,
    })
}

/// Locate ffmpeg by searching alongside the running executable, then PATH.
/// Used by CLI/daemon code that doesn't have a Tauri AppHandle.
pub fn find_ffmpeg() -> Option<std::path::PathBuf> {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for sub in ["ffmpeg", ""] {
                for name in ["ffmpeg.exe", "ffmpeg"] {
                    let p = if sub.is_empty() {
                        dir.join(name)
                    } else {
                        dir.join(sub).join(name)
                    };
                    if p.exists() {
                        return Some(p);
                    }
                }
            }
        }
    }
    let candidate = if cfg!(target_os = "windows") { "ffmpeg.exe" } else { "ffmpeg" };
    which::which(candidate).ok()
}

/// Extract a video clip using ffmpeg (standalone, does not need a Player instance).
/// `ffmpeg_path` should point to a bundled or system ffmpeg executable.
pub fn extract_clip(
    input: &str,
    start: f64,
    end: f64,
    output: &str,
    as_gif: bool,
    ffmpeg_path: &str,
) -> Result<String> {
    let duration = end - start;
    if duration <= 0.0 {
        bail!("end time must be after start time");
    }

    // Determine output path
    let output_path = if output.is_empty() {
        let ext = if as_gif { "gif" } else { "mp4" };
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        format!("unflick-clip-{}.{}", ts, ext)
    } else {
        output.to_string()
    };

    let start_str = format!("{:.3}", start);
    let duration_str = format!("{:.3}", duration);

    let mut cmd = std::process::Command::new(ffmpeg_path);
    if as_gif {
        cmd.args([
            "-y", "-ss", &start_str, "-t", &duration_str,
            "-i", input,
            "-vf", "fps=15,scale=480:-1:flags=lanczos",
            "-loop", "0",
            &output_path,
        ]);
    } else {
        cmd.args([
            "-y", "-ss", &start_str, "-t", &duration_str,
            "-i", input,
            "-c", "copy",
            "-avoid_negative_ts", "make_zero",
            &output_path,
        ]);
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    let result = cmd.output();

    match result {
        Ok(cmd_output) => {
            if cmd_output.status.success() {
                Ok(output_path)
            } else {
                let stderr = String::from_utf8_lossy(&cmd_output.stderr);
                bail!("ffmpeg failed: {}", stderr.chars().take(500).collect::<String>())
            }
        }
        Err(e) => {
            if e.kind() == std::io::ErrorKind::NotFound {
                bail!("ffmpeg not found. Please install ffmpeg and add it to PATH.")
            }
            bail!("failed to run ffmpeg: {}", e)
        }
    }
}
