use std::ffi::c_char;
use std::os::raw::{c_double, c_int, c_void};

use libloading::Library;

// mpv_format enum values
#[allow(dead_code)]
pub const MPV_FORMAT_NONE: c_int = 0;
#[allow(dead_code)]
pub const MPV_FORMAT_STRING: c_int = 1;
pub const MPV_FORMAT_FLAG: c_int = 3;
pub const MPV_FORMAT_INT64: c_int = 4;
pub const MPV_FORMAT_DOUBLE: c_int = 5;

// mpv_event_id values
#[allow(dead_code)]
pub const MPV_EVENT_NONE: c_int = 0;
#[allow(dead_code)]
pub const MPV_EVENT_SHUTDOWN: c_int = 1;
#[allow(dead_code)]
pub const MPV_EVENT_END_FILE: c_int = 7;
#[allow(dead_code)]
pub const MPV_EVENT_FILE_LOADED: c_int = 8;
#[allow(dead_code)]
pub const MPV_EVENT_PROPERTY_CHANGE: c_int = 22;

#[repr(C)]
pub struct MpvEvent {
    pub event_id: c_int,
    pub error: c_int,
    pub reply_userdata: u64,
    pub data: *mut c_void,
}

pub type MpvCtx = *mut c_void;

type FnCreate = unsafe extern "C" fn() -> MpvCtx;
type FnInitialize = unsafe extern "C" fn(MpvCtx) -> c_int;
type FnDestroy = unsafe extern "C" fn(MpvCtx);
type FnSetOptionString = unsafe extern "C" fn(MpvCtx, *const c_char, *const c_char) -> c_int;
type FnSetPropertyString = unsafe extern "C" fn(MpvCtx, *const c_char, *const c_char) -> c_int;
type FnSetProperty = unsafe extern "C" fn(MpvCtx, *const c_char, c_int, *mut c_void) -> c_int;
type FnGetProperty = unsafe extern "C" fn(MpvCtx, *const c_char, c_int, *mut c_void) -> c_int;
type FnGetPropertyString = unsafe extern "C" fn(MpvCtx, *const c_char) -> *mut c_char;
type FnFree = unsafe extern "C" fn(*mut c_void);
type FnCommand = unsafe extern "C" fn(MpvCtx, *const *const c_char) -> c_int;
type FnObserveProperty = unsafe extern "C" fn(MpvCtx, u64, *const c_char, c_int) -> c_int;
type FnWaitEvent = unsafe extern "C" fn(MpvCtx, c_double) -> *mut MpvEvent;
type FnErrorString = unsafe extern "C" fn(c_int) -> *const c_char;

macro_rules! load_fn {
    ($lib:expr, $name:expr) => {{
        let sym = $lib.get::<*const c_void>($name)
            .map_err(|e| format!("{}: {}", String::from_utf8_lossy($name), e))?;
        *sym
    }};
}

/// Dynamically loaded mpv API functions.
pub struct MpvApi {
    _lib: Library,
    pub create: FnCreate,
    pub initialize: FnInitialize,
    pub destroy: FnDestroy,
    pub set_option_string: FnSetOptionString,
    pub set_property_string: FnSetPropertyString,
    pub set_property: FnSetProperty,
    pub get_property: FnGetProperty,
    pub get_property_string: FnGetPropertyString,
    pub free: FnFree,
    pub command: FnCommand,
    pub observe_property: FnObserveProperty,
    pub wait_event: FnWaitEvent,
    pub error_string: FnErrorString,
}

impl MpvApi {
    pub fn load() -> Result<Self, String> {
        let lib_name = if cfg!(target_os = "windows") {
            "libmpv-2.dll"
        } else if cfg!(target_os = "macos") {
            "libmpv.2.dylib"
        } else {
            "libmpv.so.2"
        };

        // Try loading from multiple locations:
        // 1. Default search path (exe directory, system, PATH)
        // 2. Tauri resource directory (for bundled installs)
        let lib = unsafe { Library::new(lib_name) }
            .or_else(|_| {
                // Try exe directory / resources / mpv-dev /
                let exe_dir = std::env::current_exe()
                    .ok()
                    .and_then(|p| p.parent().map(|d| d.to_path_buf()));
                if let Some(dir) = &exe_dir {
                    // Try <exe_dir>/resources/mpv-dev/libmpv-2.dll (NSIS install)
                    let resource_path = dir.join("resources").join("mpv-dev").join(lib_name);
                    if resource_path.exists() {
                        return unsafe { Library::new(&resource_path) };
                    }
                    // Try <exe_dir>/mpv-dev/libmpv-2.dll
                    let direct_path = dir.join("mpv-dev").join(lib_name);
                    if direct_path.exists() {
                        return unsafe { Library::new(&direct_path) };
                    }
                    // Try <exe_dir>/libmpv-2.dll
                    let beside_path = dir.join(lib_name);
                    if beside_path.exists() {
                        return unsafe { Library::new(&beside_path) };
                    }
                }
                unsafe { Library::new(lib_name) }
            })
            .map_err(|e| format!("failed to load {}: {}", lib_name, e))?;

        unsafe {
            let api = Self {
                create: std::mem::transmute(load_fn!(lib, b"mpv_create\0")),
                initialize: std::mem::transmute(load_fn!(lib, b"mpv_initialize\0")),
                destroy: std::mem::transmute(load_fn!(lib, b"mpv_destroy\0")),
                set_option_string: std::mem::transmute(load_fn!(lib, b"mpv_set_option_string\0")),
                set_property_string: std::mem::transmute(load_fn!(lib, b"mpv_set_property_string\0")),
                set_property: std::mem::transmute(load_fn!(lib, b"mpv_set_property\0")),
                get_property: std::mem::transmute(load_fn!(lib, b"mpv_get_property\0")),
                get_property_string: std::mem::transmute(load_fn!(lib, b"mpv_get_property_string\0")),
                free: std::mem::transmute(load_fn!(lib, b"mpv_free\0")),
                command: std::mem::transmute(load_fn!(lib, b"mpv_command\0")),
                observe_property: std::mem::transmute(load_fn!(lib, b"mpv_observe_property\0")),
                wait_event: std::mem::transmute(load_fn!(lib, b"mpv_wait_event\0")),
                error_string: std::mem::transmute(load_fn!(lib, b"mpv_error_string\0")),
                _lib: lib,
            };
            Ok(api)
        }
    }
}
