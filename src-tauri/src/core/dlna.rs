//! Casting to a television over DLNA / UPnP.
//!
//! Three separate things have to happen, and it is worth being clear which
//! is which because they fail differently:
//!
//! 1. **Find the renderer.** SSDP: a UDP multicast question, and every
//!    television on the network that can play video answers with a URL to
//!    its own description.
//! 2. **Be reachable.** The renderer fetches the media itself, so this
//!    machine serves it over HTTP — see `mediaserver`.
//! 3. **Tell it what to do.** SOAP over HTTP to the renderer's AVTransport
//!    service: here is a URL, play it, pause, stop, where are you.
//!
//! Nothing here needs an XML parser. UPnP descriptions are small and the
//! handful of fields wanted are unambiguous, so they are pulled out by
//! name; a dependency to read four tags would cost more than it saves.

use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};

/// Where every UPnP device on the network is listening for questions.
const SSDP_ADDR: &str = "239.255.255.250:1900";
/// What we are looking for. Not `ssdp:all` — that answers with printers.
const RENDERER_TYPE: &str = "urn:schemas-upnp-org:device:MediaRenderer:1";
/// The service on that device that plays things.
const AVTRANSPORT: &str = "urn:schemas-upnp-org:service:AVTransport:1";

/// A television (or anything else) willing to play what we send it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Renderer {
    /// What it calls itself. What a person picks from a list.
    pub name: String,
    /// Stable across discoveries, so a cast can name a target that will
    /// still mean the same box next time.
    pub id: String,
    /// Absolute URL of the AVTransport control endpoint.
    pub control_url: String,
    /// The renderer's address, used to choose which of our own addresses
    /// it can reach.
    pub address: String,
}

impl Renderer {
    /// The renderer's IP, for working out how it can reach us.
    pub fn ip(&self) -> Option<IpAddr> {
        self.address
            .rsplit_once(':')
            .and_then(|(host, _)| host.trim_matches(['[', ']']).parse().ok())
            .or_else(|| self.address.parse().ok())
    }
}

// ─── Discovery ────────────────────────────────────────────────────────────

/// Ask the network what can play video, and describe what answers.
///
/// `timeout` is how long to listen. It is not latency to be minimised: SSDP
/// replies are deliberately spread over the `MX` window so that fifty
/// devices do not answer at once, so cutting it short simply loses the
/// slower televisions.
pub fn discover(timeout: Duration) -> Result<Vec<Renderer>> {
    let locations = search(timeout)?;
    let mut out = Vec::new();
    for location in locations {
        match describe(&location) {
            Ok(r) => {
                // The same device answers more than once — that is normal
                // SSDP, not a bug to work around anywhere but here.
                if !out.iter().any(|e: &Renderer| e.id == r.id) {
                    out.push(r);
                }
            }
            Err(e) => eprintln!("[unflick] dlna: {} did not describe itself: {e}", location),
        }
    }
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(out)
}

/// The SSDP half: returns the description URLs that answered.
fn search(timeout: Duration) -> Result<Vec<String>> {
    // MX must be under the timeout or the last replies arrive after we
    // have stopped listening.
    let mx = ((timeout.as_secs().max(2)) - 1).min(5);
    let request = format!(
        "M-SEARCH * HTTP/1.1\r\n\
         HOST: {SSDP_ADDR}\r\n\
         MAN: \"ssdp:discover\"\r\n\
         MX: {mx}\r\n\
         ST: {RENDERER_TYPE}\r\n\r\n"
    );

    let socket = UdpSocket::bind("0.0.0.0:0")?;
    socket.set_read_timeout(Some(Duration::from_millis(400)))?;
    // Multicast to a link-local group: the default TTL of 1 is correct and
    // deliberate — a television two routers away is not castable anyway.
    socket.send_to(request.as_bytes(), SSDP_ADDR)?;

    let deadline = Instant::now() + timeout;
    let mut locations = Vec::new();
    let mut buf = [0u8; 4096];
    while Instant::now() < deadline {
        match socket.recv_from(&mut buf) {
            Ok((n, _)) => {
                let text = String::from_utf8_lossy(&buf[..n]);
                if let Some(loc) = header(&text, "LOCATION") {
                    if !locations.contains(&loc) {
                        locations.push(loc);
                    }
                }
            }
            // A read timeout is the normal quiet between replies.
            Err(_) => continue,
        }
    }
    Ok(locations)
}

