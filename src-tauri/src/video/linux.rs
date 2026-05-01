//! Linux VideoSurface impl placeholder. Filled in P3.
//!
//! On X11: child X window via xcb, glutin GLX context.
//! On Wayland: wl_subsurface under the WebKitGTK surface, EGL context.
//! Tauri exposes both via raw-window-handle so we'll branch on
//! RawWindowHandle::Xlib vs ::Wayland at runtime.

use anyhow::{bail, Result};
use std::ffi::c_void;

use super::VideoSurface;

pub struct LinuxVideoSurface;

impl LinuxVideoSurface {
    pub fn new(_parent_handle: *mut c_void, _w: i32, _h: i32) -> Result<Self> {
        bail!("LinuxVideoSurface not yet implemented (P3)")
    }
}

impl VideoSurface for LinuxVideoSurface {
    fn make_current(&self) -> Result<()> {
        bail!("not implemented")
    }
    fn get_proc_address(&self, _name: &str) -> *mut c_void {
        std::ptr::null_mut()
    }
    fn set_geometry(&self, _x: i32, _y: i32, _w: i32, _h: i32) -> Result<()> {
        bail!("not implemented")
    }
    fn set_visible(&self, _visible: bool) {}
    fn size(&self) -> (i32, i32) {
        (0, 0)
    }
    fn swap_buffers(&self) -> Result<()> {
        bail!("not implemented")
    }
}
