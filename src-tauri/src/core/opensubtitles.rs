//! OpenSubtitles.com search and download.
//!
//! We can already *generate* subtitles with Whisper, which is the harder
//! problem — but for anything with a released subtitle, a human-made one is
//! better than a machine-made one, and it arrives in a second rather than a
//! minute. This closes that gap.
//!
//! ## The API key
//!
//! OpenSubtitles' REST API requires a per-application key, and download
//! quota is charged to whoever the key belongs to (5/day anonymous, 20/day
//! for a logged-in account). We deliberately do **not** ship a key of our
//! own: a single shared key would exhaust its quota within minutes of any
//! real usage, and every user would then see failures caused by strangers.
//! So the key is a setting the user fills in with their own, exactly as
//! IINA does.
//!
//! Absent a key, every entry point fails with a message naming the two
//! steps to fix it. That is the whole error-handling story — there is no
//! degraded mode, because there is nothing useful to degrade to.
//!
//! ## Matching
//!
//! Two ways to find subtitles for a file, and they are not equal:
//!
//!   * **moviehash** — a checksum of the file's first and last 64 KiB plus
//!     its size. A hit means someone uploaded a subtitle synced against
//!     *this exact release*, so the timing is right without adjustment.
//!   * **query** — a text search on the title. Finds subtitles for the
//!     movie, which may be cut for a different release and drift by seconds.
//!
//! We send both when we can and sort hash matches first, because the
//! difference between the two is precisely the difference between subtitles
//! that work and subtitles the user then has to nudge with `subtitle delay`.

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::http::{agent, url_encode};

const API_BASE: &str = "https://api.opensubtitles.com/api/v1";

/// Settings key holding the user's own API key.
pub const API_KEY_SETTING: &str = "opensubtitles_api_key";

/// Settings key holding the default language list (comma-separated).
pub const LANGUAGES_SETTING: &str = "opensubtitles_languages";

/// Bytes hashed from each end of the file. Fixed by the OSDb algorithm —
/// changing it changes the hash and matches nothing.
const HASH_CHUNK: usize = 64 * 1024;

/// The message shown whenever the key is missing. Spelled out in full
/// because "no API key" alone leaves the user with no idea what to do.
const NO_KEY_HELP: &str = "OpenSubtitles API key not set. \
Get a free key at https://www.opensubtitles.com/consumers, then run: \
unflick settings set opensubtitles_api_key <key>";

/// One search hit, flattened from the API's nested shape into what a user
/// actually chooses between.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SubtitleResult {
    /// The id passed to `download`. Note this is the *file* id, not the
    /// subtitle id — a subtitle entry can hold several files (multi-CD
    /// releases), and only the file id is downloadable.
    pub file_id: i64,
    pub language: String,
    /// Release the subtitle was synced against, e.g. "BluRay.x264-AMIABLE".
    pub release: String,
    pub file_name: String,
    pub downloads: i64,
    pub hearing_impaired: bool,
    pub from_trusted: bool,
    /// True when this subtitle is synced against the exact file we hashed.
    pub moviehash_match: bool,
    pub uploader: String,
    /// Human-facing page, for anyone who wants to check before downloading.
    pub url: String,
}

/// What to search for. All fields optional except that at least one of
/// `query` / `moviehash` must be present — the API rejects an empty search.
#[derive(Debug, Clone, Default)]
pub struct SearchOptions {
    pub query: Option<String>,
    pub moviehash: Option<String>,
    /// OpenSubtitles language codes: `en`, `zh-CN`, `pt-BR`, …
    pub languages: Vec<String>,
}

/// Result of a successful download, including the quota the API reports
/// back. We surface `remaining` because hitting a daily cap mid-session is
/// otherwise indistinguishable from the feature being broken.
#[derive(Debug, Clone, Serialize)]
pub struct Downloaded {
    pub path: String,
    pub file_name: String,
    /// Downloads used today, as counted by OpenSubtitles.
    pub requests: i64,
    /// Downloads left today.
    pub remaining: i64,
    /// When the quota resets (API-provided string, e.g. "23 hours").
    pub reset_time: String,
}

