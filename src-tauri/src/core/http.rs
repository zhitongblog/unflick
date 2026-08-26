//! Shared HTTP plumbing.
//!
//! Two modules now talk to third-party web APIs (SponsorBlock, OpenSubtitles)
//! and both need the same two things: percent-encoding for query strings and
//! a `ureq` agent with sane timeouts. Percent-encoding in particular is the
//! kind of code that silently diverges when copied — one copy learns that
//! `~` is unreserved and the other doesn't — so it lives here once.

use std::time::Duration;

/// Percent-encode a string for use in a URL query component.
///
/// Unreserved set per RFC 3986 §2.3; everything else is escaped, including
/// `/` and `:`. That is deliberately aggressive: every caller here puts the
/// result inside a query *value*, never a path.
pub fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                out.push_str(&format!("%{:02X}", b));
            }
        }
    }
    out
}

/// A `ureq` agent with connect/read timeouts.
///
/// Nothing we fetch over HTTP is worth stalling playback for, so every
/// caller gets a bounded wait rather than ureq's default of "forever".
pub fn agent(connect_secs: u64, total_secs: u64) -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(connect_secs))
        .timeout(Duration::from_secs(total_secs))
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leaves_unreserved_characters_alone() {
        assert_eq!(url_encode("abcXYZ019-_.~"), "abcXYZ019-_.~");
    }

    #[test]
    fn escapes_spaces_and_punctuation() {
        assert_eq!(url_encode("The Matrix (1999)"), "The%20Matrix%20%281999%29");
    }

    #[test]
    fn escapes_utf8_bytewise() {
        // Each byte of a multi-byte codepoint gets its own %XX.
        assert_eq!(url_encode("中"), "%E4%B8%AD");
    }

    #[test]
    fn escapes_json_payload_characters() {
        // SponsorBlock passes a JSON array through the query string.
        assert_eq!(url_encode("[\"sponsor\"]"), "%5B%22sponsor%22%5D");
    }
}
