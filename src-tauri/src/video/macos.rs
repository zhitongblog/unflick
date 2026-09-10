//! macOS VideoSurface: embed an NSView beneath the WKWebView and let mpv
//! render into it via a CGL OpenGL context.
//!
//! Architecture differs from Windows on purpose:
//!   - Windows: a *separate top-level* WS_POPUP owned by the Tauri window,
//!     stacked above the WebView, because WebView2's DComp surface paints
//!     over sibling child HWNDs regardless of Z-order.
//!   - macOS: a *subview of the main window's contentView*, inserted
//!     **below** the WKWebView. NSView z-order respects insertion order,
//!     so the WKWebView renders on top and we just need WKWebView's
//!     background to be transparent for the video to show through.
//!
//! This means we don't fight the compositor on macOS — no layered window,
//! no top-level popup, no click-through hack. Mouse events go to the
//! WKWebView naturally because it's the front-most subview at that rect.
//!
//! GL context: glutin's CGL backend talks to Apple's OpenGL stack
//! (deprecated since 10.14 but still functional on macOS 26 in 2026).
//! We're using legacy GL on purpose — mpv's gpu/opengl path is the most
//! portable backend and matches the Windows WGL setup.

#![cfg(target_os = "macos")]

use anyhow::{anyhow, Result};
use std::ffi::{c_void, CString};
use std::ptr;
use std::sync::Mutex;

use glutin::config::ConfigTemplateBuilder;
use glutin::context::{
    AsRawContext, ContextApi, ContextAttributesBuilder, NotCurrentContext, NotCurrentGlContext,
    PossiblyCurrentContext, PossiblyCurrentGlContext, RawContext,
};
use glutin::display::{Display, DisplayApiPreference, GlDisplay};
use glutin::surface::{GlSurface, Surface, SurfaceAttributesBuilder, SwapInterval, WindowSurface};
use objc2::rc::Retained;
use objc2::{msg_send, MainThreadOnly};
use objc2_app_kit::{NSView, NSWindowOrderingMode};
use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize};
use raw_window_handle::{
    AppKitDisplayHandle, AppKitWindowHandle, RawDisplayHandle, RawWindowHandle,
};

use super::VideoSurface;

// CGL's per-context lock. This is the only mutual exclusion Apple's GL
// stack offers between a thread *rendering into* a context and a thread
// *reconfiguring the drawable behind* that context — and reconfiguring
// the drawable is exactly what a geometry change is. Recursive from the
// same thread, so nesting a lock inside a lock is fine.
#[link(name = "OpenGL", kind = "framework")]
extern "C" {
    fn CGLLockContext(ctx: *mut c_void) -> i32;
    fn CGLUnlockContext(ctx: *mut c_void) -> i32;
}

/// Holds the CGL context lock for a scope. A raw pointer rather than a
/// borrow of the surface so the render loop can take it around a whole
/// frame without borrowing anything that a resize also wants.
struct CglLock(*mut c_void);

impl CglLock {
    fn new(ctx: *mut c_void) -> Self {
        unsafe { CGLLockContext(ctx) };
        Self(ctx)
    }
}

impl Drop for CglLock {
    fn drop(&mut self) {
        unsafe { CGLUnlockContext(self.0) };
    }
}

/// Same NotCurrent → Current lifecycle dance as the Windows impl.
/// CGL contexts are also thread-affine, so binding happens lazily on the
/// render thread.
enum ContextSlot {
    NotCurrent(NotCurrentContext),
    Current(PossiblyCurrentContext),
}

pub struct MacosVideoSurface {
    /// Our child view, retained via objc2's Retained smart pointer.
    /// Dropping this releases the underlying NSView.
    child: Retained<NSView>,
    /// The Tauri window's contentView — borrowed (we don't own it).
    /// Stored so set_geometry can convert client-coord rects we get from
    /// the frontend into the parent's coordinate space.
    parent: Retained<NSView>,
    context: Mutex<Option<ContextSlot>>,
    /// The CGLContextObj inside glutin's NSOpenGLContext, cached at
    /// construction. Cached because both sides of the race have to be able
    /// to take the context lock *without* going through `context` above —
    /// the Mutex is a Rust-side lock over the slot, not over the GL state,
    /// and `render_to_fbo` never touches it at all.
    cgl: *mut c_void,
    surface: Surface<WindowSurface>,
    display: Display,
    /// Tracked HWND-equivalent size — same rationale as WindowsVideoSurface:
    /// glutin's Surface::width/height keeps reporting the creation size on
    /// CGL, so we track ourselves to feed mpv the right FBO viewport.
    cur_w: std::sync::atomic::AtomicI32,
    cur_h: std::sync::atomic::AtomicI32,
}

