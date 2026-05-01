use std::ffi::{c_void, CStr, CString};
use std::ptr;
use std::sync::Arc;

use anyhow::{bail, Result};

use super::ffi::{self, MpvApi};

/// Safe wrapper around a raw mpv handle + dynamically loaded API.
pub struct MpvHandle {
    api: Arc<MpvApi>,
    ctx: ffi::MpvCtx,
}

// mpv is thread-safe via its own internal locking
unsafe impl Send for MpvHandle {}
unsafe impl Sync for MpvHandle {}

impl MpvHandle {
    /// Create and initialize a new mpv instance.
    /// `vo` controls video output: "null" for headless, "gpu" for GUI.
    pub fn new(vo: &str) -> Result<Self> {
        let api = Arc::new(MpvApi::load().map_err(|e| anyhow::anyhow!(e))?);

        let ctx = unsafe { (api.create)() };
        if ctx.is_null() {
            bail!("mpv_create returned null");
        }

        let handle = Self { api, ctx };

        // Set default options before initialize
        handle.set_option("vo", vo)?;
        handle.set_option("terminal", "no")?;
        handle.set_option("msg-level", "all=no")?;
        handle.set_option("idle", "yes")?;
        handle.set_option("input-default-bindings", "no")?;
        handle.set_option("input-vo-keyboard", "no")?;
        // Allow software gain above 100% so unflick can match how loud
        // VLC / browser audio sound on the same source. mpv defaults to
        // 130 already but we set it explicitly so behavior is independent
        // of the bundled libmpv version's defaults.
        handle.set_option("volume-max", "200")?;
        // Stay paused at EOF instead of unloading. Without this, reaching
        // the end of a file clears mpv's path/state and the GUI flips
        // back to the drop zone — so the user has no play button to
        // hit if they want to replay. With keep-open=yes, EOF parks
        // the file at duration with pause=true; resume() detects the
        // EOF position and rewinds to 0 before unpausing.
        handle.set_option("keep-open", "yes")?;

        let err = unsafe { (handle.api.initialize)(handle.ctx) };
        if err < 0 {
            bail!("mpv_initialize failed: {}", handle.error_str(err));
        }

        Ok(handle)
    }

    /// Create mpv with its own video window (not headless).
    pub fn new_with_video() -> Result<Self> {
        let api = Arc::new(MpvApi::load().map_err(|e| anyhow::anyhow!(e))?);

        let ctx = unsafe { (api.create)() };
        if ctx.is_null() {
            bail!("mpv_create returned null");
        }

        let handle = Self { api, ctx };

        // Let mpv use default video output and create its own window
        handle.set_option("terminal", "no")?;
        handle.set_option("msg-level", "all=no")?;
        handle.set_option("idle", "yes")?;
        handle.set_option("force-window", "yes")?;
        handle.set_option("keepaspect", "yes")?;
        handle.set_option("border", "no")?;
        handle.set_option("title", "unflick")?;
        handle.set_option("input-default-bindings", "no")?;
        handle.set_option("input-vo-keyboard", "no")?;
        handle.set_option("osc", "no")?;

        let err = unsafe { (handle.api.initialize)(handle.ctx) };
        if err < 0 {
            bail!("mpv_initialize failed: {}", handle.error_str(err));
        }

        Ok(handle)
    }

    /// Create mpv instance that renders into an existing window (HWND on Windows).
    pub fn new_with_wid(wid: i64) -> Result<Self> {
        let api = Arc::new(MpvApi::load().map_err(|e| anyhow::anyhow!(e))?);

        let ctx = unsafe { (api.create)() };
        if ctx.is_null() {
            bail!("mpv_create returned null");
        }

        let handle = Self { api, ctx };

        // Set options for embedded video rendering
        handle.set_option("terminal", "no")?;
        handle.set_option("msg-level", "all=no")?;
        handle.set_option("idle", "yes")?;
        handle.set_option("input-default-bindings", "no")?;
        handle.set_option("input-vo-keyboard", "no")?;
        handle.set_option("wid", &wid.to_string())?;
        handle.set_option("keepaspect", "yes")?;

        let err = unsafe { (handle.api.initialize)(handle.ctx) };
        if err < 0 {
            bail!("mpv_initialize failed: {}", handle.error_str(err));
        }

        Ok(handle)
    }

    fn error_str(&self, code: i32) -> String {
        unsafe {
            let ptr = (self.api.error_string)(code);
            if ptr.is_null() {
                return format!("error {}", code);
            }
            CStr::from_ptr(ptr).to_string_lossy().into_owned()
        }
    }

    fn check(&self, code: i32) -> Result<()> {
        if code >= 0 {
            Ok(())
        } else {
            bail!("{}", self.error_str(code));
        }
    }

    pub fn set_option(&self, name: &str, value: &str) -> Result<()> {
        let name = CString::new(name)?;
        let value = CString::new(value)?;
        let err = unsafe { (self.api.set_option_string)(self.ctx, name.as_ptr(), value.as_ptr()) };
        self.check(err)
    }

    pub fn set_property_string(&self, name: &str, value: &str) -> Result<()> {
        let name = CString::new(name)?;
        let value = CString::new(value)?;
        let err = unsafe { (self.api.set_property_string)(self.ctx, name.as_ptr(), value.as_ptr()) };
        self.check(err)
    }

