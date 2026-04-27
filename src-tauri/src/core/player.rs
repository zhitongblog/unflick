use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use anyhow::{bail, Result};

use super::types::{PlaybackState, PlayerStatus};
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
}