unsafe impl Send for MacosVideoSurface {}
unsafe impl Sync for MacosVideoSurface {}

impl MacosVideoSurface {
    /// `parent_ns_view` is the `*mut c_void` we get back from
    /// `tauri::WebviewWindow::ns_view()`. Caller MUST run this on the main
    /// (UI) thread — Cocoa view operations are not thread-safe.
    pub fn new(parent_ns_view: *mut c_void, w: i32, h: i32) -> Result<Self> {
        // SAFETY: caller guarantees this runs on the main thread (we're
        // invoked from Tauri's setup hook, which runs on main). objc2's
        // safe alternative returns Option<MainThreadMarker> via TLS check
        // but it's not exposed in objc2-foundation 0.2 — new_unchecked is.
        let mtm = unsafe { MainThreadMarker::new_unchecked() };

        // Wrap the raw NSView pointer Tauri gave us in objc2's typed handle.
        // We *do not* take ownership of Tauri's view — Retained::retain
        // adds a +1 retain so the parent stays alive while our struct
        // does, and our drop releases it.
        let parent: Retained<NSView> = unsafe {
            let raw = parent_ns_view as *mut NSView;
            Retained::retain(raw)
                .ok_or_else(|| anyhow!("parent NSView pointer was null"))?
        };

        // Create the child NSView at the parent's full size. set_geometry
        // will reposition immediately when the frontend's ResizeObserver
        // first fires.
        let child: Retained<NSView> = unsafe {
            let frame = NSRect::new(
                NSPoint::new(0.0, 0.0),
                NSSize::new(w.max(1) as f64, h.max(1) as f64),
            );
            let alloc = NSView::alloc(mtm);
            NSView::initWithFrame(alloc, frame)
        };

        // Insert below all existing subviews so the WKWebView (added later
        // by Tauri's WKWebViewController, or already-present) stays on top.
        // NSWindowOrderingMode::Below + nil reference puts us at the bottom.
        // objc2-app-kit 0.2 doesn't surface this multi-arg selector as a
        // typed method, so we drop down to msg_send! directly.
        unsafe {
            let nil_view: *const NSView = ptr::null();
            let _: () = msg_send![
                &*parent,
                addSubview: &*child,
                positioned: NSWindowOrderingMode::Below,
                relativeTo: nil_view,
            ];
        }

        // Hand glutin the child's NSView pointer so it builds a CGL
        // context that targets it.
        let child_ptr: *mut NSView = Retained::as_ptr(&child) as *mut NSView;
        let win_handle = AppKitWindowHandle::new(
            std::ptr::NonNull::new(child_ptr as *mut std::ffi::c_void)
                .ok_or_else(|| anyhow!("child NSView pointer was null"))?,
        );
        let raw_window = RawWindowHandle::AppKit(win_handle);
        let raw_display = RawDisplayHandle::AppKit(AppKitDisplayHandle::new());

        let display = unsafe {
            Display::new(raw_display, DisplayApiPreference::Cgl)
                .map_err(|e| anyhow!("create CGL display: {e}"))?
        };

        let template = ConfigTemplateBuilder::new()
            .with_alpha_size(8)
            .compatible_with_native_window(raw_window)
            .build();

        let config = unsafe {
            display
                .find_configs(template)
                .map_err(|e| anyhow!("find_configs: {e}"))?
                .next()
                .ok_or_else(|| anyhow!("no compatible CGL config found"))?
        };

        let context_attrs = ContextAttributesBuilder::new()
            // Same as Windows: don't pin a GL version, let the driver pick
            // what mpv asks for at runtime.
            .with_context_api(ContextApi::OpenGl(None))
            .build(Some(raw_window));

        let not_current = unsafe {
            display
                .create_context(&config, &context_attrs)
                .map_err(|e| anyhow!("create_context: {e}"))?
        };

        // Dig the CGLContextObj out now, while we still have a concrete
        // context type in hand. `raw_context()` hands back the
        // NSOpenGLContext; the CGL object under it is what the lock is
        // keyed on. Failing here rather than later: without this pointer
        // there is no way to serialise a resize against a render, and a
        // surface that cannot do that is one that segfaults on its first
        // window resize.
        let cgl: *mut c_void = unsafe {
            let RawContext::Cgl(ns_ctx) = not_current.raw_context();
            let ns_ctx = ns_ctx as *const objc2::runtime::AnyObject;
            let ns_ctx = ns_ctx
                .as_ref()
                .ok_or_else(|| anyhow!("glutin's NSOpenGLContext pointer was null"))?;
            let cgl: *mut c_void = msg_send![ns_ctx, CGLContextObj];
            if cgl.is_null() {
                return Err(anyhow!("NSOpenGLContext has no CGLContextObj"));
            }
            cgl
        };

        let surface_attrs = SurfaceAttributesBuilder::<WindowSurface>::new().build(
            raw_window,
            std::num::NonZeroU32::new(w.max(1) as u32).unwrap(),
            std::num::NonZeroU32::new(h.max(1) as u32).unwrap(),
        );
        let surface = unsafe {
            display
                .create_window_surface(&config, &surface_attrs)
                .map_err(|e| anyhow!("create_window_surface: {e}"))?
        };

        Ok(Self {
            child,
            parent,
            display,
            surface,
            cgl,
            context: Mutex::new(Some(ContextSlot::NotCurrent(not_current))),
            cur_w: std::sync::atomic::AtomicI32::new(w.max(1)),
            cur_h: std::sync::atomic::AtomicI32::new(h.max(1)),
        })
    }
}

