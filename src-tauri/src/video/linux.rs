//! Linux VideoSurface — not yet implemented.
//!
//! Architecture is settled (Tauri exposes either RawWindowHandle::Xlib or
//! RawWindowHandle::Wayland depending on the user's session). For X11 we'd
//! XCreateWindow a child below the WebKitGTK surface and let glutin's GLX
//! backend produce a GL context. Wayland needs a wl_subsurface and EGL
//! instead — handle that as v0.8.2 since the X11 path covers most desktops.
//!
//! v0.8.0 ships Windows-only. Linux X11 support is tracked as v0.8.2.

use anyhow::{bail, Result};
use std::ffi::c_void;

use super::VideoSurface;

pub struct LinuxVideoSurface;

impl LinuxVideoSurface {
    pub fn new(_parent_handle: *mut c_void, _w: i32, _h: i32) -> Result<Self> {
        bail!("Linux support is in progress — see v0.8.2 milestone. Use Windows for now.")
    }
}

impl VideoSurface for LinuxVideoSurface {
    fn make_current(&self) -> Result<()> {
        bail!("Linux not implemented yet")
    }
    fn get_proc_address(&self, _name: &str) -> *mut c_void {
        std::ptr::null_mut()
    }
    fn set_geometry(&self, _x: i32, _y: i32, _w: i32, _h: i32) -> Result<()> {
        bail!("Linux not implemented yet")
    }
    fn set_visible(&self, _visible: bool) {}
    fn size(&self) -> (i32, i32) {
        (0, 0)
    }
    fn swap_buffers(&self) -> Result<()> {
        bail!("Linux not implemented yet")
    }
}
