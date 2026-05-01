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
