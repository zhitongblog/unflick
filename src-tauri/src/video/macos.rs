//! macOS VideoSurface — not yet implemented.
//!
//! Architecture is settled (parent NSView is exposed by Tauri via
//! raw-window-handle::RawWindowHandle::AppKit; we'd attach a child NSView
//! and let glutin's CGL backend produce a GL context on it). What's missing
//! is the actual implementation + a macOS build environment to test it on.
//!
//! v0.8.0 ships Windows-only. macOS support is tracked as the v0.8.1
//! release: add objc2 + objc2-app-kit deps, port the Windows impl shape,
//! validate against an Apple Silicon Mac.

use anyhow::{bail, Result};
use std::ffi::c_void;

use super::VideoSurface;

pub struct MacosVideoSurface;

impl MacosVideoSurface {
    pub fn new(_parent_ns_view: *mut c_void, _w: i32, _h: i32) -> Result<Self> {
        bail!("macOS support is in progress — see v0.8.1 milestone. Use Windows for now.")
    }
}

impl VideoSurface for MacosVideoSurface {
    fn make_current(&self) -> Result<()> {
        bail!("macOS not implemented yet")
    }
    fn get_proc_address(&self, _name: &str) -> *mut c_void {
        std::ptr::null_mut()
    }
    fn set_geometry(&self, _x: i32, _y: i32, _w: i32, _h: i32) -> Result<()> {
        bail!("macOS not implemented yet")
    }
    fn set_visible(&self, _visible: bool) {}
    fn size(&self) -> (i32, i32) {
        (0, 0)
    }
    fn swap_buffers(&self) -> Result<()> {
        bail!("macOS not implemented yet")
    }
}
