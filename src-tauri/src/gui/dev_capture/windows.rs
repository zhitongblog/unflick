//! Windows: `ICoreWebView2::CapturePreview`.
//!
//! Chosen over `PrintWindow` for two reasons. It produces PNG directly,
//! where `PrintWindow` hands back a raw BGRA DIB and there is no PNG
//! encoder in this tree to turn it into anything. And on Windows it
//! captures the *same* layer anyway: the mpv surface is a separate
//! top-level `WS_POPUP` above the WebView, a different HWND entirely, so a
//! window grab of the player HWND does not include the picture either.
//!
//! # Why there is no blank-pixel check here
//!
//! The other two platforms inspect the pixels before returning them,
//! because on both of them a window nobody is drawing yields a flat
//! rectangle. WebView2 does not work that way: it renders in the browser
//! process and `CapturePreview` reads that composition, so it answers
//! correctly for a window that is covered, off-screen or hidden. A flat
//! result here would mean the *page* is blank — which is a true answer
//! about the interface and the caller's to interpret, not a failure to
//! report. Adding a check would turn a real finding into a refusal.
//!
//! There is no PNG decoder here either, which is the other half of it: the
//! dimensions below are read straight out of the IHDR header rather than by
//! decoding, and that is all the file is opened for.

use std::sync::mpsc;
use std::time::Duration;

use webview2_com::Microsoft::Web::WebView2::Win32::COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG;
use webview2_com::CapturePreviewCompletedHandler;
use windows::Win32::Foundation::HGLOBAL;
use windows::Win32::System::Com::StructuredStorage::CreateStreamOnHGlobal;
use windows::Win32::System::Com::{IStream, STATFLAG_NONAME, STREAM_SEEK_SET};

use crate::core::dev::Shot;

pub fn capture(window: &tauri::WebviewWindow, timeout: Duration) -> Result<Shot, String> {
    let (tx, rx) = mpsc::channel::<Result<Vec<u8>, String>>();
    let sender = tx.clone();

    window
        .with_webview(move |webview| {
            // Runs on the UI thread, which is where WebView2 requires every
            // call on the controller to happen.
            if let Err(e) = unsafe { request(&webview, sender.clone()) } {
                // Nothing else will ever answer, so answer here.
                let _ = sender.send(Err(e));
            }
        })
        .map_err(|e| format!("could not reach the webview: {}", e))?;

    let png = match rx.recv_timeout(timeout) {
        Ok(result) => result?,
        Err(_) => {
            return Err(format!(
                "WebView2 did not produce a capture within {:.0}s. If the window has \
                 just been restored, give it a moment and retry; `unflick dev \
                 snapshot` reads the interface without needing pixels.",
                timeout.as_secs_f64()
            ))
        }
    };

    let (width, height) = png_size(&png)
        .ok_or("WebView2 returned something that is not a PNG")?;
    Ok(Shot { png, width, height })
}

/// Ask WebView2 for a PNG. Returns once the request is in; the bytes
/// arrive on `sender` from the completion handler.
///
/// # Safety
///
/// Must be called on the UI thread with a live `PlatformWebview`.
unsafe fn request(
    webview: &tauri::webview::PlatformWebview,
    sender: mpsc::Sender<Result<Vec<u8>, String>>,
) -> Result<(), String> {
    let core = webview
        .controller()
        .CoreWebView2()
        .map_err(|e| format!("WebView2 has no core view: {}", e))?;

    // `true` for delete-on-release: the HGLOBAL belongs to the stream, and
    // the stream dies with the handler closure.
    let stream: IStream = CreateStreamOnHGlobal(HGLOBAL::default(), true)
        .map_err(|e| format!("could not allocate a buffer for the capture: {}", e))?;

    let readback = stream.clone();
    let handler = CapturePreviewCompletedHandler::create(Box::new(move |result| {
        let answer = match result {
            Ok(()) => read_stream(&readback),
            Err(e) => Err(format!("WebView2 refused the capture: {}", e)),
        };
        let _ = sender.send(answer);
        Ok(())
    }));

    core.CapturePreview(COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG, &stream, &handler)
        .map_err(|e| format!("WebView2 refused the capture: {}", e))
}

/// # Safety
///
/// `stream` must be a stream WebView2 has finished writing to.
unsafe fn read_stream(stream: &IStream) -> Result<Vec<u8>, String> {
    let mut stat = Default::default();
    stream
        .Stat(&mut stat, STATFLAG_NONAME)
        .map_err(|e| format!("could not measure the capture: {}", e))?;
    let size = stat.cbSize as usize;
    if size == 0 {
        return Err("WebView2 produced an empty capture".to_string());
    }

    stream
        .Seek(0, STREAM_SEEK_SET, None)
        .map_err(|e| format!("could not rewind the capture: {}", e))?;

    let mut buffer = vec![0u8; size];
    let mut filled = 0usize;
    while filled < size {
        let mut read = 0u32;
        let hr = stream.Read(
            buffer[filled..].as_mut_ptr() as *mut core::ffi::c_void,
            (size - filled) as u32,
            Some(&mut read),
        );
        if hr.is_err() {
            return Err(format!("could not read the capture back: {}", hr.message()));
        }
        if read == 0 {
            // Short of what Stat promised. Truncating silently would hand
            // back a half-written PNG that decodes to a grey band.
            return Err(format!(
                "the capture stopped short: {} of {} bytes",
                filled, size
            ));
        }
        filled += read as usize;
    }
    Ok(buffer)
}

/// Width and height straight out of the PNG's IHDR chunk.
///
/// Eight bytes of signature, then a chunk header, then width and height as
/// big-endian u32s. Cheaper and far less to go wrong than pulling in a
/// decoder to learn two numbers.
fn png_size(bytes: &[u8]) -> Option<(u32, u32)> {
    const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    if bytes.len() < 24 || bytes[..8] != SIGNATURE || &bytes[12..16] != b"IHDR" {
        return None;
    }
    let width = u32::from_be_bytes(bytes[16..20].try_into().ok()?);
    let height = u32::from_be_bytes(bytes[20..24].try_into().ok()?);
    Some((width, height))
}

#[cfg(test)]
mod tests {
    use super::png_size;

    #[test]
    fn the_header_gives_up_the_size_without_a_decoder() {
        let mut png = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        png.extend_from_slice(&13u32.to_be_bytes());
        png.extend_from_slice(b"IHDR");
        png.extend_from_slice(&1024u32.to_be_bytes());
        png.extend_from_slice(&640u32.to_be_bytes());
        assert_eq!(png_size(&png), Some((1024, 640)));

        assert_eq!(png_size(b"not a png at all, really"), None);
        assert_eq!(png_size(&png[..12]), None);
    }
}