impl VideoSurface for MacosVideoSurface {
    fn lock_gl(&self) {
        unsafe { CGLLockContext(self.cgl) };
    }

    fn unlock_gl(&self) {
        unsafe { CGLUnlockContext(self.cgl) };
    }

    fn make_current(&self) -> Result<()> {
        // Deliberately *not* under the context lock. glutin's make_current
        // is `[ctx update]` + `setView:`, and it dispatches both to the
        // main thread — while `[NSOpenGLContext update]` takes the context
        // lock itself. Holding the lock here and then waiting on a main
        // thread that is about to ask for it hangs the render thread
        // before its first frame; measured, not theorised.
        let mut guard = self
            .context
            .lock()
            .map_err(|_| anyhow!("video surface mutex poisoned"))?;
        match guard.take() {
            Some(ContextSlot::NotCurrent(ctx)) => {
                let now = ctx
                    .make_current(&self.surface)
                    .map_err(|e| anyhow!("make_current (first): {e}"))?;
                // Vsync: same rationale as Windows — cap SwapBuffers to
                // display refresh so we don't outrun the compositor.
                if let Err(e) = self.surface.set_swap_interval(
                    &now,
                    SwapInterval::Wait(std::num::NonZeroU32::new(1).unwrap()),
                ) {
                    eprintln!("[unflick-render] set_swap_interval failed (continuing): {e}");
                }
                *guard = Some(ContextSlot::Current(now));
                Ok(())
            }
            Some(ContextSlot::Current(ctx)) => {
                ctx.make_current(&self.surface)
                    .map_err(|e| anyhow!("make_current: {e}"))?;
                *guard = Some(ContextSlot::Current(ctx));
                Ok(())
            }
            None => Err(anyhow!("context already released")),
        }
    }

    fn get_proc_address(&self, name: &str) -> *mut c_void {
        let cname = match CString::new(name) {
            Ok(c) => c,
            Err(_) => return ptr::null_mut(),
        };
        self.display.get_proc_address(cname.as_c_str()) as *mut c_void
    }

