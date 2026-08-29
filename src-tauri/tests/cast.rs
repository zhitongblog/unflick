//! Casting, as far as it can be driven without a television.
//!
//! Two of the three halves of DLNA are testable here, and they are the two
//! that carry the bytes and the commands:
//!
//! * the HTTP server this machine becomes, which a renderer fetches from —
//!   exercised with real requests, including the byte ranges televisions
//!   actually send;
//! * the SOAP conversation, driven against a stand-in AVTransport service
//!   that records what it was asked to do.
//!
//! What is *not* covered is SSDP discovery, which needs something on the
//! network to answer. That is the part to check against a real television.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Duration;

mod common;

use common::Daemon;
use serde_json::json;
use unflick_lib::core::dlna;
use unflick_lib::core::mediaserver::MediaServer;

// ─── The file server ──────────────────────────────────────────────────────

fn temp_media(name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(name);
    std::fs::write(&p, bytes).expect("write media");
    p
}

/// Raw HTTP, because the point is the exact status line and headers a
/// television sees — a client that normalises them would hide the failures
/// worth catching.
fn request(port: u16, raw: &str) -> (String, Vec<u8>) {
    let mut s = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    s.write_all(raw.as_bytes()).unwrap();
    // Say the request is finished. A server closing a socket that still
    // has unread bytes waiting on it resets the connection on Windows, and
    // the reset takes the response with it.
    s.shutdown(std::net::Shutdown::Write).unwrap();
    let mut all = Vec::new();
    s.read_to_end(&mut all).unwrap();
    let split = all.windows(4).position(|w| w == b"\r\n\r\n").unwrap_or_else(|| {
        panic!(
            "no complete response ({} bytes) to {:?}: {:?}",
            all.len(),
            raw.lines().next().unwrap_or(""),
            String::from_utf8_lossy(&all[..all.len().min(400)])
        )
    });
    let head = String::from_utf8_lossy(&all[..split]).into_owned();
    (head, all[split + 4..].to_vec())
}

#[test]
fn a_whole_file_is_served_with_the_right_type_and_length() {
    let body: Vec<u8> = (0..1000u32).map(|i| (i % 251) as u8).collect();
    let path = temp_media("unflick-cast-full.mp4", &body);
    let server = MediaServer::start(path.clone()).expect("start");

    let (head, got) = request(
        server.port(),
        "GET /unflick-cast-full.mp4 HTTP/1.1\r\nHost: x\r\n\r\n",
    );
    assert!(head.starts_with("HTTP/1.1 200 OK"), "{head}");
    assert!(head.contains("Content-Type: video/mp4"), "{head}");
    assert!(head.contains("Content-Length: 1000"), "{head}");
    // Televisions check this before they will try to seek at all.
    assert!(head.contains("Accept-Ranges: bytes"), "{head}");
    assert!(head.contains("transferMode.dlna.org: Streaming"), "{head}");
    assert_eq!(got, body);

    let _ = std::fs::remove_file(path);
}

#[test]
fn a_range_request_gets_exactly_those_bytes() {
    let body: Vec<u8> = (0..1000u32).map(|i| (i % 251) as u8).collect();
    let path = temp_media("unflick-cast-range.mp4", &body);
    let server = MediaServer::start(path.clone()).expect("start");

    let (head, got) = request(
        server.port(),
        "GET /x HTTP/1.1\r\nHost: x\r\nRange: bytes=500-599\r\n\r\n",
    );
    assert!(head.starts_with("HTTP/1.1 206 Partial Content"), "{head}");
    assert!(head.contains("Content-Range: bytes 500-599/1000"), "{head}");
    assert!(head.contains("Content-Length: 100"), "{head}");
    assert_eq!(got, body[500..600]);

    // The open-ended form, which is how a stream usually starts.
    let (head, got) = request(
        server.port(),
        "GET /x HTTP/1.1\r\nHost: x\r\nRange: bytes=900-\r\n\r\n",
    );
    assert!(head.starts_with("HTTP/1.1 206"), "{head}");
    assert_eq!(got, body[900..]);

    let _ = std::fs::remove_file(path);
}

