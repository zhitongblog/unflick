//! Serving one local file over HTTP, for a television to fetch.
//!
//! DLNA casting is a pull, not a push: the renderer is handed a URL and
//! goes and gets the bytes itself. Which means casting a file off this
//! machine requires this machine to be an HTTP server for as long as the
//! television is watching.
//!
//! It serves exactly one file, to whoever asks, on an ephemeral port, and
//! only while a cast is running. That is the whole of it — no directory
//! listing, no second path, nothing that outlives the cast.
//!
//! ## Ranges are not optional
//!
//! Televisions seek by asking for byte ranges, and many of them open the
//! stream with `Range: bytes=0-` and refuse anything that answers 200 to
//! it. A server without range support looks like a player that cannot
//! seek, or like one that will not start at all.

use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::{anyhow, Result};

/// How much to move between the file and the socket at a time.
const CHUNK: usize = 64 * 1024;

/// A running one-file HTTP server.
pub struct MediaServer {
    path: PathBuf,
    port: u16,
    running: Arc<AtomicBool>,
}

impl MediaServer {
    /// Bind an ephemeral port on every interface and start serving `path`.
    ///
    /// Every interface, not loopback: the point is to be reachable from the
    /// television, which is on the network rather than on this machine.
    pub fn start(path: PathBuf) -> Result<Self> {
        if !path.is_file() {
            return Err(anyhow!("{} is not a file", path.display()));
        }
        let listener = TcpListener::bind("0.0.0.0:0")?;
        let port = listener.local_addr()?.port();
        let running = Arc::new(AtomicBool::new(true));

        let file = path.clone();
        let flag = Arc::clone(&running);
        std::thread::Builder::new()
            .name("unflick-mediaserver".into())
            .spawn(move || {
                for stream in listener.incoming() {
                    if !flag.load(Ordering::Relaxed) {
                        break;
                    }
                    let Ok(stream) = stream else { continue };
                    let file = file.clone();
                    // A thread per connection: a television opens one or
                    // two, and a pool would be machinery for a load that
                    // does not exist.
                    let _ = std::thread::Builder::new()
                        .name("unflick-mediaserver-conn".into())
                        .spawn(move || {
                            if let Err(e) = handle(stream, &file) {
                                eprintln!("[unflick] media server: {e}");
                            }
                        });
                }
            })?;

        Ok(Self { path, port, running })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The URL to hand a renderer, as seen from `peer`.
    ///
    /// The address has to be the one *that television* can reach, and a
    /// machine with a VPN, a Docker bridge and two network cards has
    /// several. Asking the routing table which local address it would use
    /// to reach the renderer is the only answer that is right on all of
    /// them.
    pub fn url_for(&self, peer: IpAddr) -> Result<String> {
        let local = local_address_towards(peer)?;
        Ok(format!(
            "http://{}:{}/{}",
            local,
            self.port,
            super::http::url_encode(&self.file_name())
        ))
    }

    /// The served file's name, kept in the URL because some renderers show
    /// it and others guess the container from it.
    pub fn file_name(&self) -> String {
        self.path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "media".into())
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::Relaxed);
        // Unblock the accept loop so the thread notices and exits.
        let _ = TcpStream::connect(("127.0.0.1", self.port));
    }
}