    pub fn set_property_f64(&self, name: &str, value: f64) -> Result<()> {
        let name = CString::new(name)?;
        let mut val = value;
        let err = unsafe {
            (self.api.set_property)(
                self.ctx,
                name.as_ptr(),
                ffi::MPV_FORMAT_DOUBLE,
                &mut val as *mut f64 as *mut c_void,
            )
        };
        self.check(err)
    }

    pub fn set_property_i64(&self, name: &str, value: i64) -> Result<()> {
        let name = CString::new(name)?;
        let mut val = value;
        let err = unsafe {
            (self.api.set_property)(
                self.ctx,
                name.as_ptr(),
                ffi::MPV_FORMAT_INT64,
                &mut val as *mut i64 as *mut c_void,
            )
        };
        self.check(err)
    }

    pub fn set_property_bool(&self, name: &str, value: bool) -> Result<()> {
        let name = CString::new(name)?;
        let mut val: i32 = if value { 1 } else { 0 };
        let err = unsafe {
            (self.api.set_property)(
                self.ctx,
                name.as_ptr(),
                ffi::MPV_FORMAT_FLAG,
                &mut val as *mut i32 as *mut c_void,
            )
        };
        self.check(err)
    }

    pub fn get_property_f64(&self, name: &str) -> Result<f64> {
        let name = CString::new(name)?;
        let mut val: f64 = 0.0;
        let err = unsafe {
            (self.api.get_property)(
                self.ctx,
                name.as_ptr(),
                ffi::MPV_FORMAT_DOUBLE,
                &mut val as *mut f64 as *mut c_void,
            )
        };
        self.check(err)?;
        Ok(val)
    }

    pub fn get_property_i64(&self, name: &str) -> Result<i64> {
        let name = CString::new(name)?;
        let mut val: i64 = 0;
        let err = unsafe {
            (self.api.get_property)(
                self.ctx,
                name.as_ptr(),
                ffi::MPV_FORMAT_INT64,
                &mut val as *mut i64 as *mut c_void,
            )
        };
        self.check(err)?;
        Ok(val)
    }

    pub fn get_property_bool(&self, name: &str) -> Result<bool> {
        let name = CString::new(name)?;
        let mut val: i32 = 0;
        let err = unsafe {
            (self.api.get_property)(
                self.ctx,
                name.as_ptr(),
                ffi::MPV_FORMAT_FLAG,
                &mut val as *mut i32 as *mut c_void,
            )
        };
        self.check(err)?;
        Ok(val != 0)
    }

    pub fn get_property_string(&self, name: &str) -> Result<String> {
        let name = CString::new(name)?;
        let ptr = unsafe { (self.api.get_property_string)(self.ctx, name.as_ptr()) };
        if ptr.is_null() {
            bail!("property {} returned null", name.to_str().unwrap_or("?"));
        }
        let s = unsafe { CStr::from_ptr(ptr).to_string_lossy().into_owned() };
        unsafe { (self.api.free)(ptr as *mut c_void) };
        Ok(s)
    }

    /// Run a command like ["loadfile", "/path/to/file"].
    pub fn command(&self, args: &[&str]) -> Result<()> {
        let c_args: Vec<CString> = args.iter().map(|s| CString::new(*s).unwrap()).collect();
        let mut ptrs: Vec<*const i8> = c_args.iter().map(|s| s.as_ptr()).collect();
        ptrs.push(ptr::null());
        let err = unsafe { (self.api.command)(self.ctx, ptrs.as_ptr()) };
        self.check(err)
    }

    /// Observe a property for changes.
    pub fn observe_property(&self, reply_userdata: u64, name: &str, format: i32) -> Result<()> {
        let name = CString::new(name)?;
        let err = unsafe { (self.api.observe_property)(self.ctx, reply_userdata, name.as_ptr(), format) };
        self.check(err)
    }

    /// Wait for an event with timeout in seconds. Returns (event_id, error_code).
    pub fn wait_event(&self, timeout: f64) -> (i32, i32) {
        let event = unsafe { (self.api.wait_event)(self.ctx, timeout) };
        if event.is_null() {
            return (ffi::MPV_EVENT_NONE, 0);
        }
        let ev = unsafe { &*event };
        (ev.event_id, ev.error)
    }

    /// Send quit command to mpv so it shuts down immediately.
    pub fn quit(&self) {
        let _ = self.command(&["quit"]);
    }

    /// Borrow the dynamically-loaded API table. Used by render.rs to call the
    /// render-context family of functions without re-loading the DLL.
    pub fn api_arc(&self) -> Arc<MpvApi> {
        Arc::clone(&self.api)
    }

    /// Raw mpv handle pointer. Stays valid for the lifetime of this MpvHandle.
    /// Render.rs needs this to bind a render context to this mpv instance.
    pub fn raw_ctx(&self) -> ffi::MpvCtx {
        self.ctx
    }
}

impl Drop for MpvHandle {
    fn drop(&mut self) {
        // Send quit first so mpv doesn't hang waiting for playback to finish
        self.quit();
        unsafe { (self.api.destroy)(self.ctx) };
    }
}