#[test]
fn a_head_request_answers_without_the_file() {
    let path = temp_media("unflick-cast-head.mkv", &vec![7u8; 4096]);
    let server = MediaServer::start(path.clone()).expect("start");

    let (head, body) = request(server.port(), "HEAD /x HTTP/1.1\r\nHost: x\r\n\r\n");
    assert!(head.starts_with("HTTP/1.1 200 OK"), "{head}");
    assert!(head.contains("Content-Length: 4096"), "{head}");
    assert!(head.contains("Content-Type: video/x-matroska"), "{head}");
    assert!(body.is_empty(), "HEAD must not send the file");

    let _ = std::fs::remove_file(path);
}

#[test]
fn a_range_past_the_end_is_refused_with_the_status_that_says_so() {
    let path = temp_media("unflick-cast-416.mp4", &vec![0u8; 100]);
    let server = MediaServer::start(path.clone()).expect("start");

    let (head, _) = request(
        server.port(),
        "GET /x HTTP/1.1\r\nHost: x\r\nRange: bytes=500-600\r\n\r\n",
    );
    assert!(head.starts_with("HTTP/1.1 416"), "{head}");
    assert!(head.contains("Content-Range: bytes */100"), "{head}");

    let _ = std::fs::remove_file(path);
}

#[test]
fn the_url_handed_to_a_renderer_is_reachable_and_names_the_file() {
    let path = temp_media("unflick cast spaces.mp4", &vec![1u8; 16]);
    let server = MediaServer::start(path.clone()).expect("start");

    let url = server
        .url_for("127.0.0.1".parse().unwrap())
        .expect("a route to loopback");
    // Spaces in a filename must not produce a URL a renderer will choke on.
    assert!(!url.contains(' '), "{url}");
    assert!(url.contains(&server.port().to_string()), "{url}");
    assert!(url.starts_with("http://127.0.0.1:"), "{url}");

    let _ = std::fs::remove_file(path);
}

#[test]
fn stopping_the_server_stops_serving() {
    // A cast that has been stopped must not leave this machine handing out
    // someone's film to anything that asks.
    let path = temp_media("unflick-cast-stop.mp4", &vec![0u8; 64]);
    let server = MediaServer::start(path.clone()).expect("start");
    let port = server.port();
    server.stop();
    drop(server);
    std::thread::sleep(Duration::from_millis(300));

    // Checked by taking the port back rather than by connecting to it.
    // Connecting proves nothing once the port is free: the operating system
    // hands ephemeral ports out again quickly, so another test's server can
    // already be answering on it, and the test would be reading someone
    // else's socket.
    assert!(
        TcpListener::bind(("127.0.0.1", port)).is_ok(),
        "the port was never released, so the server is still listening"
    );

    let _ = std::fs::remove_file(path);
}

// ─── A stand-in television ────────────────────────────────────────────────

/// What a renderer was asked to do.
#[derive(Debug)]
struct SoapCall {
    action: String,
    body: String,
}

/// An AVTransport service that records its calls.
///
/// Enough of a renderer to hold up the conversation: it serves a device
/// description, answers SOAP, and reports what it was told. Everything
/// unflick sends a television goes through here.
struct FakeRenderer {
    port: u16,
    calls: Receiver<SoapCall>,
}

impl FakeRenderer {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().unwrap().port();
        let (tx, rx): (Sender<SoapCall>, Receiver<SoapCall>) = channel();

        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let tx = tx.clone();
                std::thread::spawn(move || {
                    let _ = serve_one(stream, port, tx);
                });
            }
        });

        Self { port, calls: rx }
    }

    fn description_url(&self) -> String {
        format!("http://127.0.0.1:{}/desc.xml", self.port)
    }

    fn control_url(&self) -> String {
        format!("http://127.0.0.1:{}/ctl", self.port)
    }

    fn next_call(&self) -> SoapCall {
        self.calls
            .recv_timeout(Duration::from_secs(5))
            .expect("the renderer should have been called")
    }
}