impl Drop for MediaServer {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Which of this machine's addresses reaches `peer`.
///
/// No packet is sent — connecting a UDP socket only fixes the route — but
/// the kernel picks the source address it would use, which is exactly the
/// question.
pub fn local_address_towards(peer: IpAddr) -> Result<IpAddr> {
    let sock = UdpSocket::bind(if peer.is_ipv4() { "0.0.0.0:0" } else { "[::]:0" })?;
    sock.connect(SocketAddr::new(peer, 9))?;
    Ok(sock.local_addr()?.ip())
}

/// Serve one request.
fn handle(mut stream: TcpStream, path: &Path) -> Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);

    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("").to_ascii_uppercase();

    let mut range_header = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            break;
        }
        if let Some((name, value)) = trimmed.split_once(':') {
            if name.trim().eq_ignore_ascii_case("range") {
                range_header = Some(value.trim().to_string());
            }
        }
    }

    // Only GET and HEAD. A renderer sends HEAD first surprisingly often, to
    // find the size and content type before committing to the stream.
    if method != "GET" && method != "HEAD" {
        write_status(&mut stream, 405, "Method Not Allowed")?;
        finish(stream);
        return Ok(());
    }

    let total = std::fs::metadata(path)?.len();
    // The path in the request is ignored on purpose: this server has one
    // file, and a renderer that re-encodes the URL differently than we
    // wrote it should still get the media rather than a 404.
    let (start, end) = match range_header.as_deref().map(|h| parse_range(h, total)) {
        Some(Some(r)) => r,
        Some(None) => {
            // A range that cannot be satisfied has its own status, and
            // televisions do act on it.
            let mut head = format!("HTTP/1.1 416 Range Not Satisfiable\r\n");
            head.push_str(&format!("Content-Range: bytes */{}\r\n", total));
            head.push_str("Connection: close\r\n\r\n");
            stream.write_all(head.as_bytes())?;
            finish(stream);
            return Ok(());
        }
        None => (0, total.saturating_sub(1)),
    };
    let partial = range_header.is_some();
    let length = end.saturating_sub(start) + 1;

    let mut head = String::new();
    head.push_str(if partial {
        "HTTP/1.1 206 Partial Content\r\n"
    } else {
        "HTTP/1.1 200 OK\r\n"
    });
    head.push_str(&format!("Content-Type: {}\r\n", content_type(path)));
    head.push_str(&format!("Content-Length: {}\r\n", length));
    head.push_str("Accept-Ranges: bytes\r\n");
    if partial {
        head.push_str(&format!(
            "Content-Range: bytes {}-{}/{}\r\n",
            start, end, total
        ));
    }
    // DLNA renderers look for these two. Without them some will fetch the
    // whole file before showing a frame, and some refuse outright.
    head.push_str("transferMode.dlna.org: Streaming\r\n");
    head.push_str(
        "contentFeatures.dlna.org: DLNA.ORG_OP=01;DLNA.ORG_FLAGS=01700000000000000000000000000000\r\n",
    );
    head.push_str("Connection: close\r\n\r\n");
    stream.write_all(head.as_bytes())?;

    if method == "HEAD" {
        finish(stream);
        return Ok(());
    }

    let mut f = std::fs::File::open(path)?;
    f.seek(SeekFrom::Start(start))?;
    let mut remaining = length;
    let mut buf = vec![0u8; CHUNK];
    while remaining > 0 {
        let want = CHUNK.min(remaining as usize);
        let read = f.read(&mut buf[..want])?;
        if read == 0 {
            break;
        }
        // A television that stops watching closes the socket, and that is
        // a normal end to a cast rather than an error worth reporting.
        if stream.write_all(&buf[..read]).is_err() {
            break;
        }
        remaining -= read as u64;
    }
    finish(stream);
    Ok(())
}

/// Close a connection so the client gets the response rather than a reset.
///
/// Dropping a socket that still has unread bytes in its receive buffer makes
/// Windows send an RST, and an RST throws away whatever the client had not
/// read yet — so a perfectly good response arrives as
/// "ConnectionReset" instead. It showed up as a flaky test failing one run
/// in three; on a television it would be an occasional refusal to start.
///
/// So: say there is nothing more coming, then read whatever the client had
/// already sent before letting go.
fn finish(mut stream: TcpStream) {
    let _ = stream.shutdown(std::net::Shutdown::Write);
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_millis(200)));
    let mut sink = [0u8; 1024];
    // Bounded: a client that keeps talking after being told the answer does
    // not get to hold the thread open.
    for _ in 0..16 {
        match stream.read(&mut sink) {
            Ok(0) | Err(_) => break,
            Ok(_) => continue,
        }
    }
}

fn write_status(stream: &mut TcpStream, code: u16, text: &str) -> Result<()> {
    stream.write_all(
        format!("HTTP/1.1 {} {}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n", code, text)
            .as_bytes(),
    )?;
    Ok(())
}

