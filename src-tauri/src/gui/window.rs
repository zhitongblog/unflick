//! The window, as something CLI, MCP and the UI can all reshape.
//!
//! Picture-in-picture used to live entirely inside `toggle_pip`: a button,
//! a pair of process-global booleans, and a hard-coded 1024×640 to come back
//! to — which was wrong for anyone whose window was a different size when
//! they left. Music mode needed the same machinery, and two independent
//! copies of it would have fought over the same window: leaving music mode
//! while `IN_PIP` was still set would have restored the wrong shape.
//!
//! So there is one mode, one place that owns it, and one saved geometry to
//! come back to. See `core::window` for the mode itself and the trait the
//! control server talks to.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Mutex;

use tauri::{AppHandle, Emitter, LogicalPosition, LogicalSize, Manager};

use crate::core::events::EventSink;
use crate::core::window::{WindowHost, WindowMode};
use crate::gui::state::GuiPlayer;

/// Picture-in-picture: big enough to follow a video, small enough to keep
/// out of the way.
const PIP_SIZE: (f64, f64) = (400.0, 250.0);
/// Music mode: a column — cover, tags, transport — sized like the music
/// players people already have open.
const MUSIC_SIZE: (f64, f64) = (380.0, 560.0);
/// Where a window goes when it has nowhere to return to.
const DEFAULT_SIZE: (f64, f64) = (1024.0, 640.0);
/// Gap from the screen edges when parking the PiP window, with extra at the
/// bottom for a taskbar or dock.
const EDGE_GAP: f64 = 16.0;
const BOTTOM_GAP: f64 = 60.0;

/// What normal mode looked like, captured on the way out of it.
#[derive(Clone, Copy)]
struct Geometry {
    size: (f64, f64),
    position: (f64, f64),
    always_on_top: bool,
}

pub struct TauriWindowHost {
    app: AppHandle,
    /// `WindowMode` as a number, so the control server's connection threads
    /// can read it without taking a lock.
    mode: AtomicU8,
    /// Restored when the window returns to normal. `None` before the window
    /// has ever left it.
    normal: Mutex<Option<Geometry>>,
}

impl TauriWindowHost {
    pub fn new(app: AppHandle) -> Self {
        Self {
            app,
            mode: AtomicU8::new(encode(WindowMode::Normal)),
            normal: Mutex::new(None),
        }
    }

    /// Flip between `mode` and normal. What every toggle button wants.
    pub fn toggle(&self, mode: WindowMode) -> Result<WindowMode, String> {
        let target = if self.mode() == mode {
            WindowMode::Normal
        } else {
            mode
        };
        self.set_mode(target)?;
        Ok(target)
    }

    fn always_on_top(&self, window: &tauri::WebviewWindow, enabled: bool) -> Result<(), String> {
        window.set_always_on_top(enabled).map_err(|e| e.to_string())?;
        // The video surface is a separate always-on-top window layered over
        // the webview; leaving it behind would hide the picture the moment
        // the player window is raised.
        if let Some(rl) = self.app.state::<GuiPlayer>().render_loop.get() {
            rl.set_always_on_top(enabled);
        }
        Ok(())
    }
}

impl WindowHost for TauriWindowHost {
    fn mode(&self) -> WindowMode {
        decode(self.mode.load(Ordering::Relaxed))
    }