/// Read the user's API key from settings. `None` when unset or blank.
pub fn api_key() -> Option<String> {
    let v = super::settings::get(API_KEY_SETTING).ok().flatten()?;
    let s = v.as_str()?.trim();
    if s.is_empty() {
        return None;
    }
    Some(s.to_string())
}

fn require_key() -> Result<String> {
    api_key().ok_or_else(|| anyhow!(NO_KEY_HELP))
}

/// Whether a key is configured — lets the GUI show the setup prompt without
/// first firing a request that is guaranteed to fail.
pub fn is_configured() -> bool {
    api_key().is_some()
}

/// Default languages to search, from settings; falls back to English.
///
/// Stored as a comma-separated string rather than an array because it is
/// also typed by hand on the command line, and `zh-CN,en` is a friendlier
/// thing to type than a JSON list.
pub fn default_languages() -> Vec<String> {
    let raw = super::settings::get(LANGUAGES_SETTING)
        .ok()
        .flatten()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default();
    let parsed = split_languages(&raw);
    if parsed.is_empty() {
        vec!["en".to_string()]
    } else {
        parsed
    }
}

/// Split and normalise a comma-separated language list.
pub fn split_languages(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

/// Compute the OSDb "moviehash" for a file.
///
/// The algorithm is a 64-bit wrapping sum of the file size and every u64
/// (little-endian) in the first and last 64 KiB. It is not cryptographic
/// and not meant to be — it is cheap enough to run on a 40 GB remux and
/// unique enough to identify a release.
///
/// Files under 128 KiB have no distinct head and tail, so the algorithm
/// does not apply; we say so rather than returning a hash that matches
/// nothing.
pub fn compute_moviehash(path: &Path) -> Result<String> {
    let mut file = std::fs::File::open(path)
        .map_err(|e| anyhow!("cannot open {}: {}", path.display(), e))?;
    let size = file
        .metadata()
        .map_err(|e| anyhow!("cannot stat {}: {}", path.display(), e))?
        .len();

    if size < (HASH_CHUNK as u64) * 2 {
        bail!(
            "file is too small to hash ({} bytes; OpenSubtitles needs at least {})",
            size,
            HASH_CHUNK * 2
        );
    }

    let mut hash: u64 = size;
    let mut buf = vec![0u8; HASH_CHUNK];

    file.read_exact(&mut buf)
        .map_err(|e| anyhow!("read head of {}: {}", path.display(), e))?;
    hash = hash.wrapping_add(sum_u64_le(&buf));

    file.seek(SeekFrom::End(-(HASH_CHUNK as i64)))
        .map_err(|e| anyhow!("seek tail of {}: {}", path.display(), e))?;
    file.read_exact(&mut buf)
        .map_err(|e| anyhow!("read tail of {}: {}", path.display(), e))?;
    hash = hash.wrapping_add(sum_u64_le(&buf));

    Ok(format!("{:016x}", hash))
}

fn sum_u64_le(buf: &[u8]) -> u64 {
    buf.chunks_exact(8).fold(0u64, |acc, c| {
        acc.wrapping_add(u64::from_le_bytes(c.try_into().unwrap()))
    })
}

/// Turn a media path into a reasonable text query.
///
/// Release filenames are full of scene tags (`1080p`, `x265`, `WEB-DL`) and
/// separators that hurt a title search more than they help, so we cut the
/// name at the first such tag and normalise the separators.
/// `The.Matrix.1999.1080p.BluRay.x264-GRP.mkv` becomes `The Matrix 1999`.
pub fn query_from_filename(path: &str) -> String {
    let stem = Path::new(path)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();

    let spaced = stem.replace(['.', '_'], " ");
    let mut kept: Vec<&str> = Vec::new();
    for word in spaced.split_whitespace() {
        if is_release_tag(word) {
            break;
        }
        kept.push(word);
    }
    // A name made entirely of tags (rare, but `1080p.mkv` exists) would
    // leave nothing to search for; fall back to the whole stem.
    if kept.is_empty() {
        return spaced.split_whitespace().collect::<Vec<_>>().join(" ");
    }
    kept.join(" ")
}

/// Words that mark the start of the scene-tag suffix of a release name.
fn is_release_tag(word: &str) -> bool {
    let w = word.to_ascii_lowercase();
    const TAGS: &[&str] = &[
        "1080p", "2160p", "720p", "480p", "4k", "uhd", "hdr", "sdr", "bluray",
        "blu-ray", "brrip", "bdrip", "webrip", "web-dl", "webdl", "web", "hdtv",
        "dvdrip", "dvdscr", "remux", "x264", "x265", "h264", "h265", "hevc",
        "avc", "xvid", "divx", "aac", "ac3", "dts", "truehd", "atmos", "ddp5",
        "dd5", "flac", "10bit", "8bit", "proper", "repack", "extended",
        "unrated", "internal", "limited", "multi", "dual",
    ];
    TAGS.contains(&w.as_str())
}

/// Search OpenSubtitles.
///
/// Returns hits sorted by usefulness: exact-file (moviehash) matches first,
/// then by download count. Download count is a crude proxy for quality, but
/// it is the only signal the API gives that correlates with "this one is
/// not broken".
pub fn search(opts: &SearchOptions) -> Result<Vec<SubtitleResult>> {
    let key = require_key()?;
    if opts.query.is_none() && opts.moviehash.is_none() {
        bail!("nothing to search for: provide a query, or a file to hash");
    }

    let mut params: Vec<String> = Vec::new();
    if let Some(q) = &opts.query {
        let q = q.trim();
        if !q.is_empty() {
            params.push(format!("query={}", url_encode(q)));
        }
    }
    if let Some(h) = &opts.moviehash {
        params.push(format!("moviehash={}", url_encode(h)));
    }
    if !opts.languages.is_empty() {
        params.push(format!(
            "languages={}",
            url_encode(&opts.languages.join(","))
        ));
    }
    let url = format!("{}/subtitles?{}", API_BASE, params.join("&"));

    let body = get_json(&url, &key)?;
    let data = body
        .get("data")
        .and_then(|d| d.as_array())
        .ok_or_else(|| anyhow!("unexpected response from OpenSubtitles: no data array"))?;

    let mut out: Vec<SubtitleResult> = data.iter().filter_map(parse_hit).collect();
    out.sort_by(|a, b| {
        b.moviehash_match
            .cmp(&a.moviehash_match)
            .then(b.downloads.cmp(&a.downloads))
    });
    Ok(out)
}

/// Flatten one `data[]` entry. Returns `None` for entries with no
/// downloadable file — an entry can legitimately carry an empty `files`
/// array, and offering the user a row they cannot download is worse than
/// hiding it.
fn parse_hit(item: &Value) -> Option<SubtitleResult> {
    let attrs = item.get("attributes")?;
    let file = attrs.get("files").and_then(|f| f.as_array())?.first()?;
    let file_id = file.get("file_id").and_then(|v| v.as_i64())?;

    Some(SubtitleResult {
        file_id,
        language: str_field(attrs, "language").unwrap_or_else(|| "?".into()),
        release: str_field(attrs, "release").unwrap_or_default(),
        file_name: str_field(file, "file_name").unwrap_or_default(),
        downloads: attrs
            .get("download_count")
            .and_then(|v| v.as_i64())
            .unwrap_or(0),
        hearing_impaired: bool_field(attrs, "hearing_impaired"),
        from_trusted: bool_field(attrs, "from_trusted"),
        moviehash_match: bool_field(attrs, "moviehash_match"),
        uploader: attrs
            .get("uploader")
            .and_then(|u| u.get("name"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        url: str_field(attrs, "url").unwrap_or_default(),
    })
}

fn str_field(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(|x| x.as_str()).map(str::to_string)
}

/// Read a boolean that the API sometimes sends as `true`, sometimes as `1`,
/// and sometimes omits. Treating a numeric `1` as `false` would silently
/// mislabel hash matches, which is the one field we sort on.
fn bool_field(v: &Value, key: &str) -> bool {
    match v.get(key) {
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_i64().unwrap_or(0) != 0,
        Some(Value::String(s)) => s == "1" || s.eq_ignore_ascii_case("true"),
        _ => false,
    }
}

/// Download one subtitle file into `out_dir`.
///
/// Two round trips by design: `POST /download` charges the quota and hands
/// back a short-lived CDN link, which we then fetch. The quota counters in
/// the first response are the authoritative ones, so we carry them out even
/// though the file itself comes from the second.
pub fn download(file_id: i64, out_dir: &Path, rename_to: Option<&str>) -> Result<Downloaded> {
    let key = require_key()?;
    std::fs::create_dir_all(out_dir)
        .map_err(|e| anyhow!("cannot create {}: {}", out_dir.display(), e))?;

    let body = post_json(
        &format!("{}/download", API_BASE),
        &key,
        &serde_json::json!({ "file_id": file_id }),
    )?;

    let link = body
        .get("link")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            // A missing link with a message is how the API reports quota
            // exhaustion, so pass its own words through rather than
            // inventing our own.
            let msg = body
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("no download link in response");
            anyhow!("OpenSubtitles refused the download: {}", msg)
        })?
        .to_string();

    let api_name = body
        .get("file_name")
        .and_then(|v| v.as_str())
        .unwrap_or("subtitle.srt")
        .to_string();

    let file_name = match rename_to {
        Some(n) if !n.trim().is_empty() => sanitize_filename(n.trim()),
        _ => sanitize_filename(&api_name),
    };
    let dest: PathBuf = out_dir.join(&file_name);

    let mut content = Vec::new();
    agent(5, 60)
        .get(&link)
        .call()
        .map_err(|e| anyhow!("subtitle download failed: {}", e))?
        .into_reader()
        .read_to_end(&mut content)
        .map_err(|e| anyhow!("reading subtitle body: {}", e))?;

    std::fs::write(&dest, &content)
        .map_err(|e| anyhow!("cannot write {}: {}", dest.display(), e))?;

    Ok(Downloaded {
        path: dest.to_string_lossy().into_owned(),
        file_name,
        requests: body.get("requests").and_then(|v| v.as_i64()).unwrap_or(0),
        remaining: body.get("remaining").and_then(|v| v.as_i64()).unwrap_or(0),
        reset_time: body
            .get("reset_time")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
    })
}

/// Strip path separators and characters Windows rejects from a name the
/// server chose. The API returns uploader-supplied filenames; nothing stops
/// one containing `..\` and we write it straight to disk.
pub fn sanitize_filename(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\0' => '_',
            c if (c as u32) < 0x20 => '_',
            c => c,
        })
        .collect();
    let trimmed = cleaned.trim_matches(['.', ' ']).to_string();
    if trimmed.is_empty() {
        "subtitle.srt".to_string()
    } else {
        trimmed
    }
}

