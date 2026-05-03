//! Linux VideoSurface: child X11 window beneath the WebKitGTK widget.
//!
//! Architecture (post-v0.8.4 simplification): we just create the X11
//! child window. *We don't create a GL context*. mpv on Linux is
//! initialised with `--wid=<xid> --vo=x11`, so mpv's own internal vo
//! drives rendering via XPutImage straight into our window. That means
//! one less moving part vs. Windows/macOS (which keep the
//! glutin-context + render-thread path) — but it also fixes the
//! showstopper where llvmpipe (software GL, what you get in a VMware
//! VM with no 3D, in WSLg, or on a low-end NUC without DRI) renders
//! frames the X server never composites onto our visible window.
//! Going through `vo=x11` means mpv writes pixels straight to the
//! X11 frontbuffer with no GL backbuffer in between, which the X
//! server treats just like any other XPutImage call. Works
//! identically on native Xorg, on Xwayland, on remote X
//! forwarding, and on every VM display driver mesa supports.
//!
//! Wayland-native (`wp_subsurface` + EGL) lands as a separate v0.9.x
//! follow-up — for now Wayland users go through Xwayland (Tauri's
//! GTK3 stack picks Xwayland when GDK_BACKEND=x11 is set, see
//! main.rs).

#![cfg(target_os = "linux")]

use anyhow::{anyhow, bail, Result};
use std::ffi::c_void;
use std::ptr;

use super::VideoSurface;

pub struct LinuxVideoSurface {
    /// X11 child window ID (an XID — `c_ulong` on Linux).
    window: u64,
    /// X11 Display* — borrowed from Tauri / GTK; we do not close it.
    display_ptr: *mut c_void,
    /// Tracked size — used by `size()`.
    cur_w: std::sync::atomic::AtomicI32,
    cur_h: std::sync::atomic::AtomicI32,
}

unsafe impl Send for LinuxVideoSurface {}
unsafe impl Sync for LinuxVideoSurface {}

impl LinuxVideoSurface {
    /// `parent_xid` is the X11 Window ID of the Tauri main window
    /// (`gtk_window.window().get_xid()`). `display_ptr` is the Xlib
    /// `Display*` used by GTK/WebKit; we share it.
    pub fn new(display_ptr: *mut c_void, parent_xid: u64, w: i32, h: i32) -> Result<Self> {
        if display_ptr.is_null() {
            bail!("X11 Display* is null");
        }
        if parent_xid == 0 {
            bail!("parent X11 Window XID is 0");
        }

        // Create a child X11 window via dlopen'd libX11. We use
        // XCreateSimpleWindow with the parent's visual (depth = parent's
        // depth, visual = CopyFromParent) — that's fine here because
        // we're not creating a GL context on this window. mpv's vo=x11
        // path uses XPutImage which works on any visual the X server
        // accepts.
        let xlib = unsafe { libloading::Library::new("libX11.so.6") }
            .or_else(|_| unsafe { libloading::Library::new("libX11.so") })
            .map_err(|e| anyhow!("dlopen libX11: {e}"))?;

        type CreateSimpleWindowFn = unsafe extern "C" fn(
            *mut c_void, u64, i32, i32, u32, u32, u32, u64, u64,
        ) -> u64;
        type MapWindowFn = unsafe extern "C" fn(*mut c_void, u64) -> i32;
        type FlushFn = unsafe extern "C" fn(*mut c_void) -> i32;

        let create_simple_window: libloading::Symbol<CreateSimpleWindowFn> = unsafe {
            xlib.get(b"XCreateSimpleWindow\0")
                .map_err(|e| anyhow!("XCreateSimpleWindow: {e}"))?
        };
        let map_window: libloading::Symbol<MapWindowFn> = unsafe {
            xlib.get(b"XMapWindow\0").map_err(|e| anyhow!("XMapWindow: {e}"))?
        };
        let flush: libloading::Symbol<FlushFn> = unsafe {
            xlib.get(b"XFlush\0").map_err(|e| anyhow!("XFlush: {e}"))?
        };

        let window = unsafe {
            create_simple_window(
                display_ptr,
                parent_xid,
                0,
                0,
                w.max(1) as u32,
                h.max(1) as u32,
                0,
                0,
                0, // background = black so the area looks intentional before mpv loads.
            )
        };
        if window == 0 {
            bail!("XCreateSimpleWindow returned 0");
        }
        unsafe {
            map_window(display_ptr, window);
            flush(display_ptr);
        }

        // Box-leak the xlib handle so symbol pointers stay valid for
        // the lifetime of the surface. Surface lives until app shutdown.
        Box::leak(Box::new(xlib));

        Ok(Self {
            window,
            display_ptr,
            cur_w: std::sync::atomic::AtomicI32::new(w.max(1)),
            cur_h: std::sync::atomic::AtomicI32::new(h.max(1)),
        })
    }

