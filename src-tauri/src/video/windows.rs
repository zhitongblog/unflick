//! Windows VideoSurface impl: a child HWND with a WGL OpenGL context.
//!
//! Layout: Tauri creates the main HWND. WebView2 sits on top as one child
//! window (CoreWebView2Controller's host). We create a sibling child HWND
//! beneath the WebView in Z-order. The WebView is configured with a
//! transparent background so the video shows through where its CSS body is
//! transparent, and the controls/chrome occlude where they aren't.

use std::ffi::{c_void, CString, OsStr};
use std::os::windows::ffi::OsStrExt;
use std::ptr;
use std::sync::Mutex;

use anyhow::{anyhow, bail, Result};
use glutin::config::ConfigTemplateBuilder;
use glutin::context::{
    ContextApi, ContextAttributesBuilder, NotCurrentContext, NotCurrentGlContext,
    PossiblyCurrentContext, PossiblyCurrentGlContext,
};
use glutin::display::{Display, DisplayApiPreference, GlDisplay};
use glutin::surface::{GlSurface, Surface, SurfaceAttributesBuilder, SwapInterval, WindowSurface};
use raw_window_handle::{
    RawDisplayHandle, RawWindowHandle, Win32WindowHandle, WindowsDisplayHandle,
};
use windows_sys::core::PCWSTR;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::ClientToScreen;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, RegisterClassExW, SetLayeredWindowAttributes, SetWindowPos,
    ShowWindow, CS_HREDRAW, CS_OWNDC, CS_VREDRAW, HWND_NOTOPMOST, HWND_TOP, HWND_TOPMOST,
    LWA_ALPHA, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SW_HIDE, SW_SHOW, WM_ERASEBKGND,
    WNDCLASSEXW, WS_CLIPCHILDREN, WS_CLIPSIBLINGS, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT, WS_POPUP,
};

use super::VideoSurface;

const VIDEO_CLASS_NAME: &str = "UnflickVideoSurface";

fn wide(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
}