    fn set_mode(&self, mode: WindowMode) -> Result<(), String> {
        let current = self.mode();
        if current == mode {
            // Idempotent on purpose: a script that asks for music mode twice
            // should not shove the window somewhere the second time.
            return Ok(());
        }

        let window = self
            .app
            .get_webview_window("main")
            .ok_or("no main window")?;

        // Leaving normal is the only moment the window's own shape is worth
        // remembering — the compact sizes are ours, not the user's.
        if current == WindowMode::Normal {
            *self.normal.lock().unwrap() = current_geometry(&window);
        }

        match mode {
            WindowMode::Normal => {
                let saved = self.normal.lock().unwrap().take();
                let geometry = saved.unwrap_or(Geometry {
                    size: DEFAULT_SIZE,
                    position: (0.0, 0.0),
                    always_on_top: false,
                });
                self.always_on_top(&window, geometry.always_on_top)?;
                set_inner_size(&window, geometry.size);
                if saved.is_some() {
                    let _ = window
                        .set_position(LogicalPosition::new(geometry.position.0, geometry.position.1));
                } else {
                    let _ = window.center();
                }
            }
            WindowMode::Pip => {
                self.always_on_top(&window, true)?;
                set_inner_size(&window, PIP_SIZE);
                park_bottom_right(&window, PIP_SIZE);
            }
            WindowMode::Music => {
                // Deliberately not always-on-top: music mode is where the
                // window sits for an hour, and pinning it over everything
                // else is a different request (that's what PiP is).
                let restore = self
                    .normal
                    .lock()
                    .unwrap()
                    .map(|g| g.always_on_top)
                    .unwrap_or(false);
                self.always_on_top(&window, restore)?;
                set_inner_size(&window, MUSIC_SIZE);
            }
        }

        self.mode.store(encode(mode), Ordering::Relaxed);
        // One event for every entry point — button, hotkey, CLI, agent — so
        // the frontend never has to guess which layout it is in.
        let _ = self.app.emit("unflick:window-mode", mode.as_str());
        Ok(())
    }
}

/// Set the window's inner size, and make it stick.
///
/// What comes back from `inner_size` after a `set_size` is not what was
/// asked for — on Windows here it lands 20 px taller every time — so a
/// naive capture-and-restore grew the window on every trip out of normal
/// mode and back: three toggles, sixty pixels. Rather than bake in a
/// platform constant that will be wrong on the next machine, ask what
/// actually happened and correct the difference once.
///
/// Only small differences are corrected. A large one means the read came
/// back before the resize landed, and acting on it would move the window
/// somewhere nobody asked for.
fn set_inner_size(window: &tauri::WebviewWindow, target: (f64, f64)) {
    let _ = window.set_size(LogicalSize::new(target.0, target.1));

    let (Ok(scale), Ok(actual)) = (window.scale_factor(), window.inner_size()) else {
        return;
    };
    let dw = target.0 - actual.width as f64 / scale;
    let dh = target.1 - actual.height as f64 / scale;
    if (dw.abs() > 1.0 || dh.abs() > 1.0) && dw.abs() < 64.0 && dh.abs() < 64.0 {
        let _ = window.set_size(LogicalSize::new(target.0 + dw, target.1 + dh));
    }
}

fn current_geometry(window: &tauri::WebviewWindow) -> Option<Geometry> {
    let scale = window.scale_factor().ok()?;
    let size = window.inner_size().ok()?;
    let position = window.outer_position().ok()?;
    Some(Geometry {
        size: (size.width as f64 / scale, size.height as f64 / scale),
        position: (position.x as f64 / scale, position.y as f64 / scale),
        always_on_top: window.is_always_on_top().unwrap_or(false),
    })
}

/// Park the window in the bottom-right of the monitor it is on.
///
/// Not a fixed coordinate: the value this replaced was tuned for a 1080p
/// Windows display and put the window off-screen on a small laptop.
fn park_bottom_right(window: &tauri::WebviewWindow, size: (f64, f64)) {
    let Ok(Some(monitor)) = window.current_monitor() else {
        let _ = window.center();
        return;
    };
    let scale = monitor.scale_factor();
    let mon_size = monitor.size();
    let mon_pos = monitor.position();
    let x = mon_pos.x as f64 / scale + mon_size.width as f64 / scale - size.0 - EDGE_GAP;
    let y = mon_pos.y as f64 / scale + mon_size.height as f64 / scale - size.1 - BOTTOM_GAP;
    let _ = window.set_position(LogicalPosition::new(x, y));
}

fn encode(mode: WindowMode) -> u8 {
    match mode {
        WindowMode::Normal => 0,
        WindowMode::Pip => 1,
        WindowMode::Music => 2,
    }
}

fn decode(value: u8) -> WindowMode {
    match value {
        1 => WindowMode::Pip,
        2 => WindowMode::Music,
        _ => WindowMode::Normal,
    }
}

/// Forwards "this list changed" from the control server to the frontend.
///
/// Same object as the window host purely because both are the GUI's answer
/// to "the control server needs to reach the window", and both need nothing
/// but the AppHandle.
impl EventSink for TauriWindowHost {
    fn notify(&self, topic: &str) {
        let _ = self.app.emit("unflick:changed", topic);
    }
}