/// Fetch a device description and pull out what a cast needs.
fn describe(location: &str) -> Result<Renderer> {
    let body = super::http::agent(3, 6)
        .get(location)
        .call()
        .map_err(|e| anyhow!("{e}"))?
        .into_string()?;

    let name = tag(&body, "friendlyName").unwrap_or_else(|| "Unnamed renderer".into());
    let id = tag(&body, "UDN").unwrap_or_else(|| location.to_string());

    let control = control_url_for(&body, AVTRANSPORT)
        .ok_or_else(|| anyhow!("no AVTransport service"))?;
    let control_url = resolve(location, &control);
    let address = authority(location).unwrap_or_default();

    Ok(Renderer { name, id, control_url, address })
}

/// The `controlURL` of the service whose `serviceType` names `wanted`.
///
/// Done by walking `<service>` blocks rather than by one big regex: a
/// description lists several services, and picking the wrong one gets a
/// renderer that accepts `Play` and does nothing.
pub fn control_url_for(xml: &str, wanted: &str) -> Option<String> {
    let mut rest = xml;
    while let Some(start) = rest.find("<service>") {
        let after = &rest[start..];
        let end = after.find("</service>").map(|e| e + "</service>".len())?;
        let block = &after[..end];
        if let Some(kind) = tag(block, "serviceType") {
            // Match on the type without its version: renderers ship
            // AVTransport:1, :2 and :3, and they all speak the actions used
            // here.
            let base = |s: &str| s.rsplit_once(':').map(|(b, _)| b.to_string());
            if kind == wanted || base(&kind) == base(wanted) {
                return tag(block, "controlURL");
            }
        }
        rest = &after[end..];
    }
    None
}

/// The text of the first `<name>…</name>`, whatever namespace prefix it has.
pub fn tag(xml: &str, name: &str) -> Option<String> {
    let open = format!("<{}", name);
    let close = format!("</{}>", name);
    let start = xml.find(&open)?;
    // Skip the rest of the opening tag, attributes and all.
    let content_start = start + xml[start..].find('>')? + 1;
    let end = xml[content_start..].find(&close)? + content_start;
    Some(unescape(xml[content_start..end].trim()))
}

/// One header out of an SSDP or HTTP response, case-insensitively.
pub fn header(response: &str, name: &str) -> Option<String> {
    response.lines().find_map(|line| {
        let (k, v) = line.split_once(':')?;
        if k.trim().eq_ignore_ascii_case(name) {
            Some(v.trim().to_string())
        } else {
            None
        }
    })
}

/// Make a possibly-relative control URL absolute against the description's.
pub fn resolve(base: &str, url: &str) -> String {
    if url.starts_with("http://") || url.starts_with("https://") {
        return url.to_string();
    }
    let Some(auth) = authority(base) else {
        return url.to_string();
    };
    if let Some(path) = url.strip_prefix('/') {
        format!("http://{}/{}", auth, path)
    } else {
        // Relative to the description's directory, which is what the spec
        // says and what the awkward renderers rely on.
        let dir = base
            .rfind('/')
            .map(|i| &base[..i + 1])
            .unwrap_or(base);
        format!("{}{}", dir, url)
    }
}

/// `http://192.168.1.5:8200/desc.xml` → `192.168.1.5:8200`
fn authority(url: &str) -> Option<String> {
    let rest = url.split_once("://")?.1;
    Some(rest.split('/').next()?.to_string())
}

// ─── Control ──────────────────────────────────────────────────────────────

/// One SOAP call to an AVTransport service.
///
/// `body` is the inner XML of the action element; the envelope and the
/// headers are the same every time.
fn soap(control_url: &str, action: &str, body: &str) -> Result<String> {
    let envelope = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
<s:Body><u:{action} xmlns:u="{AVTRANSPORT}"><InstanceID>0</InstanceID>{body}</u:{action}></s:Body>
</s:Envelope>"#
    );

    let response = super::http::agent(3, 15)
        .post(control_url)
        .set("Content-Type", "text/xml; charset=\"utf-8\"")
        .set("SOAPAction", &format!("\"{}#{}\"", AVTRANSPORT, action))
        .send_string(&envelope);

    match response {
        Ok(r) => Ok(r.into_string()?),
        Err(ureq::Error::Status(code, r)) => {
            // A renderer that refuses says why in the fault, and that text
            // is far more use than the status on its own — "701 transition
            // not available" is a different problem from "not enough
            // bandwidth".
            let body = r.into_string().unwrap_or_default();
            let reason = tag(&body, "errorDescription")
                .or_else(|| tag(&body, "faultstring"))
                .unwrap_or_else(|| format!("HTTP {}", code));
            bail!("{} refused {}: {}", control_url, action, reason)
        }
        Err(e) => bail!("{} did not answer {}: {}", control_url, action, e),
    }
}