/// Suggested sidecar name for a downloaded subtitle: the video's stem plus
/// the language, so several languages can sit beside one video and mpv
/// picks them all up as external tracks.
pub fn sidecar_name(video_path: &str, language: &str, source_name: &str) -> String {
    let stem = Path::new(video_path)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "subtitle".into());
    let ext = Path::new(source_name)
        .extension()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "srt".into());
    let lang = language.trim();
    if lang.is_empty() || lang == "?" {
        sanitize_filename(&format!("{}.{}", stem, ext))
    } else {
        sanitize_filename(&format!("{}.{}.{}", stem, lang, ext))
    }
}

// ---------------------------------------------------------------------------
// Orchestration
//
// The three operations a user performs, assembled from the primitives above.
// They live here rather than in the control server because the GUI calls
// them directly - it holds its own Player and never goes through the socket -
// and a second copy of "derive the query, hash the file, pick a directory"
// is exactly how the two would drift apart.
//
// Nothing here touches a Player. The caller supplies the playing file and
// loads the result, which is all the player involvement there is.
// ---------------------------------------------------------------------------

/// A search as the user expressed it, before defaults are applied.
#[derive(Debug, Clone, Default)]
pub struct SearchRequest {
    /// Explicit search text. When absent, derived from `file`.
    pub query: Option<String>,
    /// The video to search for. Callers pass the playing file when the user
    /// did not name one.
    pub file: Option<String>,
    /// Raw comma-separated language list. When absent, falls back to the
    /// `opensubtitles_languages` setting, then English.
    pub languages: Option<String>,
    /// Whether to match by file hash as well as by title.
    pub hash: bool,
}

