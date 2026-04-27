use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use anyhow::{bail, Result};

use super::types::{PlaybackState, PlayerStatus, SubtitleTrack};
use crate::mpv::MpvHandle;

/// Core player logic backed by libmpv.
pub struct Player {
    mpv: MpvHandle,
    /// Cache the last known file path since mpv may not have "path" available after stop.
    current_file: Mutex<Option<String>>,
}

impl Player {
    pub fn new() -> Result<Self> {
        let mpv = MpvHandle::new("null")?;
        Ok(Self {
            mpv,
            current_file: Mutex::new(None),
        })
    }

    /// Create a player that renders video into an existing window handle (HWND on Windows).
    pub fn new_with_wid(wid: i64) -> Result<Self> {
        let mpv = MpvHandle::new_with_wid(wid)?;
        Ok(Self {
            mpv,
            current_file: Mutex::new(None),
        })
    }

    pub fn play(&self, path: &str, seek: Option<f64>, volume: Option<i64>, speed: Option<f64>) -> Result<()> {
        // Set volume/speed before loading file
        if let Some(v) = volume {
            self.mpv.set_property_i64("volume", v.clamp(0, 100))?;
        }
        if let Some(s) = speed {
            self.mpv.set_property_f64("speed", s)?;
        }

        // Load the file
        self.mpv.command(&["loadfile", path])?;
        *self.current_file.lock().unwrap() = Some(path.to_string());

        // Wait a moment for the file to start loading, then seek if needed
        if let Some(pos) = seek {
            // Give mpv a moment to start the file
            thread::sleep(Duration::from_millis(200));
            let _ = self.mpv.set_property_f64("time-pos", pos);
        }

        Ok(())
    }

    pub fn pause(&self) -> Result<()> {
        self.mpv.set_property_bool("pause", true)
    }

    pub fn resume(&self) -> Result<()> {
        self.mpv.set_property_bool("pause", false)
    }

    pub fn stop(&self) -> Result<()> {
        self.mpv.command(&["stop"])?;
        *self.current_file.lock().unwrap() = None;
        Ok(())
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

    pub fn set_speed(&self, speed: f64) -> Result<()> {
        if speed <= 0.0 {
            bail!("speed must be positive");
        }
        self.mpv.set_property_f64("speed", speed)
    }

    pub fn get_property_i64(&self, name: &str) -> Result<i64> {
        self.mpv.get_property_i64(name)
    }

    pub fn get_property_string(&self, name: &str) -> Result<String> {
        self.mpv.get_property_string(name)
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
}

/// Extract a video clip using ffmpeg (standalone, does not need a Player instance).
pub fn extract_clip(
    input: &str,
    start: f64,
    end: f64,
    output: &str,
    as_gif: bool,
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

    let result = if as_gif {
        std::process::Command::new("ffmpeg")
            .args([
                "-y", "-ss", &start_str, "-t", &duration_str,
                "-i", input,
                "-vf", "fps=15,scale=480:-1:flags=lanczos",
                "-loop", "0",
                &output_path,
            ])
            .output()
    } else {
        std::process::Command::new("ffmpeg")
            .args([
                "-y", "-ss", &start_str, "-t", &duration_str,
                "-i", input,
                "-c", "copy",
                "-avoid_negative_ts", "make_zero",
                &output_path,
            ])
            .output()
    };

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