/// Hand a renderer a URL and start it.
pub fn set_uri(renderer: &Renderer, url: &str, title: &str, upnp_class: &str, mime: &str) -> Result<()> {
    // Metadata is nominally optional and practically is not: plenty of
    // televisions play a URL with no DIDL-Lite and show it as "unknown",
    // and some refuse it outright.
    let didl = format!(
        r#"<DIDL-Lite xmlns="urn:schemas-upnp-org:metadata-1-0/DIDL-Lite/" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:upnp="urn:schemas-upnp-org:metadata-1-0/upnp/"><item id="0" parentID="-1" restricted="1"><dc:title>{}</dc:title><upnp:class>{}</upnp:class><res protocolInfo="http-get:*:{}:DLNA.ORG_OP=01;DLNA.ORG_FLAGS=01700000000000000000000000000000">{}</res></item></DIDL-Lite>"#,
        escape(title),
        upnp_class,
        mime,
        escape(url)
    );
    soap(
        &renderer.control_url,
        "SetAVTransportURI",
        &format!(
            "<CurrentURI>{}</CurrentURI><CurrentURIMetaData>{}</CurrentURIMetaData>",
            escape(url),
            escape(&didl)
        ),
    )?;
    Ok(())
}

pub fn play(renderer: &Renderer) -> Result<()> {
    soap(&renderer.control_url, "Play", "<Speed>1</Speed>").map(|_| ())
}

pub fn pause(renderer: &Renderer) -> Result<()> {
    soap(&renderer.control_url, "Pause", "").map(|_| ())
}

pub fn stop(renderer: &Renderer) -> Result<()> {
    soap(&renderer.control_url, "Stop", "").map(|_| ())
}

/// Seek to a position in seconds.
pub fn seek(renderer: &Renderer, seconds: f64) -> Result<()> {
    soap(
        &renderer.control_url,
        "Seek",
        &format!(
            "<Unit>REL_TIME</Unit><Target>{}</Target>",
            hms(seconds.max(0.0))
        ),
    )
    .map(|_| ())
}

/// Where the renderer is, and how long the thing it is playing runs for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CastPosition {
    pub position: f64,
    pub duration: f64,
    /// `PLAYING`, `PAUSED_PLAYBACK`, `STOPPED`, … — the renderer's word.
    pub state: String,
}

pub fn position(renderer: &Renderer) -> Result<CastPosition> {
    let info = soap(&renderer.control_url, "GetPositionInfo", "")?;
    let transport = soap(&renderer.control_url, "GetTransportInfo", "")?;
    Ok(CastPosition {
        position: tag(&info, "RelTime").map(|s| seconds(&s)).unwrap_or(0.0),
        duration: tag(&info, "TrackDuration").map(|s| seconds(&s)).unwrap_or(0.0),
        state: tag(&transport, "CurrentTransportState").unwrap_or_else(|| "UNKNOWN".into()),
    })
}

// ─── Small conversions ────────────────────────────────────────────────────

/// `93.5` → `0:01:33`, the only time format AVTransport accepts.
pub fn hms(seconds: f64) -> String {
    let total = seconds.max(0.0) as u64;
    format!("{}:{:02}:{:02}", total / 3600, (total % 3600) / 60, total % 60)
}

/// `0:01:33` → `93.0`. Renderers also answer `NOT_IMPLEMENTED`, which is
/// zero rather than an error — it means "this stream has no duration",
/// which is true of a live one.
pub fn seconds(hms: &str) -> f64 {
    let mut total = 0.0;
    let mut any = false;
    for part in hms.trim().split(':') {
        let Ok(v) = part.trim().parse::<f64>() else {
            return 0.0;
        };
        total = total * 60.0 + v;
        any = true;
    }
    if any {
        total
    } else {
        0.0
    }
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        // Ampersand last, or `&amp;lt;` would come out as `<`.
        .replace("&amp;", "&")
}

/// Build a renderer by hand, for a caller that already knows the address —
/// and for tests, which stand up a fake AVTransport rather than a
/// television.
pub fn renderer_at(name: &str, control_url: &str) -> Renderer {
    Renderer {
        name: name.to_string(),
        id: control_url.to_string(),
        address: authority(control_url).unwrap_or_default(),
        control_url: control_url.to_string(),
    }
}

