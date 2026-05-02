//! Linux VideoSurface: child X11 window beneath the WebKitGTK widget.
//!
//! Architecture parallels macOS: instead of a separate top-level popup
//! (the Windows pattern, forced by WebView2's DComp surface painting
//! over child HWNDs), we create a child X11 window underneath the
//! WebKit2GTK widget that hosts our OpenGL context. WebKit's drawing
//! happens above; the WebView's transparent body lets video bleed
//! through where the React layout is empty.
//!
//! Wayland is **not** yet supported — Tauri's GTK3 stack runs on
//! Xwayland by default in WSLg / GNOME Wayland sessions, so X11 path
//! covers WSL development + most current users. Native Wayland
//! (wl_subsurface + EGL) lands in a follow-up.
//!
//! GL context: glutin's GLX backend talks to libGL on X11. Same legacy
//! GL profile mpv expects on Windows / macOS.

#![cfg(target_os = "linux")]

use anyhow::{anyhow, bail, Result};
use std::ffi::{c_void, CString};
use std::ptr;
use std::sync::Mutex;

use glutin::config::ConfigTemplateBuilder;
use glutin::context::{
    ContextApi, ContextAttributesBuilder, NotCurrentContext, NotCurrentGlContext,
    PossiblyCurrentContext, PossiblyCurrentGlContext,
};
use glutin::display::{Display, DisplayApiPreference, GlDisplay};
use glutin::surface::{GlSurface, Surface, SurfaceAttributesBuilder, SwapInterval, WindowSurface};
use raw_window_handle::{RawDisplayHandle, RawWindowHandle, XlibDisplayHandle, XlibWindowHandle};

use super::VideoSurface;

enum ContextSlot {
    NotCurrent(NotCurrentContext),
    Current(PossiblyCurrentContext),
}

pub struct LinuxVideoSurface {
    /// X11 child window ID (an XID — `c_ulong` on Linux).
    window: u64,
    /// X11 Display* — borrowed from Tauri / GTK; we do not close it.
    display_ptr: *mut c_void,
    context: Mutex<Option<ContextSlot>>,
    surface: Surface<WindowSurface>,
    display: Display,
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

        // Create a child X11 window. We dlopen libX11 to keep this file
        // dependency-free at build time — the lib is universally
        // available on any system that can run the WebKitGTK Tauri
        // app, and dlopen avoids pulling x11-dl as a hard dep.
        let xlib = unsafe { libloading::Library::new("libX11.so.6") }
            .or_else(|_| unsafe { libloading::Library::new("libX11.so") })
            .map_err(|e| anyhow!("dlopen libX11: {e}"))?;

        type CreateSimpleWindowFn = unsafe extern "C" fn(
            *mut c_void, // display
            u64,         // parent
            i32,         // x
            i32,         // y
            u32,         // width
            u32,         // height
            u32,         // border_width
            u64,         // border
            u64,         // background
        ) -> u64;
        type MapWindowFn = unsafe extern "C" fn(*mut c_void, u64) -> i32;
        type FlushFn = unsafe extern "C" fn(*mut c_void) -> i32;