fn serve_one(mut stream: TcpStream, port: u16, tx: Sender<SoapCall>) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let path = line.split_whitespace().nth(1).unwrap_or("/").to_string();

    let mut length = 0usize;
    let mut action = String::new();
    loop {
        let mut h = String::new();
        if reader.read_line(&mut h)? == 0 {
            break;
        }
        let h = h.trim_end();
        if h.is_empty() {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            match k.trim().to_ascii_lowercase().as_str() {
                "content-length" => length = v.trim().parse().unwrap_or(0),
                "soapaction" => {
                    // `"urn:…:AVTransport:1#Play"` → `Play`
                    action = v
                        .trim()
                        .trim_matches('"')
                        .rsplit('#')
                        .next()
                        .unwrap_or("")
                        .to_string()
                }
                _ => {}
            }
        }
    }

    let mut body = vec![0u8; length];
    if length > 0 {
        reader.read_exact(&mut body)?;
    }
    let body = String::from_utf8_lossy(&body).into_owned();

    let reply = if path.contains("desc.xml") {
        format!(
            r#"<?xml version="1.0"?><root><device>
<friendlyName>Test Television</friendlyName>
<UDN>uuid:test-renderer-0001</UDN>
<serviceList>
<service><serviceType>urn:schemas-upnp-org:service:ConnectionManager:1</serviceType><controlURL>/cm</controlURL></service>
<service><serviceType>urn:schemas-upnp-org:service:AVTransport:1</serviceType><controlURL>http://127.0.0.1:{port}/ctl</controlURL></service>
</serviceList></device></root>"#
        )
    } else {
        let inner = match action.as_str() {
            "GetPositionInfo" => {
                "<TrackDuration>0:45:00</TrackDuration><RelTime>0:01:33</RelTime>".to_string()
            }
            "GetTransportInfo" => {
                "<CurrentTransportState>PLAYING</CurrentTransportState>".to_string()
            }
            _ => String::new(),
        };
        let _ = tx.send(SoapCall { action: action.clone(), body: body.clone() });
        format!(
            r#"<?xml version="1.0"?><s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/"><s:Body><u:{action}Response xmlns:u="urn:schemas-upnp-org:service:AVTransport:1">{inner}</u:{action}Response></s:Body></s:Envelope>"#
        )
    };

    let head = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/xml; charset=\"utf-8\"\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        reply.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(reply.as_bytes())?;
    Ok(())
}

#[test]
fn a_renderer_describes_itself_into_something_castable() {
    let tv = FakeRenderer::start();

    // The same path discovery takes once SSDP has produced a LOCATION.
    let body = ureq::get(&tv.description_url())
        .call()
        .expect("description")
        .into_string()
        .unwrap();

    assert_eq!(dlna::tag(&body, "friendlyName").as_deref(), Some("Test Television"));
    // ConnectionManager is listed first and is the wrong one.
    assert_eq!(
        dlna::control_url_for(&body, "urn:schemas-upnp-org:service:AVTransport:1").as_deref(),
        Some(format!("http://127.0.0.1:{}/ctl", tv.port).as_str())
    );
}

#[test]
fn handing_over_a_file_sends_the_uri_and_the_metadata_a_television_needs() {
    let tv = FakeRenderer::start();
    let r = dlna::renderer_at("Test Television", &tv.control_url());

    dlna::set_uri(
        &r,
        "http://192.168.1.9:5555/Fish%20%26%20Chips.mkv",
        "Fish & Chips",
        "object.item.videoItem",
        "video/x-matroska",
    )
    .expect("set uri");

    let call = tv.next_call();
    assert_eq!(call.action, "SetAVTransportURI");
    assert!(call.body.contains("<InstanceID>0</InstanceID>"), "{}", call.body);

    // The URL arrives intact through two layers of XML escaping — the
    // envelope's, and the DIDL-Lite's inside it.
    assert!(
        call.body.contains("Fish%20%26%20Chips.mkv"),
        "the media URL did not survive escaping: {}",
        call.body
    );
    // Metadata is not optional in practice; without it plenty of
    // televisions show "unknown" or refuse the item outright.
    assert!(call.body.contains("DIDL-Lite"), "no metadata sent: {}", call.body);
    assert!(call.body.contains("object.item.videoItem"), "{}", call.body);
    assert!(call.body.contains("video/x-matroska"), "{}", call.body);
    // A title with an ampersand must not break the document.
    assert!(call.body.contains("Fish &amp;amp; Chips"), "{}", call.body);
}

#[test]
fn transport_commands_reach_the_renderer_in_its_own_vocabulary() {
    let tv = FakeRenderer::start();
    let r = dlna::renderer_at("Test Television", &tv.control_url());

    dlna::play(&r).expect("play");
    let call = tv.next_call();
    assert_eq!(call.action, "Play");
    // AVTransport requires a Speed, and renderers reject the call without it.
    assert!(call.body.contains("<Speed>1</Speed>"), "{}", call.body);

    dlna::pause(&r).expect("pause");
    assert_eq!(tv.next_call().action, "Pause");

    dlna::stop(&r).expect("stop");
    assert_eq!(tv.next_call().action, "Stop");

    dlna::seek(&r, 93.7).expect("seek");
    let call = tv.next_call();
    assert_eq!(call.action, "Seek");
    // Seconds are not a time format AVTransport accepts.
    assert!(call.body.contains("<Unit>REL_TIME</Unit>"), "{}", call.body);
    assert!(call.body.contains("<Target>0:01:33</Target>"), "{}", call.body);
}

