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
use glutin::surface::{GlSurface, Surface, SurfaceAttributesBuilder, WindowSurface};
use raw_window_handle::{
    RawDisplayHandle, RawWindowHandle, Win32WindowHandle, WindowsDisplayHandle,
};
use windows_sys::core::PCWSTR;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::ClientToScreen;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, RegisterClassExW, SetLayeredWindowAttributes, SetWindowPos,
    ShowWindow, CS_HREDRAW, CS_OWNDC, CS_VREDRAW, HWND_TOP, LWA_ALPHA, SWP_NOACTIVATE, SW_HIDE,
    SW_SHOW, WNDCLASSEXW, WS_CLIPCHILDREN, WS_CLIPSIBLINGS, WS_EX_LAYERED, WS_EX_NOACTIVATE,
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
                //   pass-through. WS_EX_TRANSPARENT alone is documented
                //   as click-through but in practice WebView2 still ate
                //   right-click events through it; pairing with LAYERED
                //   gives the OS-recognised "click-through layered
                //   window" combo. We immediately set α=255 below so
                //   GL rendering is unaffected — modern Windows treats
                //   layered+opaque windows as a normal HW-accelerated
                //   window for compositing purposes.
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

        // Layered window with α=255 means "fully opaque, but participate in
        // the layered-window click-through machinery". Required so the
        // WS_EX_TRANSPARENT bit actually delivers right-click + drag-drop
        // events through to the owner.
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
        let screen_x = origin.x + x;
        let screen_y = origin.y + y;

        let ok = unsafe {
            SetWindowPos(
                self.hwnd,
                HWND_TOP,
                screen_x,
                screen_y,
                w.max(1),
                h.max(1),
                SWP_NOACTIVATE,
            )
        };
        if ok == 0 {
            bail!("SetWindowPos failed");
        }
        Ok(())
    }

    fn set_visible(&self, visible: bool) {
        unsafe {
            ShowWindow(self.hwnd, if visible { SW_SHOW } else { SW_HIDE });
        }
    }

    fn size(&self) -> (i32, i32) {
        // We track size implicitly through the surface — query glutin.
        // Surface holds the size we passed at creation; resize is a separate
        // op (see resize_surface in P5).
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