impl SearchRequest {
    /// A request that hashes, which is what every caller wants unless the
    /// user explicitly opted out. `Default` can't express this because
    /// `bool::default()` is false.
    pub fn for_file(file: Option<String>) -> Self {
        Self {
            file,
            hash: true,
            ..Default::default()
        }
    }
}

/// A completed search, including what it actually searched for. Callers echo
/// these back so a user staring at unexpected results can see the derived
/// query rather than guessing at it.
#[derive(Debug, Clone, Serialize)]
pub struct SearchOutcome {
    pub results: Vec<SubtitleResult>,
    pub query: Option<String>,
    pub moviehash: Option<String>,
    pub moviehash_matches: usize,
    pub languages: Vec<String>,
    pub file: Option<String>,
    /// Why hashing was skipped, when it was attempted and failed. Lets a
    /// caller distinguish "no exact match exists" from "we never looked".
    pub hash_error: Option<String>,
}

/// Apply defaults to a request and run the search.
pub fn run_search(req: &SearchRequest) -> Result<SearchOutcome> {
    let languages = match req.languages.as_deref() {
        Some(raw) => {
            let parsed = split_languages(raw);
            if parsed.is_empty() {
                bail!("languages list is empty");
            }
            parsed
        }
        None => default_languages(),
    };

    let query = req
        .query
        .as_deref()
        .map(str::trim)
        .filter(|q| !q.is_empty())
        .map(String::from)
        .or_else(|| {
            req.file
                .as_deref()
                .map(query_from_filename)
                .filter(|q| !q.is_empty())
        });

    // Hashing needs a real local file. A stream URL or a missing path simply
    // means no hash - not an error, since the text query still works.
    let mut hash_error: Option<String> = None;
    let moviehash = match req.file.as_deref() {
        Some(f) if req.hash && Path::new(f).is_file() => {
            match compute_moviehash(Path::new(f)) {
                Ok(h) => Some(h),
                Err(e) => {
                    hash_error = Some(e.to_string());
                    None
                }
            }
        }
        _ => None,
    };

    if query.is_none() && moviehash.is_none() {
        bail!("nothing to search for: play a file, or pass a query");
    }

    let results = search(&SearchOptions {
        query: query.clone(),
        moviehash: moviehash.clone(),
        languages: languages.clone(),
    })?;
    let moviehash_matches = results.iter().filter(|r| r.moviehash_match).count();

    Ok(SearchOutcome {
        results,
        query,
        moviehash,
        moviehash_matches,
        languages,
        file: req.file.clone(),
        hash_error,
    })
}