/// The address a renderer can be reached at, as a socket address.
pub fn peer_of(renderer: &Renderer) -> Option<SocketAddr> {
    renderer.address.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const DESCRIPTION: &str = r#"<?xml version="1.0"?>
<root xmlns="urn:schemas-upnp-org:device-1-0">
  <device>
    <friendlyName>Living Room TV</friendlyName>
    <UDN>uuid:4d696372-6f73-6f66-7420-0011322744aa</UDN>
    <serviceList>
      <service>
        <serviceType>urn:schemas-upnp-org:service:ConnectionManager:1</serviceType>
        <controlURL>/upnp/control/cm</controlURL>
      </service>
      <service>
        <serviceType>urn:schemas-upnp-org:service:AVTransport:1</serviceType>
        <controlURL>/upnp/control/avt</controlURL>
      </service>
    </serviceList>
  </device>
</root>"#;

    #[test]
    fn the_right_service_is_picked_out_of_the_description() {
        // ConnectionManager comes first and accepts nothing useful; picking
        // it would give a renderer that answers and never plays.
        assert_eq!(
            control_url_for(DESCRIPTION, AVTRANSPORT).as_deref(),
            Some("/upnp/control/avt")
        );
    }

    #[test]
    fn a_later_avtransport_version_is_still_avtransport() {
        let xml = DESCRIPTION.replace("AVTransport:1", "AVTransport:3");
        assert_eq!(
            control_url_for(&xml, AVTRANSPORT).as_deref(),
            Some("/upnp/control/avt")
        );
    }

    #[test]
    fn a_device_with_no_avtransport_is_not_a_renderer() {
        let xml = DESCRIPTION.replace("AVTransport:1", "RenderingControl:1");
        assert_eq!(control_url_for(&xml, AVTRANSPORT), None);
    }

    #[test]
    fn tags_survive_attributes_and_escaping() {
        assert_eq!(tag(DESCRIPTION, "friendlyName").as_deref(), Some("Living Room TV"));
        assert_eq!(
            tag("<dc:title>Fish &amp; Chips</dc:title>", "dc:title").as_deref(),
            Some("Fish & Chips")
        );
        assert_eq!(
            tag(r#"<res protocolInfo="x">http://a/b</res>"#, "res").as_deref(),
            Some("http://a/b")
        );
        assert_eq!(tag(DESCRIPTION, "nothingHere"), None);
    }

    #[test]
    fn control_urls_are_made_absolute_the_three_ways_they_come() {
        let base = "http://192.168.1.5:8200/rootDesc.xml";
        assert_eq!(resolve(base, "/ctl/AVT"), "http://192.168.1.5:8200/ctl/AVT");
        assert_eq!(resolve(base, "ctl/AVT"), "http://192.168.1.5:8200/ctl/AVT");
        assert_eq!(
            resolve(base, "http://10.0.0.9:80/AVT"),
            "http://10.0.0.9:80/AVT"
        );
    }

    #[test]
    fn the_location_header_is_found_however_it_is_cased() {
        let reply = "HTTP/1.1 200 OK\r\nCACHE-CONTROL: max-age=1800\r\n\
                     Location: http://192.168.1.5:8200/rootDesc.xml\r\n\
                     ST: urn:schemas-upnp-org:device:MediaRenderer:1\r\n\r\n";
        assert_eq!(
            header(reply, "LOCATION").as_deref(),
            Some("http://192.168.1.5:8200/rootDesc.xml")
        );
        assert_eq!(header(reply, "usn"), None);
    }

    #[test]
    fn times_round_trip_in_the_format_avtransport_uses() {
        assert_eq!(hms(0.0), "0:00:00");
        assert_eq!(hms(93.7), "0:01:33");
        assert_eq!(hms(3725.0), "1:02:05");
        assert_eq!(seconds("0:01:33"), 93.0);
        assert_eq!(seconds("1:02:05"), 3725.0);
        // A live stream has no duration and says so.
        assert_eq!(seconds("NOT_IMPLEMENTED"), 0.0);
        assert_eq!(seconds(""), 0.0);
    }

    #[test]
    fn a_renderers_address_is_read_off_its_control_url() {
        let r = renderer_at("TV", "http://192.168.1.5:8200/ctl");
        assert_eq!(r.address, "192.168.1.5:8200");
        assert_eq!(r.ip().map(|i| i.to_string()).as_deref(), Some("192.168.1.5"));
    }
}
