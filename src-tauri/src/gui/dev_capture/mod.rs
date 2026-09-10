//! A picture of the interface layer, on three platforms.
//!
//! This is the one dev verb that is not a JavaScript program, and the only
//! place in the bridge where per-platform code exists at all. Everything
//! else — click, text, snapshot, wait — is one file that runs unchanged
//! everywhere, which is what makes a platform-specific bug in those verbs
//! impossible rather than merely unlikely.
//!
//! # It captures the interface, not the film
//!
//! Each platform's *webview* snapshot API is used, not a window grab:
//!
//!   * macOS — `WKWebView takeSnapshotWithConfiguration:`
//!   * Windows — `ICoreWebView2::CapturePreview`
//!   * Linux — `webkit_web_view_get_snapshot`
//!
//! The alternative, grabbing the OS window, does not mean the same thing on
//! the three platforms: mpv draws on its own surface, a `WS_POPUP` *above*
//! the webview on Windows and a child *below* it on macOS and Linux, so one
//! grab API would produce a different composite per platform. And on macOS
//! `CGWindowListCreateImage` needs Screen Recording permission — a TCC
//! dialog no agent can click, which would make the verb unusable in exactly
//! the unattended case it exists for.
//!
//! So `dev capture` shows the UI. The decoded picture comes from mpv, via
//! `unflick frame capture` / `describe_frame`, at higher fidelity than any
//! compositor grab. Compositing the two here would fabricate a screenshot
//! no compositor ever produced — and on Windows the popup's geometry versus
//! the webview's is precisely the thing worth *verifying* rather than
//! assuming.
//!
//! # A blank picture is a failure, not a picture
//!
//! A capture of a window nothing is drawing comes back as a flat rectangle
//! of one colour. Returning that would be the exact failure this whole
//! track exists to remove: a verification that passes having looked at
//! nothing. So every platform that can hand back raw pixels checks them,
//! and a flat result is refused with the causes named.

use std::time::Duration;

use crate::core::dev::Shot;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "linux")]
mod linux;

/// What to say when the pixels came back but there was nothing in them.
pub(crate) const BLANK_MESSAGE: &str =
    "the capture came back a single flat colour, so nothing is being drawn into the \
     window — it is fully covered, on another desktop, or the page has not painted \
     yet. Bring the unflick window to the front and retry; `unflick dev snapshot` \
     reads the interface without needing pixels at all.";

pub fn capture(window: &tauri::WebviewWindow, timeout: Duration) -> Result<Shot, String> {
    #[cfg(target_os = "macos")]
    {
        macos::capture(window, timeout)
    }
    #[cfg(target_os = "windows")]
    {
        windows::capture(window, timeout)
    }
    #[cfg(target_os = "linux")]
    {
        linux::capture(window, timeout)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        let _ = (window, timeout);
        Err(format!(
            "capturing the window is not implemented on {} — the other five dev \
             commands work here",
            std::env::consts::OS
        ))
    }
}

/// Is every sampled pixel the same colour.
///
/// Sampled on a grid rather than exhaustively: a 3000×2000 capture is six
/// million pixels and the answer does not get truer for reading all of
/// them. One pixel that differs is enough to prove the window drew
/// something, and the grid is dense enough (64×64) that a real interface
/// cannot slip through it.
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub(crate) fn looks_blank(
    pixels: &[u8],
    stride: usize,
    width: usize,
    height: usize,
    bytes_per_pixel: usize,
) -> bool {
    if width == 0 || height == 0 || bytes_per_pixel == 0 || pixels.len() < bytes_per_pixel {
        return true;
    }
    let first = &pixels[..bytes_per_pixel];
    let step_x = (width / 64).max(1);
    let step_y = (height / 64).max(1);

    let mut y = 0;
    while y < height {
        let mut x = 0;
        while x < width {
            let offset = y * stride + x * bytes_per_pixel;
            if offset + bytes_per_pixel <= pixels.len()
                && &pixels[offset..offset + bytes_per_pixel] != first
            {
                return false;
            }
            x += step_x;
        }
        y += step_y;
    }
    true
}

#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
mod tests {
    use super::looks_blank;

    #[test]
    fn a_flat_rectangle_is_recognised_as_nothing() {
        let flat = vec![0u8; 16 * 16 * 4];
        assert!(looks_blank(&flat, 16 * 4, 16, 16, 4));

        // Black is the interesting case: it is what an undrawn window and a
        // dark player both look like at a glance, and only a single
        // differing pixel tells them apart.
        let mut painted = vec![0u8; 16 * 16 * 4];
        painted[(9 * 16 + 3) * 4] = 1;
        assert!(!looks_blank(&painted, 16 * 4, 16, 16, 4));
    }

    #[test]
    fn a_short_or_empty_buffer_is_not_mistaken_for_a_picture() {
        assert!(looks_blank(&[], 0, 0, 0, 4));
        assert!(looks_blank(&[1, 2], 8, 2, 2, 4));
    }
}