/// Download one subtitle, choosing where to put it.
///
/// Saved beside the video when that directory is writable: mpv auto-loads
/// sidecar subtitles, so the download survives into the next session without
/// anyone having to load it again. Streams and read-only directories fall
/// back to `fallback_dir`.
pub fn run_download(
    file_id: i64,
    video: Option<&str>,
    language: &str,
    name: Option<&str>,
    fallback_dir: &Path,
) -> Result<Downloaded> {
    let (out_dir, rename) = match video.and_then(sidecar_target) {
        Some(dir) => {
            let n = name
                .map(String::from)
                .unwrap_or_else(|| sidecar_name(video.unwrap_or(""), language, "subtitle.srt"));
            (dir, Some(n))
        }
        None => (fallback_dir.to_path_buf(), name.map(String::from)),
    };
    download(file_id, &out_dir, rename.as_deref())
}

/// Search, then download the best hit - the one-step form.
///
/// "Best" is whatever `run_search` sorted to the top: an exact-file match if
/// one exists, otherwise the most-downloaded subtitle in the requested
/// language. Downloads cost daily quota, so this takes exactly one.
///
/// Returns the chosen candidate alongside the download so the caller can
/// report *what* it got - "the subtitle for this exact file" and "a subtitle
/// for something with this title" are very different outcomes.
pub fn run_auto(
    req: &SearchRequest,
    fallback_dir: &Path,
) -> Result<(Downloaded, SubtitleResult, SearchOutcome)> {
    let outcome = run_search(req)?;
    let best = outcome.results.first().cloned().ok_or_else(|| {
        anyhow!(
            "no subtitles found for \"{}\" in {}",
            outcome.query.as_deref().unwrap_or(""),
            outcome.languages.join(", ")
        )
    })?;

    let dl = run_download(
        best.file_id,
        outcome.file.as_deref(),
        &best.language,
        None,
        fallback_dir,
    )?;
    Ok((dl, best, outcome))
}