/// `bytes=0-`, `bytes=500-999`, `bytes=-500`.
///
/// Returns `None` when the range cannot be satisfied, which is a different
/// answer from "there was no range" and gets a different status code.
pub fn parse_range(header: &str, total: u64) -> Option<(u64, u64)> {
    let spec = header.trim().strip_prefix("bytes=")?.trim();
    // Multiple ranges are legal and no renderer sends them; honouring only
    // the first is what every media server does.
    let spec = spec.split(',').next()?.trim();
    let (start_s, end_s) = spec.split_once('-')?;

    if total == 0 {
        return None;
    }
    let last = total - 1;

    if start_s.is_empty() {
        // A suffix range: the final N bytes.
        let n: u64 = end_s.trim().parse().ok()?;
        if n == 0 {
            return None;
        }
        return Some((total.saturating_sub(n), last));
    }

    let start: u64 = start_s.trim().parse().ok()?;
    if start > last {
        return None;
    }
    let end = if end_s.trim().is_empty() {
        last
    } else {
        end_s.trim().parse::<u64>().ok()?.min(last)
    };
    if end < start {
        return None;
    }
    Some((start, end))
}

/// A content type from the extension.
///
/// Renderers route on this: get it wrong and a television will refuse a
/// file it can play perfectly well.
pub fn content_type(path: &Path) -> &'static str {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "mp4" | "m4v" => "video/mp4",
        "mkv" => "video/x-matroska",
        "avi" => "video/x-msvideo",
        "mov" => "video/quicktime",
        "wmv" => "video/x-ms-wmv",
        "webm" => "video/webm",
        "ts" | "m2ts" | "mts" => "video/mp2t",
        "mpg" | "mpeg" => "video/mpeg",
        "flv" => "video/x-flv",
        "mp3" => "audio/mpeg",
        "m4a" | "aac" => "audio/mp4",
        "flac" => "audio/flac",
        "wav" => "audio/wav",
        "ogg" | "oga" => "audio/ogg",
        "opus" => "audio/opus",
        _ => "application/octet-stream",
    }
}

/// The UPnP class a renderer expects in the item's metadata.
pub fn upnp_class(path: &Path) -> &'static str {
    if content_type(path).starts_with("audio/") {
        "object.item.audioItem.musicTrack"
    } else {
        "object.item.videoItem"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges_are_read_the_way_a_television_writes_them() {
        assert_eq!(parse_range("bytes=0-", 1000), Some((0, 999)));
        assert_eq!(parse_range("bytes=500-999", 1000), Some((500, 999)));
        assert_eq!(parse_range("bytes=500-", 1000), Some((500, 999)));
        // A suffix range: the last 100 bytes.
        assert_eq!(parse_range("bytes=-100", 1000), Some((900, 999)));
        // An end past the file is clamped, not refused — televisions do
        // this when they guess at the size.
        assert_eq!(parse_range("bytes=0-99999", 1000), Some((0, 999)));
        // Only the first of several.
        assert_eq!(parse_range("bytes=0-99,200-299", 1000), Some((0, 99)));
    }

    #[test]
    fn an_unsatisfiable_range_is_told_apart_from_no_range() {
        // The distinction matters: one is a 416, the other a 200.
        assert_eq!(parse_range("bytes=1000-", 1000), None);
        assert_eq!(parse_range("bytes=5-4", 1000), None);
        assert_eq!(parse_range("bytes=abc-", 1000), None);
        assert_eq!(parse_range("seconds=0-", 1000), None);
        assert_eq!(parse_range("bytes=0-", 0), None);
    }

    #[test]
    fn the_content_type_follows_the_container() {
        assert_eq!(content_type(Path::new("a.mkv")), "video/x-matroska");
        assert_eq!(content_type(Path::new("a.MP4")), "video/mp4");
        assert_eq!(content_type(Path::new("a.flac")), "audio/flac");
        assert_eq!(content_type(Path::new("a.unknown")), "application/octet-stream");
    }

    #[test]
    fn audio_and_video_get_different_upnp_classes() {
        // A renderer that is handed a video class for an mp3 will often
        // show a black screen rather than play it.
        assert_eq!(upnp_class(Path::new("a.mp3")), "object.item.audioItem.musicTrack");
        assert_eq!(upnp_class(Path::new("a.mkv")), "object.item.videoItem");
    }
}