#[test]
fn the_renderers_position_is_read_back_as_numbers() {
    let tv = FakeRenderer::start();
    let r = dlna::renderer_at("Test Television", &tv.control_url());

    let p = dlna::position(&r).expect("position");
    assert_eq!(p.position, 93.0);
    assert_eq!(p.duration, 2700.0);
    assert_eq!(p.state, "PLAYING");
}

#[test]
fn a_renderer_that_refuses_says_why() {
    // A television declines with a SOAP fault carrying a description, and
    // "701 transition not available" is a different problem from a refused
    // connection. Losing that text leaves a user with nothing to act on.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            // Read the request out before answering. Replying and closing
            // on a socket with an unread body gets the client a connection
            // reset instead of the fault — which is what this test is for.
            if let Ok(clone) = stream.try_clone() {
                let mut reader = BufReader::new(clone);
                let mut length = 0usize;
                let mut line = String::new();
                let _ = reader.read_line(&mut line);
                loop {
                    let mut h = String::new();
                    if reader.read_line(&mut h).unwrap_or(0) == 0 {
                        break;
                    }
                    let h = h.trim_end();
                    if h.is_empty() {
                        break;
                    }
                    if let Some((k, v)) = h.split_once(':') {
                        if k.trim().eq_ignore_ascii_case("content-length") {
                            length = v.trim().parse().unwrap_or(0);
                        }
                    }
                }
                let mut body = vec![0u8; length];
                if length > 0 {
                    let _ = reader.read_exact(&mut body);
                }
            }
            let body = r#"<?xml version="1.0"?><s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/"><s:Body><s:Fault><faultstring>UPnPError</faultstring><detail><UPnPError><errorCode>701</errorCode><errorDescription>Transition not available</errorDescription></UPnPError></detail></s:Fault></s:Body></s:Envelope>"#;
            let head = format!(
                "HTTP/1.1 500 Internal Server Error\r\nContent-Type: text/xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(body.as_bytes());
        }
    });

    let r = dlna::renderer_at("Awkward TV", &format!("http://127.0.0.1:{port}/ctl"));
    let err = dlna::play(&r).expect_err("should have failed");
    let text = err.to_string();
    assert!(
        text.contains("Transition not available"),
        "the renderer's own reason should survive: {text}"
    );
    assert!(text.contains("Play"), "{text}");
}

// ─── The refusals, through the real binary ────────────────────────────────
//
// Every one of these returns before discovery runs, so they neither wait
// for the network nor risk reaching a television that happens to be on it.

#[test]
fn casting_nothing_says_so_rather_than_searching_the_network() {
    let d = Daemon::start();
    d.send("cast", json!({"action": "to"}))
        .expect_err_containing("nothing is playing");
}

#[test]
fn only_a_local_file_can_be_cast() {
    // The television fetches from this machine, so there has to be
    // something here to fetch. A stream is already a URL, but not one the
    // television can necessarily reach or decode — failing here beats
    // failing on the screen in the other room.
    let d = Daemon::start();
    d.send("cast", json!({"action": "to", "file": "https://example.com/a.mp4"}))
        .expect_err_containing("not a local file");
    d.send("cast", json!({"action": "to", "file": r"D:
ope\missing.mkv"}))
        .expect_err_containing("not a local file");
}

#[test]
fn driving_a_cast_that_is_not_running_is_an_error_not_a_silent_success() {
    let d = Daemon::start();
    for action in ["stop", "pause", "resume"] {
        d.send("cast", json!({"action": action}))
            .expect_err_containing("not casting");
    }
    // Status is the exception: "not casting" is a real answer to it.
    let reply = d.send("cast", json!({"action": "status"}));
    reply.expect_ok();
    assert!(reply.data().is_null());
}

#[test]
fn an_unknown_cast_action_is_named_in_the_error() {
    let d = Daemon::start();
    d.send("cast", json!({"action": "beam"}))
        .expect_err_containing("beam");
}
