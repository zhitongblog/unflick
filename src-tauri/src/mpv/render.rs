//! Safe Rust wrapper around `mpv_render_context_*`.
//!
//! mpv's render-context API lets us drive video decoding through libmpv but
//! own the GL context ourselves. We pass mpv a `get_proc_address` callback at
//! init time, then on every frame mpv tells us via `update_callback` that a
//! new frame is ready, and we call `render()` from the GL thread to paint it
//! into our framebuffer.
//!
//! This is the cross-platform path used in v0.8: same code runs on Windows
//! (ANGLE → D3D11), macOS (CGL → Metal under the hood), and Linux (GLX/EGL).
//! No platform-specific render backend needed in Rust — that lives in the
//! GL context creation, which is per-platform.

use std::ffi::CString;
use std::os::raw::{c_char, c_int, c_void};
use std::ptr;
use std::sync::Arc;

use anyhow::{bail, Result};

use super::ffi::{
    self, MpvApi, MpvOpenGlFbo, MpvOpenGlInitParams, MpvRenderCtx, MpvRenderParam,
    MpvRenderUpdateFn,
};
use super::MpvHandle;

/// Caller-supplied lookup that resolves a GL function name to a pointer.
/// Implemented per-platform: WGL on Windows, NSOpenGLContext::getProcAddress
/// on macOS, glXGetProcAddress on Linux/X11, eglGetProcAddress on Wayland.
pub type GetProcAddrFn =
    unsafe extern "C" fn(ctx: *mut c_void, name: *const c_char) -> *mut c_void;

/// Safe wrapper around an mpv_render_context. Owns the underlying mpv
/// resource and frees it on drop.
///
/// IMPORTANT: every method that touches GL state — that's everything except
/// `set_update_callback` — must run on the thread that owns the GL context
/// passed at construction. mpv asserts this internally and will deadlock
/// otherwise.
pub struct MpvRenderContext {
    api: Arc<MpvApi>,
    ctx: MpvRenderCtx,
}

// The render context is internally thread-safe for the call patterns we use:
// `update_callback` fires from mpv's worker thread, render() runs on the GL
// thread. mpv guarantees this is sound.
unsafe impl Send for MpvRenderContext {}
unsafe impl Sync for MpvRenderContext {}

impl MpvRenderContext {
    /// Bind a new render context to `handle`'s mpv instance, using the
    /// caller-provided GL function loader.
    ///
    /// `get_proc_address_ctx` is passed back unchanged to `get_proc_address`
    /// on each lookup — it's the user-data slot for the loader (typically the
    /// HDC / NSOpenGLContext / EGLDisplay).
    pub fn new_opengl(
        handle: &MpvHandle,
        get_proc_address: GetProcAddrFn,
        get_proc_address_ctx: *mut c_void,
    ) -> Result<Self> {
        // Keep the api/type strings alive across the FFI call — params hold
        // raw pointers into them.
        let api_type = CString::new("opengl").unwrap();

        let mut init_params = MpvOpenGlInitParams {
            get_proc_address: Some(get_proc_address),
            get_proc_address_ctx,
        };

        let mut params = [
            MpvRenderParam {
                type_: ffi::MPV_RENDER_PARAM_API_TYPE,
                data: api_type.as_ptr() as *mut c_void,
            },
            MpvRenderParam {
                type_: ffi::MPV_RENDER_PARAM_OPENGL_INIT_PARAMS,
                data: &mut init_params as *mut _ as *mut c_void,
            },
            MpvRenderParam {
                type_: ffi::MPV_RENDER_PARAM_INVALID,
                data: ptr::null_mut(),
            },
        ];

        let api = handle.api_arc();
        let mpv_ctx = handle.raw_ctx();
        let mut ctx: MpvRenderCtx = ptr::null_mut();

        let err = unsafe {
            (api.render_context_create)(
                &mut ctx as *mut MpvRenderCtx,
                mpv_ctx,
                params.as_mut_ptr(),
            )
        };
        if err < 0 {
            bail!("mpv_render_context_create failed: code {}", err);
        }
        if ctx.is_null() {
            bail!("mpv_render_context_create returned null context");
        }

        Ok(Self { api, ctx })
    }

    /// Render the current frame to a framebuffer. `fbo = 0` selects the GL
    /// context's default framebuffer (i.e. paint directly to the window).
    /// `(w, h)` must match the framebuffer's actual dimensions in pixels.
    pub fn render_to_fbo(&self, fbo: c_int, w: c_int, h: c_int) -> Result<()> {
        let mut fbo_param = MpvOpenGlFbo {
            fbo,
            w,
            h,
            internal_format: 0, // 0 = let mpv guess (works for default FB).
        };
        // Most platforms render top-down to the default framebuffer, but mpv
        // assumes OpenGL's bottom-up convention by default. Flip Y so frames
        // come out the right way up. Set to 0 if you're rendering to a texture
        // you'll later sample yourself with the inverse mapping.
        let mut flip_y: c_int = 1;

        let mut params = [
            MpvRenderParam {
                type_: ffi::MPV_RENDER_PARAM_OPENGL_FBO,
                data: &mut fbo_param as *mut _ as *mut c_void,
            },
            MpvRenderParam {
                type_: ffi::MPV_RENDER_PARAM_FLIP_Y,
                data: &mut flip_y as *mut _ as *mut c_void,
            },
            MpvRenderParam {
                type_: ffi::MPV_RENDER_PARAM_INVALID,
                data: ptr::null_mut(),
            },
        ];

        let err = unsafe { (self.api.render_context_render)(self.ctx, params.as_mut_ptr()) };
        if err < 0 {
            bail!("mpv_render_context_render failed: code {}", err);
        }
        Ok(())
    }

    /// Register a callback fired from mpv's internal thread whenever a new
    /// frame is ready. The callback should be CHEAP — typically just signal a
    /// condvar / post a window message that schedules a render on the GL
    /// thread. Calling render() directly from this callback is forbidden.
    pub fn set_update_callback(&self, cb: MpvRenderUpdateFn, ctx: *mut c_void) {
        unsafe {
            (self.api.render_context_set_update_callback)(self.ctx, Some(cb), ctx);
        }
    }

    /// Poll the render context for state changes. Returns flags from the
    /// MPV_RENDER_UPDATE_* family. We mostly drive renders via the update
    /// callback, but this is useful from the GL thread to check whether a
    /// new frame is ready before scheduling a paint.
    pub fn poll(&self) -> u64 {
        unsafe { (self.api.render_context_update)(self.ctx) }
    }
}

impl Drop for MpvRenderContext {
    fn drop(&mut self) {
        // Must run on the GL thread (mpv documents this). We trust callers
        // to drop the context from the right place.
        unsafe { (self.api.render_context_free)(self.ctx) };
    }
}
