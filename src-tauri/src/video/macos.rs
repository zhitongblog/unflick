//! macOS VideoSurface impl placeholder. Filled in P3.
//!
//! Will create a child NSOpenGLView (or layer-backed NSView with a
//! CAOpenGLLayer) inside the Tauri window's content NSView. mpv's
//! cocoa-cb is the spiritual reference.

use anyhow::{bail, Result};
use std::ffi::c_void;

use super::VideoSurface;

pub struct MacosVideoSurface;

impl MacosVideoSurface {
    pub fn new(_parent_ns_view: *mut c_void, _w: i32, _h: i32) -> Result<Self> {
        bail!("MacosVideoSurface not yet implemented (P3)")
    }
}

impl VideoSurface for MacosVideoSurface {
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
