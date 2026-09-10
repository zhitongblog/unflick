//! Embedded video surface — the native widget that mpv renders into.
//!
//! Architecture: the Tauri main window hosts a transparent WebView for all
//! UI chrome (title bar, controls, dialogs, panels). Beneath that WebView,
//! we own a native child surface (HWND on Windows, NSView on macOS,
//! GtkWidget on Linux) where libmpv paints decoded video through an
//! OpenGL context we provide.
//!
//! The trait [`VideoSurface`] hides per-platform widget plumbing. Each
//! impl in this module's submodules creates the child widget, attaches a
//! GL context to it, and exposes the lifecycle hooks the renderer thread
//! needs (make_current / swap / resize / show-hide).

use anyhow::Result;
use std::ffi::c_void;

#[cfg(target_os = "windows")]
pub mod windows;

#[cfg(target_os = "macos")]
pub mod macos;

#[cfg(target_os = "linux")]
pub mod linux;

/// Opaque host that owns a native child widget and a GL context bound to it.
/// All GL-touching methods must be called on the dedicated render thread the
/// surface was created on; `set_geometry` and `set_visible` are safe from any
/// thread (they cross to the main / UI thread internally where required).
pub trait VideoSurface: Send + Sync {
    /// Make this surface's GL context current on the calling thread.
    /// Required before mpv_render_context_render() or any GL state change.
    fn make_current(&self) -> Result<()>;

    /// Take / release whatever lock keeps a frame's GL work from
    /// overlapping a change to the drawable it is painting into.
    ///
    /// Only macOS needs this, and it needs it badly: `set_geometry` there
    /// resizes the NSView and calls `[NSOpenGLContext update]`, both on
    /// the AppKit main thread, both of which reallocate the renderer's
    /// attachments. A render already in flight on the render thread then
    /// walks a resource list that no longer exists — an intermittent
    /// SIGSEGV inside AppleMetalOpenGLRenderer, hit in practice on the
    /// frontend's very first geometry push. macOS holds CGL's context
    /// lock across both sides so one waits for the other.
    ///
    /// Windows and Linux move their child widget without touching the GL
    /// context at all, so the default is a pair of no-ops. Always paired;
    /// the render loop takes them through an RAII guard.
    fn lock_gl(&self) {}

    /// Release what [`VideoSurface::lock_gl`] took.
    fn unlock_gl(&self) {}

    /// Resolve a GL function pointer. Used as mpv's get_proc_address callback.
    /// Caller must hold a current context.
    fn get_proc_address(&self, name: &str) -> *mut c_void;

    /// Move + resize the child widget within its parent in physical pixels.
    /// Called when the WebView's video region reflows (resize, panel slide-in,
    /// fullscreen toggle).
    fn set_geometry(&self, x: i32, y: i32, w: i32, h: i32) -> Result<()>;

    /// Show or hide the child widget. Used during PiP transitions and when
    /// the WebView covers the whole window with a modal dialog.
    fn set_visible(&self, visible: bool);

    /// Cut rectangles out of the surface so WebView chrome drawn *under*
    /// it can show through. Rects are in the same logical, window-client
    /// coordinate space as [`set_geometry`]; an empty slice restores the
    /// whole surface.
    ///
    /// This is what makes fullscreen fullscreen on Windows, where the
    /// surface is a top-level popup above the WebView: the control bar
    /// has to punch a hole rather than push the video out of the way,
    /// because shrinking the surface makes mpv re-letterbox and the
    /// picture visibly jumps every time the mouse moves. Cutting a hole
    /// leaves the surface — and so mpv's viewport — exactly where it was.
    ///
    /// Default: no-op. macOS and Linux host the surface *below* the
    /// WebView, so chrome drawn over the video is already visible.
    fn set_exclusions(&self, _rects: &[(i32, i32, i32, i32)]) -> Result<()> {
        Ok(())
    }

    /// Pin / unpin the surface above all other windows. Mirrors the main
    /// window's always-on-top toggle so the video stays attached.
    /// Default: no-op (platform stub).
    fn set_always_on_top(&self, _enabled: bool) {}

    /// Set the surface's overall opacity (0–255). 255 is fully opaque,
    /// 0 is fully transparent. Used to fade the video popup when an
    /// in-app menu or dialog opens — the menu is rendered in the
    /// WebView *below* the popup in z-order, so lowering the popup
    /// alpha lets the menu show through without hiding the video
    /// completely. Default: no-op (platform stub).
    fn set_alpha(&self, _alpha: u8) {}

    /// Current framebuffer size in physical pixels — pass into mpv's FBO param.
    fn size(&self) -> (i32, i32);

    /// Push the current framebuffer to the screen. Wraps SwapBuffers / etc.
    /// Caller must hold a current context.
    fn swap_buffers(&self) -> Result<()>;
}

/// Trampoline that adapts a `&dyn VideoSurface` to the C ABI mpv expects for
/// `get_proc_address`. The `ctx` is set to `Box::into_raw(Box::new(&surface))`
/// at render-context construction time and reclaimed when the surface drops.
///
/// SAFETY: caller guarantees ctx is a `*const &dyn VideoSurface` that outlives
/// the render context.
pub unsafe extern "C" fn get_proc_address_trampoline(
    ctx: *mut c_void,
    name: *const std::os::raw::c_char,
) -> *mut c_void {
    if ctx.is_null() || name.is_null() {
        return std::ptr::null_mut();
    }
    let surface = unsafe { &*(ctx as *const &dyn VideoSurface) };
    let cstr = unsafe { std::ffi::CStr::from_ptr(name) };
    match cstr.to_str() {
        Ok(s) => surface.get_proc_address(s),
        Err(_) => std::ptr::null_mut(),
    }
}