/// Register the child window class once per process. Idempotent — extra
/// RegisterClassExW calls return 0 with ERROR_CLASS_ALREADY_EXISTS, which
/// we treat as success.
fn ensure_class_registered() -> Result<PCWSTR> {
    use std::sync::OnceLock;
    static CLASS_NAME: OnceLock<Vec<u16>> = OnceLock::new();
    static REGISTERED: OnceLock<()> = OnceLock::new();

    let name = CLASS_NAME.get_or_init(|| wide(VIDEO_CLASS_NAME));

    REGISTERED.get_or_init(|| unsafe {
        let hinst = GetModuleHandleW(ptr::null());
        let class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            // CS_OWNDC: each window gets a private DC, required for stable
            // wglMakeCurrent semantics. CS_HREDRAW/VREDRAW: redraw on resize.
            style: CS_HREDRAW | CS_VREDRAW | CS_OWNDC,
            lpfnWndProc: Some(window_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinst,
            hIcon: ptr::null_mut(),
            hCursor: ptr::null_mut(),
            hbrBackground: ptr::null_mut(), // No GDI background — GL paints it.
            lpszMenuName: ptr::null(),
            lpszClassName: name.as_ptr(),
            hIconSm: ptr::null_mut(),
        };
        // Ignore the result: 0 with ALREADY_EXISTS is fine.
        let _ = RegisterClassExW(&class);
    });

    Ok(name.as_ptr())
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // Suppress GDI's "erase background" pass. With hbrBackground=NULL the
    // default does nothing visible, but Windows still returns through a
    // BeginPaint cycle that can produce a black flash between two GL
    // SwapBuffers calls. Returning non-zero tells Windows we already
    // erased — GL paints the whole client every frame, so this is true.
    if msg == WM_ERASEBKGND {
        return 1;
    }
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

/// Context slot — glutin uses the type-state pattern (NotCurrent vs
/// PossiblyCurrent) to enforce correct usage at compile time. Our render
/// thread is the only one that ever binds the context, but the surface
/// constructor runs on a different thread, so we store it as NotCurrent
/// initially and transition on the first `make_current` call.
enum ContextSlot {
    NotCurrent(NotCurrentContext),
    Current(PossiblyCurrentContext),
}

/// A WGL-backed video surface positioned over the Tauri window.
///
/// We can't use a child HWND parented to Tauri's main window because
/// WebView2 renders via DirectComposition, and DComp surfaces are
/// composited *above* sibling child HWNDs regardless of Z-order. So mpv
/// would always be hidden behind the WebView's solid bg.
///
/// Instead this is a top-level WS_POPUP window *owned* by the Tauri main
/// HWND. Owner relationship gives us the z-order tracking we want:
/// Windows keeps owned popups above their owner automatically when the
/// owner gets focus, and stacks the popup below other apps when the user
/// alt-tabs away. set_geometry converts client coords (what the frontend
/// ResizeObserver hands us) to screen coords for the popup.
pub struct WindowsVideoSurface {
    hwnd: HWND,
    /// Owner = Tauri's main window. Stored so set_geometry can read its
    /// screen position via GetWindowRect on every reflow.
    owner_hwnd: HWND,
    /// Current backing-store size in physical pixels. We track this
    /// ourselves because glutin's `Surface::resize` is a no-op on WGL —
    /// `Surface::width()/height()` keeps returning the size the surface
    /// was *created* with, even after we've moved the HWND. Letting
    /// mpv render into a stale FBO size with a smaller HWND produces
    /// the "video stuck to the upper-left, black band on the right"
    /// look the user reported.
    cur_w: std::sync::atomic::AtomicI32,
    cur_h: std::sync::atomic::AtomicI32,
    // glutin objects. Order in the struct matters for drop: the context and
    // surface must drop before the display. Rust drops fields top-to-bottom,
    // so list them context → surface → display.
    context: Mutex<Option<ContextSlot>>,
    surface: Surface<WindowSurface>,
    display: Display,
}

unsafe impl Send for WindowsVideoSurface {}
unsafe impl Sync for WindowsVideoSurface {}

impl WindowsVideoSurface {
    /// Create a child HWND beneath `parent` and bind a WGL context to it.
    /// The child starts at 0,0 with the parent's client size; callers should
    /// reposition via [`set_geometry`] once the WebView lays out the
    /// transparent video region.
    pub fn new(owner_hwnd: HWND, w: i32, h: i32) -> Result<Self> {
        let class_name = ensure_class_registered()?;
        let hinst = unsafe { GetModuleHandleW(ptr::null()) };

        let title = wide("");
        let hwnd: HWND = unsafe {
            CreateWindowExW(
                // WS_EX_NOACTIVATE: clicks/focus go to the Tauri window
                //   beneath us, not the popup.
                // WS_EX_TOOLWINDOW: don't show in alt-tab or taskbar.
                // WS_EX_LAYERED + WS_EX_TRANSPARENT: full mouse-event
                //   pass-through. WM_NCHITTEST → HTTRANSPARENT alone is
                //   not enough on top-level windows — only the layered
                //   window manager handles top-level click-through
                //   correctly. We immediately set α=255 below so GL
                //   rendering shows fully opaque.
                WS_EX_LAYERED | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_TRANSPARENT,
                class_name,
                title.as_ptr(),
                // WS_POPUP: top-level borderless. With owner_hwnd passed as
                // hWndParent below, this becomes an *owned* popup — Windows
                // keeps it above the owner in z-order automatically.
                // WS_CLIPSIBLINGS/CHILDREN: don't smear other windows.
                WS_POPUP | WS_CLIPSIBLINGS | WS_CLIPCHILDREN,
                0,
                0,
                w.max(1),
                h.max(1),
                owner_hwnd,        // owner — popup floats above this window
                ptr::null_mut(),   // no menu
                hinst,
                ptr::null(),
            )
        };
        if hwnd.is_null() {
            bail!("CreateWindowExW for video surface returned NULL");
        }

        // Layered window with α=255: opaque to compositor, but participates
        // in the layered-window click-through machinery so WS_EX_TRANSPARENT
        // actually delivers right-click + drag-drop to the owner.
        unsafe {
            SetLayeredWindowAttributes(hwnd, 0, 255, LWA_ALPHA);
        }

        // Wrap our HWND in raw-window-handle to feed glutin. windows-sys 0.59
        // exposes HWND as `*mut c_void`; raw-window-handle 0.6 wants the
        // numeric value as NonZeroIsize, so cast through that.
        let mut win_handle = Win32WindowHandle::new(
            std::num::NonZeroIsize::new(hwnd as isize)
                .ok_or_else(|| anyhow!("hwnd is zero"))?,
        );
        win_handle.hinstance = std::num::NonZeroIsize::new(hinst as isize);
        let raw_window = RawWindowHandle::Win32(win_handle);
        let raw_display = RawDisplayHandle::Windows(WindowsDisplayHandle::new());

        // WGL display. Pass our window so glutin can pick a pixel format
        // compatible with it.
        let display = unsafe {
            Display::new(raw_display, DisplayApiPreference::Wgl(Some(raw_window)))
                .map_err(|e| anyhow!("create WGL display: {e}"))?
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
                .ok_or_else(|| anyhow!("no compatible WGL config found"))?
        };

        let context_attrs = ContextAttributesBuilder::new()
            // mpv uses GL 2.1+ functions. Don't pin a version; let the driver
            // pick the highest profile available. mpv selects the path it
            // needs at runtime via get_proc_address.
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

        // Stay un-current. WGL contexts are thread-affine — binding here on
        // the Tauri setup thread would make the render thread's first
        // make_current fail with ERROR_BUSY. Hand the un-current context to
        // the render thread and let it claim ownership.
        Ok(Self {
            hwnd,
            owner_hwnd,
            cur_w: std::sync::atomic::AtomicI32::new(w.max(1)),
            cur_h: std::sync::atomic::AtomicI32::new(h.max(1)),
            display,
            surface,
            context: Mutex::new(Some(ContextSlot::NotCurrent(not_current))),
        })
    }
}

impl VideoSurface for WindowsVideoSurface {
    fn make_current(&self) -> Result<()> {
        let mut guard = self
            .context
            .lock()
            .map_err(|_| anyhow!("video surface mutex poisoned"))?;
        // Two-phase: the very first call promotes NotCurrent → Current and
        // claims the context for the calling (render) thread. Subsequent
        // calls just rebind on the same thread — cheap.
        match guard.take() {
            Some(ContextSlot::NotCurrent(ctx)) => {
                let now = ctx
                    .make_current(&self.surface)
                    .map_err(|e| anyhow!("make_current (first): {e}"))?;
                // Cap the swap rate to the display refresh. Without vsync
                // the render thread issues SwapBuffers as fast as mpv
                // calls update_callback, which on some Win11 drivers
                // produces visible tearing/flicker because the popup
                // window's compositor present is racing the GL flip.
                if let Err(e) = self
                    .surface
                    .set_swap_interval(&now, SwapInterval::Wait(std::num::NonZeroU32::new(1).unwrap()))
                {
                    eprintln!("[unflick-render] set_swap_interval failed (continuing): {e}");
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
        // glutin display knows how to resolve via WGL.
        self.display.get_proc_address(cname.as_c_str()) as *mut c_void
    }

    fn set_geometry(&self, x: i32, y: i32, w: i32, h: i32) -> Result<()> {
        // (x, y, w, h) arrive in *CSS / logical* pixels — that's what
        // getBoundingClientRect on the WebView gives the frontend. Win32
        // SetWindowPos on a PerMonitorV2-aware process uses *physical*
        // monitor pixels. On a 100% DPI monitor those match; on a 125%
        // monitor (very common on Windows), the popup ends up 80% of the
        // size React intended — short on the right and bottom of the
        // video region. Convert to physical px before talking to Win32.
        //
        // GetDpiForWindow tracks the monitor the owner is currently on,
        // so dragging the window across monitors with different DPIs
        // reports the new value; the WM_DPICHANGED that comes with the
        // move re-layouts the WebView, which fires ResizeObserver, which
        // calls back into here with fresh logical coords — so we always
        // scale by the *current* monitor's DPI.
        let dpi = unsafe { GetDpiForWindow(self.owner_hwnd) };
        let scale = if dpi == 0 { 1.0 } else { dpi as f64 / 96.0 };
        let scaled_x = (x as f64 * scale).round() as i32;
        let scaled_y = (y as f64 * scale).round() as i32;
        let scaled_w = ((w as f64 * scale).round() as i32).max(1);
        let scaled_h = ((h as f64 * scale).round() as i32).max(1);

        // (x, y) are in *client* coordinates of the Tauri window — what
        // React's getBoundingClientRect handed the frontend. We need
        // screen coords for the popup.
        //
        // Modern Windows on Win11 keeps invisible "drop shadow" / resize
        // hit-test borders around even decoration-less windows (about 8 px
        // each side, 9 px on top), so GetWindowRect's outer rect is bigger
        // than the actual client area and using its origin as our base
        // would put the popup off by those margins. ClientToScreen maps
        // (0,0) of the *client* area to absolute screen coords, which is
        // exactly the offset we want to add.
        let mut origin = POINT { x: 0, y: 0 };
        let ok = unsafe { ClientToScreen(self.owner_hwnd, &mut origin) };
        if ok == 0 {
            bail!("ClientToScreen on owner failed");
        }
        let screen_x = origin.x + scaled_x;
        let screen_y = origin.y + scaled_y;

        let ok = unsafe {
            SetWindowPos(
                self.hwnd,
                HWND_TOP,
                screen_x,
                screen_y,
                scaled_w,
                scaled_h,
                SWP_NOACTIVATE,
            )
        };
        if ok == 0 {
            bail!("SetWindowPos failed");
        }

        // Update our tracked size so size() returns the new dimensions.
        // mpv reads size via render_to_fbo's w/h param to know what
        // viewport to letterbox into; if we still report the original
        // size after the HWND has shrunk, mpv letterboxes for the
        // larger area and the visible window only sees the upper-left
        // crop — what the user reported as "video not centred".
        self.cur_w
            .store(scaled_w, std::sync::atomic::Ordering::Relaxed);
        self.cur_h
            .store(scaled_h, std::sync::atomic::Ordering::Relaxed);

        // Resize the GL backing surface to match the HWND. Without this,
        // mpv keeps rendering into the original FBO size while the popup
        // is whatever size we last asked Win32 for, so SwapBuffers
        // stretches/clips the result. (Note: glutin::Surface::resize is
        // a no-op on WGL — the size atomic above is the actual fix.)
        if let Ok(guard) = self.context.lock() {
            if let Some(ContextSlot::Current(ctx)) = guard.as_ref() {
                let nw = std::num::NonZeroU32::new(scaled_w as u32).unwrap();
                let nh = std::num::NonZeroU32::new(scaled_h as u32).unwrap();
                self.surface.resize(ctx, nw, nh);
            }
        }
        Ok(())
    }

    fn set_visible(&self, visible: bool) {
        unsafe {
            ShowWindow(self.hwnd, if visible { SW_SHOW } else { SW_HIDE });
        }
    }

    fn set_always_on_top(&self, enabled: bool) {
        unsafe {
            SetWindowPos(
                self.hwnd,
                if enabled { HWND_TOPMOST } else { HWND_NOTOPMOST },
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
    }

    fn set_alpha(&self, alpha: u8) {
        // The popup is WS_EX_LAYERED with LWA_ALPHA. The α byte feeds
        // straight into the layered-window compositor — no GL context
        // change needed and no render-thread synchronisation required.
        unsafe {
            SetLayeredWindowAttributes(self.hwnd, 0, alpha, LWA_ALPHA);
        }
    }

    fn size(&self) -> (i32, i32) {
        // Read from our own tracked size — see comment on cur_w/cur_h.
        // glutin's Surface::width/height stays pinned to the creation
        // size on WGL, which is wrong as soon as set_geometry runs.
        use std::sync::atomic::Ordering;
        (
            self.cur_w.load(Ordering::Relaxed),
            self.cur_h.load(Ordering::Relaxed),
        )
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
