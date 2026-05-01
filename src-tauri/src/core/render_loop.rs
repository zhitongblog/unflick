//! Render thread that drives mpv → OpenGL surface.
//!
//! Why a dedicated thread? Two reasons:
//!
//! 1. OpenGL contexts are thread-affine. The thread that creates the GL
//!    context owns it; rendering must happen there. Doing GL work on the
//!    Tauri main thread or the mpv worker thread would either fight WebView2
//!    for the UI thread, or break GL's threading rules.
//!
//! 2. mpv's render-context API expects the GL caller and the update callback
//!    to be on different threads — the callback fires from mpv's internal
//!    thread to nudge us; we wake the GL thread to render.
//!
//! Lifecycle:
//!   start(player, surface)
//!     ├─ spawn render thread
//!     │   ├─ surface.make_current()
//!     │   ├─ create MpvRenderContext bound to surface's GL via get_proc_address
//!     │   ├─ register update_callback that signals our condvar
//!     │   └─ loop { wait → render → swap_buffers }
//!     └─ return RenderLoop handle
//!
//!   shutdown()
//!     ├─ flip Shutdown flag, notify
//!     ├─ render thread exits loop, drops MpvRenderContext (must run on GL thread)
//!     └─ join

use std::ffi::c_void;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};

use anyhow::{anyhow, Result};

use crate::mpv::{ffi::MpvRenderUpdateFn, MpvRenderContext};
use crate::video::{get_proc_address_trampoline, VideoSurface};

use super::player::Player;

/// Shared signal between the mpv worker thread (which calls update_callback)
/// and the render thread (which sleeps on the condvar). Boxed and pinned by
/// the render thread itself for the lifetime of the render context.
struct RenderSignal {
    state: Mutex<RenderState>,
    cv: Condvar,
}

struct RenderState {
    /// Set true by mpv update_callback when a new frame is ready.
    frame_ready: bool,
    /// Set true by main thread on geometry changes — forces a redraw even
    /// if mpv hasn't pushed a new frame (e.g. resize during pause).
    redraw: bool,
    /// Set true by RenderLoop::shutdown(), exits the loop.
    shutdown: bool,
}

unsafe extern "C" fn update_callback_trampoline(ctx: *mut c_void) {
    if ctx.is_null() {
        return;
    }
    // Reborrow as &Arc<RenderSignal>; the Arc was leaked into the void* slot
    // at thread start and lives until thread end.
    let signal = unsafe { &*(ctx as *const RenderSignal) };
    let mut state = match signal.state.lock() {
        Ok(g) => g,
        Err(_) => return,
    };
    state.frame_ready = true;
    signal.cv.notify_all();
}

/// Owns the render thread + signal. Drop or call `shutdown()` to stop the
/// thread cleanly. Drop without shutdown also tries to stop, but join is
/// best-effort there.
pub struct RenderLoop {
    join: Option<JoinHandle<()>>,
    signal: Arc<RenderSignal>,
    surface: Arc<dyn VideoSurface>,
    /// Held to keep the mpv handle alive across the loop. The render thread
    /// borrows it via Arc::clone.
    _player: Arc<Player>,
}

impl RenderLoop {
    pub fn start(player: Arc<Player>, surface: Arc<dyn VideoSurface>) -> Result<Self> {
        let signal = Arc::new(RenderSignal {
            state: Mutex::new(RenderState {
                frame_ready: false,
                redraw: false,
                shutdown: false,
            }),
            cv: Condvar::new(),
        });

        let player_for_thread = Arc::clone(&player);
        let surface_for_thread = Arc::clone(&surface);
        let signal_for_thread = Arc::clone(&signal);

        let join = thread::Builder::new()
            .name("unflick-render".into())
            .spawn(move || {
                if let Err(e) = run_render_thread(
                    player_for_thread,
                    surface_for_thread,
                    signal_for_thread,
                ) {
                    eprintln!("[unflick-render] thread exited with error: {e}");
                }
            })
            .map_err(|e| anyhow!("spawn render thread: {e}"))?;

        Ok(Self {
            join: Some(join),
            signal,
            surface,
            _player: player,
        })
    }