    fn set_geometry(&self, x: i32, y: i32, w: i32, h: i32) -> Result<()> {
        // Everything below reconfigures the drawable the GL context is
        // painting into: setFrame resizes the surface AppKit hands the
        // renderer, and glutin's `resize` is `[NSOpenGLContext update]`.
        // All of it lands on the AppKit main thread, which is where the
        // frontend's `video_surface_set_geometry` runs — while the render
        // thread is inside `mpv_render_context_render` with no idea any of
        // it is happening. That is how the renderer ends up walking a
        // GLDContextRec resource list that was freed underneath it: a
        // SIGSEGV in AppleMetalOpenGLRenderer that took 1 of 10 idle
        // launches on the single geometry push the frontend makes when it
        // mounts, and 11 of 11 when the pushes come every frame. (Both
        // with the screen unlocked. Locked, the race narrows to about 1 in
        // 6 — the machine's answer depends on whether anyone is looking at
        // it.) Hold the context lock across the whole reconfiguration so
        // an in-flight frame finishes first.
        //
        // Only from the main thread, and not just by Cocoa's rule about
        // views: glutin's `resize` dispatches *to* the main thread, so a
        // lock held across it from anywhere else deadlocks against the
        // lock `[NSOpenGLContext update]` takes for itself. Every caller
        // is on main — Tauri runs non-async commands there, and so does
        // the window-event hook behind `refresh_geometry` — so the branch
        // is an assertion about that, not a fallback for it.
        let _lock = MainThreadMarker::new().map(|_| CglLock::new(self.cgl));
        // The frontend gives us coords in the WebView's client space (y
        // measured from the top, like CSS). Cocoa's default coord system
        // is flipped — y from the bottom — so we have to translate.
        unsafe {
            let parent_bounds = self.parent.bounds();
            let parent_h = parent_bounds.size.height;
            let cocoa_y = parent_h - (y as f64) - (h.max(1) as f64);

            let frame = NSRect::new(
                NSPoint::new(x as f64, cocoa_y),
                NSSize::new(w.max(1) as f64, h.max(1) as f64),
            );
            self.child.setFrame(frame);

            // Track the new size so size() returns it (mpv letterboxes
            // against the size we report).
            use std::sync::atomic::Ordering;
            self.cur_w.store(w.max(1), Ordering::Relaxed);
            self.cur_h.store(h.max(1), Ordering::Relaxed);

            // Resize the GL drawable to match. Keeps the FBO pixel size
            // in sync with the view's bounds (no-op on macOS / WGL but
            // harmless).
            if let Ok(guard) = self.context.lock() {
                if let Some(ContextSlot::Current(ctx)) = guard.as_ref() {
                    let nw = std::num::NonZeroU32::new(w.max(1) as u32).unwrap();
                    let nh = std::num::NonZeroU32::new(h.max(1) as u32).unwrap();
                    self.surface.resize(ctx, nw, nh);
                }
            }
        }
        Ok(())
    }

    fn set_visible(&self, visible: bool) {
        // setHidden:YES removes from rendering but keeps the view in the
        // hierarchy, so we don't have to re-add on show.
        unsafe {
            self.child.setHidden(!visible);
        }
    }

    fn set_alpha(&self, alpha: u8) {
        unsafe {
            self.child.setAlphaValue((alpha as f64) / 255.0);
        }
    }

    fn set_always_on_top(&self, _enabled: bool) {
        // No-op on macOS — always-on-top is a property of the *window*,
        // not the view. Tauri's `set_always_on_top` already handles the
        // NSWindow.level change, and our subview rides along inside it.
    }

    fn size(&self) -> (i32, i32) {
        use std::sync::atomic::Ordering;
        (
            self.cur_w.load(Ordering::Relaxed),
            self.cur_h.load(Ordering::Relaxed),
        )
    }

    fn swap_buffers(&self) -> Result<()> {
        // No lock of its own: the render loop already holds the context
        // lock across render-then-swap, and it is the only caller.
        let guard = self
            .context
            .lock()
            .map_err(|_| anyhow!("video surface mutex poisoned"))?;
        match guard.as_ref() {
            Some(ContextSlot::Current(ctx)) => self
                .surface
                .swap_buffers(ctx)
                .map_err(|e| anyhow!("swap_buffers: {e}")),
            Some(ContextSlot::NotCurrent(_)) => {
                Err(anyhow!("swap_buffers before first make_current"))
            }
            None => Err(anyhow!("context already released")),
        }
    }
}

impl Drop for MacosVideoSurface {
    fn drop(&mut self) {
        unsafe {
            self.child.removeFromSuperview();
        }
    }
}
