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
    /// Set true once the mpv render context exists and its update callback
    /// is wired. Until then mpv has a `vo=libmpv` with nothing behind it,
    /// and `loadfile` ends the file instead of playing it — so anything
    /// wanting to open something has to wait for this first.
    ready: bool,
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
    /// Last geometry the frontend pushed (client coords + size). Cached so
    /// `refresh_geometry` can re-apply when the owner window moves
    /// (which doesn't change the client-relative rect, but the popup
    /// surface needs new screen coords).
    last_geometry: Mutex<Option<(i32, i32, i32, i32)>>,
    /// Held to keep the mpv handle alive across the loop. The render thread
    /// borrows it via Arc::clone.
    _player: Arc<Player>,
}

impl RenderLoop {
    pub fn start(player: Arc<Player>, surface: Arc<dyn VideoSurface>) -> Result<Self> {
        let signal = Arc::new(RenderSignal {
            state: Mutex::new(RenderState {
                ready: false,
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
            last_geometry: Mutex::new(None),
            _player: player,
        })
    }

    /// Linux-only constructor: the surface is owned by this loop for
    /// geometry forwarding (set_geometry / refresh_geometry / set_visible),
    /// but no render thread runs. mpv was created with `vo=x11 wid=<xid>`
    /// in lib.rs::bring_up_video_pipeline and renders into the X11 child
    /// window by itself via XPutImage — bypassing GL entirely so we work
    /// on every X11 system, including software-rendered virtual machines
    /// where llvmpipe-via-Composite leaves the window black.
    #[cfg(target_os = "linux")]
    pub fn start_passive(player: Arc<Player>, surface: Arc<dyn VideoSurface>) -> Self {
        let signal = Arc::new(RenderSignal {
            state: Mutex::new(RenderState {
                // No render thread here: mpv draws into the X11 child
                // window itself, so there is nothing to become ready.
                ready: true,
                frame_ready: false,
                redraw: false,
                shutdown: false,
            }),
            cv: Condvar::new(),
        });
        Self {
            join: None,
            signal,
            surface,
            last_geometry: Mutex::new(None),
            _player: player,
        }
    }

    /// Resize / move the underlying native widget. Safe from any thread.
    /// Caches the rect so the backend can replay it on owner-window moves
    /// (which don't fire the frontend's ResizeObserver).
    /// Block until mpv can be given a file, or until `timeout` elapses.
    ///
    /// Returns whether it became ready. A false is worth acting on but not
    /// worth refusing to play over — the caller tries anyway and lets mpv
    /// give the real answer.
    pub fn wait_ready(&self, timeout: std::time::Duration) -> bool {
        let Ok(state) = self.signal.state.lock() else {
            return false;
        };
        match self
            .signal
            .cv
            .wait_timeout_while(state, timeout, |s| !s.ready && !s.shutdown)
        {
            Ok((s, _)) => s.ready,
            Err(_) => false,
        }
    }

    pub fn set_geometry(&self, x: i32, y: i32, w: i32, h: i32) -> Result<()> {
        self.surface.set_geometry(x, y, w, h)?;
        if let Ok(mut g) = self.last_geometry.lock() {
            *g = Some((x, y, w, h));
        }
        if let Ok(mut state) = self.signal.state.lock() {
            state.redraw = true;
            self.signal.cv.notify_all();
        }
        Ok(())
    }

    /// Re-apply the most-recently-set geometry. Used when the Tauri main
    /// window moves: the client-coord rect is unchanged but the popup's
    /// screen coords need to slide along with the owner. No-op before the
    /// frontend has called set_geometry at least once.
    pub fn refresh_geometry(&self) -> Result<()> {
        let cached = match self.last_geometry.lock() {
            Ok(g) => *g,
            Err(_) => None,
        };
        if let Some((x, y, w, h)) = cached {
            self.surface.set_geometry(x, y, w, h)?;
            if let Ok(mut state) = self.signal.state.lock() {
                state.redraw = true;
                self.signal.cv.notify_all();
            }
        }
        Ok(())
    }

    /// Show / hide the video surface. Used during PiP transitions.
    pub fn set_visible(&self, visible: bool) {
        self.surface.set_visible(visible);
    }

    /// Pin or unpin the video surface above all other windows. Required
    /// to track the main window's always-on-top setting — Win32's owner
    /// relationship handles z-order *within* a normal app stack but
    /// doesn't automatically promote owned popups when the owner
    /// becomes topmost.
    pub fn set_always_on_top(&self, enabled: bool) {
        self.surface.set_always_on_top(enabled);
    }

    /// Fade the surface to a given alpha. The frontend uses this to make
    /// in-app menus / dialogs readable when they're rendered behind the
    /// popup — lowering α lets them show through.
    pub fn set_alpha(&self, alpha: u8) {
        self.surface.set_alpha(alpha);
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
    eprintln!("[unflick-render] render context created");

    // Wire up mpv's "frame ready" notifier. We Arc::into_raw to get a stable
    // pointer for mpv's void* slot; the matching from_raw on exit reclaims
    // it without leaking.
    let signal_ptr = Arc::into_raw(Arc::clone(&signal)) as *mut c_void;
    let cb: MpvRenderUpdateFn = update_callback_trampoline;
    ctx.set_update_callback(cb, signal_ptr);
    eprintln!("[unflick-render] update callback wired, entering loop");

    // mpv can be handed a file from here on. Whoever is waiting to open the
    // one named on the command line is released by this.
    if let Ok(mut st) = signal.state.lock() {
        st.ready = true;
    }
    signal.cv.notify_all();

    let mut frames_rendered: u64 = 0;
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
            eprintln!("[unflick-render] render_to_fbo: {e}");
            continue;
        }
        if let Err(e) = surface.swap_buffers() {
            eprintln!("[unflick-render] swap_buffers: {e}");
            continue;
        }
        frames_rendered += 1;
        // Log every 30 frames so we can tell whether the loop is alive
        // without flooding the log when playback is steady.
        if frames_rendered == 1 || frames_rendered % 30 == 0 {
            eprintln!(
                "[unflick-render] frame #{frames_rendered} rendered ({}x{})",
                w, h
            );
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