/// Directory to drop a sidecar subtitle into, if the video lives in a
/// writable local directory. `None` for streams, missing files, and
/// read-only locations (optical media, a mounted share) - the caller then
/// falls back to a cache directory rather than failing the download.
fn sidecar_target(video: &str) -> Option<PathBuf> {
    let path = Path::new(video);
    if !path.is_file() {
        return None;
    }
    let dir = path.parent()?;
    // Probing beats checking permission bits: a directory can be writable
    // per its metadata and still reject writes (read-only mount, a network
    // share, Windows ACLs), and a failed download is a wasted quota unit.
    let probe = dir.join(".unflick-write-probe");
    match std::fs::write(&probe, b"") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            Some(dir.to_path_buf())
        }
        Err(_) => None,
    }
}

// ─── transport ────────────────────────────────────────────────────────────

/// The API wants the key in a header on every call, plus a User-Agent it
/// can attribute traffic to — requests without one are rejected.
fn user_agent() -> String {
    format!("unflick v{}", env!("CARGO_PKG_VERSION"))
}

fn get_json(url: &str, key: &str) -> Result<Value> {
    let resp = agent(5, 20)
        .get(url)
        .set("Api-Key", key)
        .set("User-Agent", &user_agent())
        .set("Accept", "application/json")
        .call();
    read_json(resp)
}

fn post_json(url: &str, key: &str, payload: &Value) -> Result<Value> {
    let resp = agent(5, 30)
        .post(url)
        .set("Api-Key", key)
        .set("User-Agent", &user_agent())
        .set("Accept", "application/json")
        .set("Content-Type", "application/json")
        .send_string(&payload.to_string());
    read_json(resp)
}

/// Turn a ureq result into JSON, translating the status codes that mean
/// something specific here into messages the user can act on.
fn read_json(resp: std::result::Result<ureq::Response, ureq::Error>) -> Result<Value> {
    match resp {
        Ok(r) => {
            let body = r.into_string().map_err(|e| anyhow!("read body: {}", e))?;
            serde_json::from_str(&body)
                .map_err(|e| anyhow!("OpenSubtitles returned invalid JSON: {}", e))
        }
        // Verified against the live API: an invalid key comes back as 403,
        // not the 401 the docs imply. Both get the same advice, because
        // "the key is wrong" is overwhelmingly the reason for either.
        Err(ureq::Error::Status(code @ (401 | 403), _)) => Err(anyhow!(
            "OpenSubtitles rejected the API key ({}). Check it with `unflick settings get --key {}`, or get one at https://www.opensubtitles.com/consumers",
            code,
            API_KEY_SETTING
        )),
        Err(ureq::Error::Status(406, r)) => Err(anyhow!(
            "OpenSubtitles download quota exhausted (406): {}",
            api_message(r)
        )),
        Err(ureq::Error::Status(429, r)) => Err(anyhow!(
            "OpenSubtitles rate limit hit (429) — wait a moment and retry: {}",
            api_message(r)
        )),
        Err(ureq::Error::Status(code, r)) => {
            Err(anyhow!("OpenSubtitles http {}: {}", code, api_message(r)))
        }
        Err(e) => Err(anyhow!("OpenSubtitles request failed: {}", e)),
    }
}