    /// Expose the X11 XID so lib.rs can hand it to mpv via `--wid=<xid>`.
    pub fn window_id(&self) -> u64 {
        self.window
    }
}

impl VideoSurface for LinuxVideoSurface {
    // make_current / get_proc_address / swap_buffers are no-ops on Linux
    // because we don't drive GL ourselves — see file-level comment.

    fn make_current(&self) -> Result<()> {
        Ok(())
    }

    fn get_proc_address(&self, _name: &str) -> *mut c_void {
        ptr::null_mut()
    }

    fn set_geometry(&self, x: i32, y: i32, w: i32, h: i32) -> Result<()> {
        // Use XMoveResizeWindow via dlopen.
        let xlib = unsafe {
            libloading::Library::new("libX11.so.6")
                .or_else(|_| libloading::Library::new("libX11.so"))
                .map_err(|e| anyhow!("dlopen libX11: {e}"))?
        };
        type MoveResizeFn =
            unsafe extern "C" fn(*mut c_void, u64, i32, i32, u32, u32) -> i32;
        type FlushFn = unsafe extern "C" fn(*mut c_void) -> i32;
        let move_resize: libloading::Symbol<MoveResizeFn> = unsafe {
            xlib.get(b"XMoveResizeWindow\0")
                .map_err(|e| anyhow!("XMoveResizeWindow: {e}"))?
        };
        let flush: libloading::Symbol<FlushFn> = unsafe {
            xlib.get(b"XFlush\0").map_err(|e| anyhow!("XFlush: {e}"))?
        };
        unsafe {
            move_resize(
                self.display_ptr,
                self.window,
                x,
                y,
                w.max(1) as u32,
                h.max(1) as u32,
            );
            flush(self.display_ptr);
        }
        use std::sync::atomic::Ordering;
        self.cur_w.store(w.max(1), Ordering::Relaxed);
        self.cur_h.store(h.max(1), Ordering::Relaxed);
        Ok(())
    }

    fn set_visible(&self, visible: bool) {
        let xlib = match unsafe {
            libloading::Library::new("libX11.so.6")
                .or_else(|_| libloading::Library::new("libX11.so"))
        } {
            Ok(l) => l,
            Err(_) => return,
        };
        type SimpleFn = unsafe extern "C" fn(*mut c_void, u64) -> i32;
        unsafe {
            let res = if visible {
                xlib.get::<SimpleFn>(b"XMapWindow\0")
                    .map(|f| f(self.display_ptr, self.window))
            } else {
                xlib.get::<SimpleFn>(b"XUnmapWindow\0")
                    .map(|f| f(self.display_ptr, self.window))
            };
            let _ = res;
            type FlushFn = unsafe extern "C" fn(*mut c_void) -> i32;
            if let Ok(flush) = xlib.get::<FlushFn>(b"XFlush\0") {
                flush(self.display_ptr);
            }
        }
    }

    fn size(&self) -> (i32, i32) {
        use std::sync::atomic::Ordering;
        (
            self.cur_w.load(Ordering::Relaxed),
            self.cur_h.load(Ordering::Relaxed),
        )
    }

    fn swap_buffers(&self) -> Result<()> {
        // No GL context on Linux — see file-level comment. mpv handles
        // its own swap via XPutImage. This method is part of the
        // VideoSurface trait shared with Windows / macOS, so it has to
        // exist; it's simply never called on Linux because we use
        // RenderLoop::start_passive (no render thread).
        Ok(())
    }
}