        let create_simple_window: libloading::Symbol<CreateSimpleWindowFn> = unsafe {
            xlib.get(b"XCreateSimpleWindow\0")
                .map_err(|e| anyhow!("XCreateSimpleWindow: {e}"))?
        };
        let map_window: libloading::Symbol<MapWindowFn> = unsafe {
            xlib.get(b"XMapWindow\0")
                .map_err(|e| anyhow!("XMapWindow: {e}"))?
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
                0,
            )
        };
        if window == 0 {
            bail!("XCreateSimpleWindow returned 0");
        }
        unsafe {
            map_window(display_ptr, window);
            flush(display_ptr);
        }

        // Hand our XID to glutin via raw-window-handle.
        let mut win_handle = XlibWindowHandle::new(window);
        // visual id 0 is "default" — let glutin pick.
        win_handle.visual_id = 0;
        let mut display_handle = XlibDisplayHandle::new(
            std::ptr::NonNull::new(display_ptr),
            0, // screen 0 — Tauri's GTK uses the default screen
        );
        // Avoid unused warning if glutin's signature evolves.
        let _ = &mut display_handle;
        let raw_window = RawWindowHandle::Xlib(win_handle);
        let raw_display = RawDisplayHandle::Xlib(display_handle);

        // Use GLX since we're on X11 with libGL available.
        let display = unsafe {
            Display::new(raw_display, DisplayApiPreference::Glx(Box::new(|_| {
                // Default register-window-class hook does nothing on GLX.
            })))
            .map_err(|e| anyhow!("create GLX display: {e}"))?
        };

        let template = ConfigTemplateBuilder::new()
            .with_alpha_size(8)
            .compatible_with_native_window(raw_window)
            .build();

        let config = unsafe {
            display
                .find_configs(template)
                .map_err(|e| anyhow!("find_configs: {e}"))?
                .next()
                .ok_or_else(|| anyhow!("no compatible GLX config found"))?
        };

        let context_attrs = ContextAttributesBuilder::new()
            .with_context_api(ContextApi::OpenGl(None))
            .build(Some(raw_window));

        let not_current = unsafe {
            display
                .create_context(&config, &context_attrs)
                .map_err(|e| anyhow!("create_context: {e}"))?
        };

        let surface_attrs = SurfaceAttributesBuilder::<WindowSurface>::new().build(
            raw_window,
            std::num::NonZeroU32::new(w.max(1) as u32).unwrap(),
            std::num::NonZeroU32::new(h.max(1) as u32).unwrap(),
        );
        let surface = unsafe {
            display
                .create_window_surface(&config, &surface_attrs)
                .map_err(|e| anyhow!("create_window_surface: {e}"))?
        };

        // Keep xlib loaded for the lifetime of the surface so symbol
        // pointers stay valid. Box-leak it — the surface lives until
        // app shutdown anyway.
        Box::leak(Box::new(xlib));

        Ok(Self {
            window,
            display_ptr,
            display,
            surface,
            context: Mutex::new(Some(ContextSlot::NotCurrent(not_current))),
        })
    }
}

impl VideoSurface for LinuxVideoSurface {
    fn make_current(&self) -> Result<()> {
        let mut guard = self
            .context
            .lock()
            .map_err(|_| anyhow!("video surface mutex poisoned"))?;
        match guard.take() {
            Some(ContextSlot::NotCurrent(ctx)) => {
                let now = ctx
                    .make_current(&self.surface)
                    .map_err(|e| anyhow!("make_current (first): {e}"))?;
                if let Err(e) = self.surface.set_swap_interval(
                    &now,
                    SwapInterval::Wait(std::num::NonZeroU32::new(1).unwrap()),
                ) {
                    eprintln!("[unflick-render] set_swap_interval failed: {e}");
                }
                *guard = Some(ContextSlot::Current(now));
                Ok(())
            }
            Some(ContextSlot::Current(ctx)) => {
                ctx.make_current(&self.surface)
                    .map_err(|e| anyhow!("make_current: {e}"))?;
                *guard = Some(ContextSlot::Current(ctx));
                Ok(())
            }
            None => Err(anyhow!("context already released")),
        }
    }

    fn get_proc_address(&self, name: &str) -> *mut c_void {
        let cname = match CString::new(name) {
            Ok(c) => c,
            Err(_) => return ptr::null_mut(),
        };
        self.display.get_proc_address(cname.as_c_str()) as *mut c_void
    }

    fn set_geometry(&self, x: i32, y: i32, w: i32, h: i32) -> Result<()> {
        // Use XMoveResizeWindow via the same dlopen approach as new().
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
                xlib.get::<SimpleFn>(b"XMapWindow\0").map(|f| f(self.display_ptr, self.window))
            } else {
                xlib.get::<SimpleFn>(b"XUnmapWindow\0").map(|f| f(self.display_ptr, self.window))
            };
            let _ = res;
            type FlushFn = unsafe extern "C" fn(*mut c_void) -> i32;
            if let Ok(flush) = xlib.get::<FlushFn>(b"XFlush\0") {
                flush(self.display_ptr);
            }
        }
    }

    fn size(&self) -> (i32, i32) {
        let w = self.surface.width().unwrap_or(1) as i32;
        let h = self.surface.height().unwrap_or(1) as i32;
        (w, h)
    }

    fn swap_buffers(&self) -> Result<()> {
        let guard = self
            .context
            .lock()
            .map_err(|_| anyhow!("video surface mutex poisoned"))?;
        match guard.as_ref() {
            Some(ContextSlot::Current(ctx)) => self
                .surface
                .swap_buffers(ctx)
                .map_err(|e| anyhow!("swap_buffers: {e}")),
            Some(ContextSlot::NotCurrent(_)) => {
                Err(anyhow!("swap_buffers before first make_current"))
            }
            None => Err(anyhow!("context already released")),
        }
    }
}