    /// Resize / move the underlying native widget. Safe from any thread.
    pub fn set_geometry(&self, x: i32, y: i32, w: i32, h: i32) -> Result<()> {
        self.surface.set_geometry(x, y, w, h)?;
        // Force a repaint so the resize takes effect even if the file is
        // paused (otherwise the new framebuffer stays black until next frame).
        if let Ok(mut state) = self.signal.state.lock() {
            state.redraw = true;
            self.signal.cv.notify_all();
        }
        Ok(())
    }

    /// Show / hide the video surface. Used during PiP transitions.
    pub fn set_visible(&self, visible: bool) {
        self.surface.set_visible(visible);
    }

    /// Stop the render thread and wait for it to finish. Idempotent.
    pub fn shutdown(&mut self) {
        if let Ok(mut state) = self.signal.state.lock() {
            state.shutdown = true;
            self.signal.cv.notify_all();
        }
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

impl Drop for RenderLoop {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn run_render_thread(
    _player: Arc<Player>,
    surface: Arc<dyn VideoSurface>,
    signal: Arc<RenderSignal>,
) -> Result<()> {
    // GL context affinity: this is the only thread that ever calls
    // make_current / render / swap_buffers on this surface.
    surface
        .make_current()
        .map_err(|e| anyhow!("make_current at render-thread start: {e}"))?;

    // Build the C-ABI-friendly pointer mpv hands back to get_proc_address.
    // The render context retains this pointer for its whole lifetime so the
    // box must live at least that long. We Box it here on the heap and hold
    // the raw pointer; the box is dropped at function exit (after ctx).
    let surface_ref: &dyn VideoSurface = surface.as_ref();
    let surface_box: Box<&dyn VideoSurface> = Box::new(surface_ref);
    let surface_ctx: *mut c_void = Box::into_raw(surface_box) as *mut c_void;

    let ctx = MpvRenderContext::new_opengl(
        _player.mpv_handle(),
        get_proc_address_trampoline,
        surface_ctx,
    )
    .map_err(|e| anyhow!("create render context: {e}"))?;

    // Wire up mpv's "frame ready" notifier. We Arc::into_raw to get a stable
    // pointer for mpv's void* slot; the matching from_raw on exit reclaims
    // it without leaking.
    let signal_ptr = Arc::into_raw(Arc::clone(&signal)) as *mut c_void;
    let cb: MpvRenderUpdateFn = update_callback_trampoline;
    ctx.set_update_callback(cb, signal_ptr);

    loop {
        // Wait for either a frame, a forced redraw, or shutdown.
        let mut state = signal
            .state
            .lock()
            .map_err(|_| anyhow!("render signal poisoned"))?;
        while !state.frame_ready && !state.redraw && !state.shutdown {
            state = signal
                .cv
                .wait(state)
                .map_err(|_| anyhow!("render signal poisoned"))?;
        }
        if state.shutdown {
            break;
        }
        let did_frame = state.frame_ready;
        let did_redraw = state.redraw;
        state.frame_ready = false;
        state.redraw = false;
        drop(state);
        let _ = (did_frame, did_redraw); // Will use for diagnostics later.

        let (w, h) = surface.size();
        if let Err(e) = ctx.render_to_fbo(0, w, h) {
            // A single-frame failure shouldn't kill the loop — log and try
            // the next signal. Logs to stderr since console is detached on
            // Windows release builds; will surface in attached terminals
            // (CLI mode) or via Tauri devtools (dev builds).
            eprintln!("[unflick-render] render_to_fbo: {e}");
            continue;
        }
        if let Err(e) = surface.swap_buffers() {
            eprintln!("[unflick-render] swap_buffers: {e}");
        }
    }

    // Ordered teardown: drop the render context first (must run on the GL
    // thread per mpv docs), then reclaim the heap-allocated callback ctx and
    // get-proc-address ctx.
    drop(ctx);
    unsafe {
        // Reclaim the Arc we leaked into mpv's callback slot.
        let _ = Arc::from_raw(signal_ptr as *const RenderSignal);
        // Reclaim the box wrapping the surface reference.
        let _ = Box::from_raw(surface_ctx as *mut &dyn VideoSurface);
    }

    Ok(())
}
