//! Linux: `webkit_web_view_get_snapshot`.
//!
//! Chosen over `XGetImage` for the same reason as on the other two
//! platforms: it is the *webview's* own picture, so it means the same thing
//! here as it does there, and it needs no compositor cooperation. Under a
//! window manager the visible region is the window; under `xvfb-run` with a
//! mapped window it is the same.
//!
//! `SnapshotRegion::Visible`, not `FullDocument`: the interface is a
//! fixed-viewport app with no scrolling document, and the visible region is
//! what a person is actually looking at. A full-document snapshot of a page
//! that fills the viewport is the same picture with a slower path to it.

use std::sync::mpsc;
use std::time::Duration;

use webkit2gtk::gio;
use webkit2gtk::{SnapshotOptions, SnapshotRegion, WebViewExt};

use crate::core::dev::Shot;

use super::BLANK_MESSAGE;

pub fn capture(window: &tauri::WebviewWindow, timeout: Duration) -> Result<Shot, String> {
    let (tx, rx) = mpsc::channel::<Result<Shot, String>>();
    let sender = tx.clone();

    window
        .with_webview(move |webview| {
            // Runs on the GTK main thread, which is where `snapshot` must
            // be called from — it asserts on the main context otherwise.
            webview.inner().snapshot(
                SnapshotRegion::Visible,
                SnapshotOptions::NONE,
                None::<&gio::Cancellable>,
                move |result| {
                    let _ = sender.send(match result {
                        Ok(surface) => encode_png(surface),
                        Err(e) => Err(format!(
                            "WebKit would not produce a snapshot: {}",
                            e
                        )),
                    });
                },
            );
        })
        .map_err(|e| format!("could not reach the webview: {}", e))?;

    match rx.recv_timeout(timeout) {
        Ok(result) => result,
        Err(_) => Err(format!(
            "WebKit did not produce a snapshot within {:.0}s. A window the window \
             manager has never mapped has no visible region to snapshot — show the \
             unflick window and retry; `unflick dev snapshot` reads the interface \
             without needing pixels.",
            timeout.as_secs_f64()
        )),
    }
}

fn encode_png(surface: cairo::Surface) -> Result<Shot, String> {
    surface.flush();
    let mut image = cairo::ImageSurface::try_from(surface).map_err(|s| {
        format!(
            "WebKit produced a {:?} surface rather than an image one, which cannot \
             be encoded",
            s.type_()
        )
    })?;

    let width = image.width().max(0) as usize;
    let height = image.height().max(0) as usize;
    if width == 0 || height == 0 {
        return Err(
            "the snapshot came back with no pixels in it, which is what an unmapped \
             window produces"
                .to_string(),
        );
    }
    let stride = image.stride().max(0) as usize;

    {
        // ARGB32 and RGB24 are both four bytes per pixel in cairo. Anything
        // else and the stride arithmetic below would be reading the wrong
        // bytes, so the check is skipped rather than made up.
        let format = image.format();
        if matches!(format, cairo::Format::ARgb32 | cairo::Format::Rgb24) {
            let data = image
                .data()
                .map_err(|e| format!("the snapshot's pixels could not be read: {}", e))?;
            if super::looks_blank(&data, stride, width, height, 4) {
                return Err(BLANK_MESSAGE.to_string());
            }
        }
    }

    let mut png = Vec::new();
    image
        .write_to_png(&mut png)
        .map_err(|e| format!("the snapshot could not be encoded as PNG: {}", e))?;

    Ok(Shot {
        png,
        width: width as u32,
        height: height as u32,
    })
}
