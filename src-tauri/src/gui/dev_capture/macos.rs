//! macOS: `WKWebView takeSnapshotWithConfiguration:`.
//!
//! Chosen over `CGWindowListCreateImage` deliberately. That one needs the
//! Screen Recording permission — a TCC dialog no unattended agent can ever
//! click — is deprecated since macOS 14, and returns nothing usable while
//! the screen is locked. A verb whose whole job is unattended verification
//! cannot be built on a permission prompt.
//!
//! # afterScreenUpdates, and the wait that never ends
//!
//! `afterScreenUpdates: YES` asks WebKit to hold the snapshot until the
//! next screen update. A window that is minimised, on another Space, or
//! entirely covered may never produce one, and the completion block simply
//! never fires — the caller waits out its whole timeout for no reason.
//!
//! So the flag follows the window: `YES` when AppKit says the window's
//! content is on screen, which is when it is both meaningful and safe, and
//! `NO` otherwise, where it returns the last committed layer contents
//! immediately. Either way the pixels are checked before they are handed
//! back, because "returned quickly" and "returned something" are different
//! claims.

use std::sync::mpsc;
use std::time::Duration;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2_app_kit::{
    NSBitmapImageFileType, NSBitmapImageRep, NSImage, NSWindow, NSWindowOcclusionState,
};
use objc2_foundation::{NSDictionary, NSError, MainThreadMarker};
use objc2_web_kit::{WKSnapshotConfiguration, WKWebView};

use crate::core::dev::Shot;

use super::BLANK_MESSAGE;

pub fn capture(window: &tauri::WebviewWindow, timeout: Duration) -> Result<Shot, String> {
    // Both of these are refusals, not retries: the pixels behind a locked
    // screen or a minimised window are not stale, they do not exist. Naming
    // the cause is the entire difference between this and a black PNG.
    if screen_is_locked() {
        return Err(
            "the screen is locked, so nothing is drawing the window and a capture \
             would be a black rectangle. Unlock the screen and retry; `unflick dev \
             snapshot` and `unflick dev text` read the interface without pixels and \
             work locked."
                .to_string(),
        );
    }
    if window.is_minimized().unwrap_or(false) {
        return Err(
            "the unflick window is minimised, so there is nothing on screen to \
             capture. Restore it and retry."
                .to_string(),
        );
    }

    let (tx, rx) = mpsc::channel::<Result<Shot, String>>();
    let sender = tx.clone();
    window
        .with_webview(move |webview| {
            // `with_webview` runs on the main thread, which is where every
            // AppKit and WebKit call below has to happen.
            let result = unsafe { take_snapshot(&webview, sender.clone()) };
            if let Err(e) = result {
                // The completion block will never fire, so nothing else
                // will ever answer the caller.
                let _ = sender.send(Err(e));
            }
        })
        .map_err(|e| format!("could not reach the webview: {}", e))?;

    match rx.recv_timeout(timeout) {
        Ok(result) => result,
        Err(_) => Err(format!(
            "the window did not produce a snapshot within {:.0}s. WebKit holds a \
             snapshot back until the window next draws, and a window that is \
             covered, on another Space, or minimised may never draw. Bring the \
             unflick window to the front and retry.",
            timeout.as_secs_f64()
        )),
    }
}

/// Ask WKWebView for a picture. Returns as soon as the request is in;
/// the answer arrives on `sender` from the completion block.
///
/// # Safety
///
/// Must be called on the main thread with a live `PlatformWebview`.
unsafe fn take_snapshot(
    webview: &tauri::webview::PlatformWebview,
    sender: mpsc::Sender<Result<Shot, String>>,
) -> Result<(), String> {
    let mtm = MainThreadMarker::new()
        .ok_or("the webview handed us a thread that is not the main one")?;

    let view: &WKWebView = &*webview.inner().cast();
    let ns_window: &NSWindow = &*webview.ns_window().cast();

    // NSWindowOcclusionStateVisible means "some part of this window's
    // content is on screen". When it isn't, waiting for a screen update is
    // waiting for something that is not going to happen.
    let on_screen = ns_window
        .occlusionState()
        .contains(NSWindowOcclusionState::Visible);

    let config = WKSnapshotConfiguration::new(mtm);
    config.setAfterScreenUpdates(on_screen);

    let block = RcBlock::new(move |image: *mut NSImage, error: *mut NSError| {
        let result = if image.is_null() {
            let detail = if error.is_null() {
                "WebKit gave no reason".to_string()
            } else {
                (*error).localizedDescription().to_string()
            };
            Err(format!("the webview would not produce a snapshot: {}", detail))
        } else {
            encode_png(&*image)
        };
        let _ = sender.send(result);
    });

    view.takeSnapshotWithConfiguration_completionHandler(Some(&config), &block);
    Ok(())
}

/// NSImage → PNG bytes, with the pixels inspected on the way past.
///
/// # Safety
///
/// Must be called on the main thread.
unsafe fn encode_png(image: &NSImage) -> Result<Shot, String> {
    let tiff = image
        .TIFFRepresentation()
        .ok_or("the snapshot had no bitmap in it")?;
    let rep: Retained<NSBitmapImageRep> = NSBitmapImageRep::imageRepWithData(&tiff)
        .ok_or("the snapshot's bitmap could not be read")?;

    let width = rep.pixelsWide().max(0) as usize;
    let height = rep.pixelsHigh().max(0) as usize;
    if width == 0 || height == 0 {
        return Err("the snapshot came back with no pixels in it".to_string());
    }

    // Planar reps store each channel in a separate buffer, so the
    // interleaved read below would be wrong. WebKit does not produce them,
    // and if that ever changes the honest answer is to say so rather than
    // to check the wrong bytes.
    if !rep.isPlanar() {
        let stride = rep.bytesPerRow().max(0) as usize;
        let bpp = (rep.bitsPerPixel().max(0) as usize) / 8;
        let data = rep.bitmapData();
        if !data.is_null() && stride > 0 && bpp > 0 {
            let pixels = std::slice::from_raw_parts(data as *const u8, stride * height);
            if super::looks_blank(pixels, stride, width, height, bpp) {
                return Err(BLANK_MESSAGE.to_string());
            }
        }
    }

    let png = rep
        .representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new())
        .ok_or("the snapshot could not be encoded as PNG")?;

    Ok(Shot {
        png: png.to_vec(),
        width: width as u32,
        height: height as u32,
    })
}

/// Is the login window in front of everything.
///
/// Worth its own check rather than being folded into "the capture was
/// blank": a locked screen is a thing the person running this can do
/// something about in one gesture, and a message that says so beats a
/// message listing four possibilities.
fn screen_is_locked() -> bool {
    use core_foundation::base::{CFType, TCFType};
    use core_foundation::boolean::CFBoolean;
    use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
    use core_foundation::string::CFString;

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGSessionCopyCurrentDictionary() -> CFDictionaryRef;
    }

    unsafe {
        let raw = CGSessionCopyCurrentDictionary();
        if raw.is_null() {
            // No session dictionary at all — a daemon context, or a login
            // window with nothing behind it. Not evidence of a lock, and
            // guessing "locked" here would refuse captures that work.
            return false;
        }
        let session: CFDictionary<CFString, CFType> = CFDictionary::wrap_under_create_rule(raw);
        let key = CFString::new("CGSSessionScreenIsLocked");
        match session.find(&key) {
            Some(value) => value
                .downcast::<CFBoolean>()
                .map(bool::from)
                .unwrap_or(false),
            // The key is absent entirely when the screen is unlocked.
            None => false,
        }
    }
}
