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
    ContextApi, ContextAttributesBuilder, NotCurrentGlContext, PossiblyCurrentContext,
    PossiblyCurrentGlContext,
};
use glutin::display::{Display, DisplayApiPreference, GlDisplay};
use glutin::surface::{GlSurface, Surface, SurfaceAttributesBuilder, WindowSurface};
use raw_window_handle::{
    RawDisplayHandle, RawWindowHandle, Win32WindowHandle, WindowsDisplayHandle,
};
use windows_sys::core::PCWSTR;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, RegisterClassExW, SetWindowPos, ShowWindow, CS_HREDRAW,
    CS_OWNDC, CS_VREDRAW, SWP_NOACTIVATE, SWP_NOZORDER, SW_HIDE, SW_SHOW, WNDCLASSEXW, WS_CHILD,
    WS_CLIPCHILDREN, WS_CLIPSIBLINGS,
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

/// A WGL-backed video surface parented to a Tauri window's HWND.
pub struct WindowsVideoSurface {
    hwnd: HWND,
    // glutin objects. Order in the struct matters for drop: the surface and
    // context must drop before the display. Rust drops fields top-to-bottom,
    // so list them context → surface → display.
    context: Mutex<Option<PossiblyCurrentContext>>,
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
    pub fn new(parent_hwnd: HWND, w: i32, h: i32) -> Result<Self> {
        let class_name = ensure_class_registered()?;
        let hinst = unsafe { GetModuleHandleW(ptr::null()) };

        let title = wide("");
        let hwnd: HWND = unsafe {
            CreateWindowExW(
                0,
                class_name,
                title.as_ptr(),
                // WS_CHILD: parented inside Tauri's HWND.
                // WS_CLIPSIBLINGS/CHILDREN: don't paint over WebView2's HWND.
                // No WS_VISIBLE — the surface starts hidden so the GUI's
                // existing rendering path (HTML5 video, until P5) keeps
                // working unchanged. Callers flip visibility via
                // `set_visible(true)` after the WebView region is laid out.
                WS_CHILD | WS_CLIPSIBLINGS | WS_CLIPCHILDREN,
                0,
                0,
                w.max(1),
                h.max(1),
                parent_hwnd,
                ptr::null_mut(),
                hinst,
                ptr::null(),
            )
        };
        if hwnd.is_null() {
            bail!("CreateWindowExW for video surface returned NULL");
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

        // Make current once so WGL knows about us. We'll release before
        // returning — the renderer thread will make_current itself.
        let context = not_current
            .make_current(&surface)
            .map_err(|e| anyhow!("make_current: {e}"))?;

        Ok(Self {
            hwnd,
            display,
            surface,
            context: Mutex::new(Some(context)),
        })
    }
}

impl VideoSurface for WindowsVideoSurface {
    fn make_current(&self) -> Result<()> {
        let mut guard = self
            .context
            .lock()
            .map_err(|_| anyhow!("video surface mutex poisoned"))?;
        let ctx = guard
            .as_ref()
            .ok_or_else(|| anyhow!("context already released"))?;
        ctx.make_current(&self.surface)
            .map_err(|e| anyhow!("make_current: {e}"))?;
        // Re-store same context — we use Option only to satisfy Mutex<Option>
        // patterns and to leave room for context replacement (PiP).
        let _ = guard.as_mut();
        Ok(())
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
        let ok = unsafe {
            SetWindowPos(
                self.hwnd,
                ptr::null_mut(), // hwndInsertAfter — ignored due to SWP_NOZORDER
                x,
                y,
                w.max(1),
                h.max(1),
                SWP_NOZORDER | SWP_NOACTIVATE,
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
        let ctx = guard
            .as_ref()
            .ok_or_else(|| anyhow!("context already released"))?;
        self.surface
            .swap_buffers(ctx)
            .map_err(|e| anyhow!("swap_buffers: {e}"))?;
        Ok(())
    }
}