/// Pull the API's own error text out of a failing response body.
fn api_message(r: ureq::Response) -> String {
    let body = r.into_string().unwrap_or_default();
    serde_json::from_str::<Value>(&body)
        .ok()
        .and_then(|v| {
            v.get("message")
                .or_else(|| v.get("errors"))
                .map(|m| m.to_string())
        })
        .unwrap_or(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join("unflick-os-hash-test");
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    #[test]
    fn hash_sums_size_and_both_ends() {
        // Hand-built file: 128 KiB of zeros means the chunk sums contribute
        // nothing, so the hash must be exactly the file size.
        let path = scratch("zeros.bin");
        std::fs::write(&path, vec![0u8; HASH_CHUNK * 2]).unwrap();

        let h = compute_moviehash(&path).unwrap();
        assert_eq!(h, format!("{:016x}", HASH_CHUNK * 2));
        assert_eq!(h.len(), 16, "OSDb hashes are 16 hex chars, zero-padded");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn hash_rejects_small_files() {
        let path = scratch("tiny.bin");
        std::fs::write(&path, b"not a movie").unwrap();

        let err = compute_moviehash(&path).unwrap_err().to_string();
        assert!(err.contains("too small"), "got: {}", err);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn hash_is_sensitive_to_content() {
        let a = scratch("a.bin");
        let b = scratch("b.bin");
        let mut data = vec![0u8; HASH_CHUNK * 2];
        std::fs::write(&a, &data).unwrap();
        data[0] = 1;
        std::fs::write(&b, &data).unwrap();

        assert_ne!(
            compute_moviehash(&a).unwrap(),
            compute_moviehash(&b).unwrap()
        );
        std::fs::remove_file(&a).ok();
        std::fs::remove_file(&b).ok();
    }

    #[test]
    fn query_strips_scene_tags() {
        assert_eq!(
            query_from_filename("The.Matrix.1999.1080p.BluRay.x264-GRP.mkv"),
            "The Matrix 1999"
        );
        assert_eq!(
            query_from_filename("/movies/Arrival 2016 2160p HDR.mkv"),
            "Arrival 2016"
        );
    }

    #[test]
    fn query_keeps_plain_names() {
        assert_eq!(query_from_filename("holiday video.mp4"), "holiday video");
        assert_eq!(query_from_filename("Show.S01E02.mkv"), "Show S01E02");
    }

    #[test]
    fn query_survives_an_all_tag_name() {
        // Nothing but tags: falling back to the stem beats an empty query,
        // which the API rejects outright.
        assert_eq!(query_from_filename("1080p.x264.mkv"), "1080p x264");
    }

    #[test]
    fn sanitize_blocks_traversal_and_reserved_chars() {
        assert_eq!(sanitize_filename("../../evil.srt"), "_.._evil.srt");
        assert_eq!(sanitize_filename("a:b*c?.srt"), "a_b_c_.srt");
        assert_eq!(sanitize_filename("   "), "subtitle.srt");
        assert_eq!(sanitize_filename("..."), "subtitle.srt");
    }

    #[test]
    fn sidecar_pairs_language_with_video_stem() {
        assert_eq!(
            sidecar_name("D:/m/The Matrix.mkv", "zh-CN", "whatever.srt"),
            "The Matrix.zh-CN.srt"
        );
        // Unknown language: don't invent one.
        assert_eq!(sidecar_name("/m/clip.mp4", "?", "x.ass"), "clip.ass");
    }

    #[test]
    fn languages_split_and_trim() {
        assert_eq!(split_languages(" zh-CN , en ,, "), vec!["zh-CN", "en"]);
        assert!(split_languages("  ").is_empty());
    }

    #[test]
    fn bool_field_accepts_the_three_shapes_the_api_uses() {
        let v = serde_json::json!({"a": true, "b": 1, "c": "1", "d": 0, "e": false});
        assert!(bool_field(&v, "a"));
        assert!(bool_field(&v, "b"));
        assert!(bool_field(&v, "c"));
        assert!(!bool_field(&v, "d"));
        assert!(!bool_field(&v, "e"));
        assert!(!bool_field(&v, "missing"));
    }

    #[test]
    fn parse_hit_flattens_and_skips_fileless_entries() {
        let ok = serde_json::json!({
            "attributes": {
                "language": "en",
                "release": "BluRay.x264",
                "download_count": 4200,
                "moviehash_match": true,
                "from_trusted": 1,
                "uploader": {"name": "someone"},
                "url": "https://example/sub",
                "files": [{"file_id": 77, "file_name": "matrix.srt"}]
            }
        });
        let hit = parse_hit(&ok).unwrap();
        assert_eq!(hit.file_id, 77);
        assert_eq!(hit.language, "en");
        assert_eq!(hit.downloads, 4200);
        assert!(hit.moviehash_match);
        assert!(hit.from_trusted);
        assert_eq!(hit.uploader, "someone");

        let fileless = serde_json::json!({"attributes": {"language": "en", "files": []}});
        assert!(parse_hit(&fileless).is_none());
    }

    #[test]
    fn sidecar_target_prefers_the_videos_own_directory() {
        let video = scratch("movie.mkv");
        std::fs::write(&video, b"x").unwrap();
        let dir = sidecar_target(video.to_str().unwrap()).unwrap();
        assert_eq!(dir, video.parent().unwrap());
        // No probe file left behind.
        assert!(!dir.join(".unflick-write-probe").exists());
        std::fs::remove_file(&video).ok();
    }

    #[test]
    fn sidecar_target_declines_streams_and_missing_files() {
        assert!(sidecar_target("https://example.com/a.mp4").is_none());
        assert!(sidecar_target("").is_none());
        let missing = scratch("definitely-not-here.mkv");
        std::fs::remove_file(&missing).ok();
        assert!(sidecar_target(missing.to_str().unwrap()).is_none());
    }

    #[test]
    fn run_search_rejects_an_empty_language_list() {
        let req = SearchRequest {
            query: Some("x".into()),
            languages: Some("  ,  ".into()),
            ..Default::default()
        };
        let err = run_search(&req).unwrap_err().to_string();
        assert!(err.contains("languages list is empty"), "got: {}", err);
    }

    #[test]
    fn run_search_needs_something_to_go_on() {
        // No query, no file: refused before any key check or network call.
        let err = run_search(&SearchRequest {
            languages: Some("en".into()),
            ..Default::default()
        })
        .unwrap_err()
        .to_string();
        assert!(err.contains("nothing to search for"), "got: {}", err);
    }

    #[test]
    fn for_file_hashes_by_default() {
        // The bug this guards: `SearchRequest::default()` has hash = false,
        // so a caller building one by hand silently loses exact matching.
        assert!(SearchRequest::for_file(Some("a.mkv".into())).hash);
        assert!(!SearchRequest::default().hash);
    }

    #[test]
    fn the_missing_key_message_names_both_steps() {
        // Deliberately asserted on the constant rather than by calling
        // `search` with settings redirected: CONFIG_DIR_ENV is process-wide
        // and the keybind tests in this same binary read settings too, so
        // mutating it here would race them.
        assert!(NO_KEY_HELP.contains("opensubtitles.com/consumers"));
        assert!(NO_KEY_HELP.contains("settings set opensubtitles_api_key"));
    }

    #[test]
    fn search_refuses_an_empty_query() {
        // Guard runs before the key check would matter for a real caller,
        // but the key check comes first, so assert on whichever fires —
        // both are refusals, and neither hits the network.
        let err = search(&SearchOptions::default()).unwrap_err().to_string();
        assert!(
            err.contains("nothing to search for") || err.contains("API key not set"),
            "got: {}",
            err
        );
    }
}
