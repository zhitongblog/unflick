//! Recognising a DVD or Blu-ray, wherever it is being kept.
//!
//! mpv can play both — the bundled build lists `dvd`, `dvdnav`, `bd`,
//! `bluray` and `br` among its protocols — but only when it is told that is
//! what it is looking at. Handing it `D:\film.iso` as a file gets an ISO9660
//! image demuxed as if it were a media container, which fails in a way that
//! reads as "unflick cannot play this".
//!
//! What mpv wants instead is a protocol and a device: `dvd://` with
//! `dvd-device` pointing at the image, the folder, or the drive. Everything
//! here exists to work out which of those it is, from the path alone, before
//! anything is loaded.
//!
//! ## Why not just try it
//!
//! Loading `dvd://` against a Blu-ray fails slowly and visibly — a black
//! window, then an error. Trying one and falling back to the other doubles
//! that. The layout on the disc says which it is, so this reads it: a
//! `VIDEO_TS` directory is a DVD, `BDMV` is a Blu-ray, and inside an image
//! those directories are still there to be found.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The two kinds of video disc mpv can open for us.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DiscKind {
    Dvd,
    BluRay,
}

impl DiscKind {
    /// The mpv protocol that opens it.
    ///
    /// `dvd://` rather than `dvdnav://`: dvdnav starts in the disc's menu,
    /// which is the right answer for someone who wants the extras and the
    /// wrong one for someone who put a film on. Menus are reachable by
    /// asking for them (`unflick play dvdnav://`), which is the way round
    /// that matches "open this and play it".
    pub fn url(self) -> &'static str {
        match self {
            DiscKind::Dvd => "dvd://",
            DiscKind::BluRay => "bd://",
        }
    }

    /// The mpv option that says where the disc is.
    pub fn device_property(self) -> &'static str {
        match self {
            DiscKind::Dvd => "dvd-device",
            DiscKind::BluRay => "bluray-device",
        }
    }

    /// The directory that identifies this kind of disc.
    fn marker(self) -> &'static str {
        match self {
            DiscKind::Dvd => "VIDEO_TS",
            DiscKind::BluRay => "BDMV",
        }
    }

    /// How this kind is spelled inside an identity key.
    fn slug(self) -> &'static str {
        match self {
            DiscKind::Dvd => "dvd",
            DiscKind::BluRay => "bluray",
        }
    }
}

/// A disc unflick knows how to open, and where it lives.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Disc {
    pub kind: DiscKind,
    /// What to hand `loadfile`.
    pub url: String,
    /// What to set `dvd-device` / `bluray-device` to.
    pub device: String,
}

/// Protocols that are already a disc, whoever typed them.
const DISC_SCHEMES: &[(&str, DiscKind)] = &[
    ("dvd://", DiscKind::Dvd),
    ("dvdnav://", DiscKind::Dvd),
    ("bd://", DiscKind::BluRay),
    ("bluray://", DiscKind::BluRay),
    ("br://", DiscKind::BluRay),
];

/// Extensions worth opening and looking inside.
///
/// Deliberately short. Probing every file someone plays for a filesystem
/// header would be a read on the hot path for no gain — these are the names
/// disc images actually get.
const IMAGE_EXTENSIONS: &[&str] = &["iso", "img", "udf"];

/// Work out whether `path` is a disc, and how to open it.
///
/// `None` means "not a disc" — the ordinary case, and the one that must
/// stay cheap, so a plain media file costs one extension comparison and no
/// filesystem read beyond what the caller already did.
pub fn detect(path: &str) -> Option<Disc> {
    // Someone naming the protocol has already answered the question.
    let lower = path.to_ascii_lowercase();
    for (scheme, kind) in DISC_SCHEMES {
        if lower.starts_with(scheme) {
            // `dvd://2/D:\film.iso` — mpv's own syntax puts the device after
            // the title. Keep whatever they wrote; they are being explicit.
            return Some(Disc {
                kind: *kind,
                url: path.to_string(),
                device: String::new(),
            });
        }
    }

    let p = Path::new(path);

    // A folder, a mounted drive, or a mount point: look for the marker
    // directory. This also covers `D:\` on Windows and `/Volumes/FILM` on
    // macOS, which are directories as far as this is concerned.
    if p.is_dir() {
        return kind_of_directory(p).map(|kind| Disc {
            kind,
            url: kind.url().to_string(),
            device: path.to_string(),
        });
    }

    // An image file: the marker directory is inside it.
    let ext = p
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if IMAGE_EXTENSIONS.contains(&ext.as_str()) {
        return kind_of_image(p).map(|kind| Disc {
            kind,
            url: kind.url().to_string(),
            device: path.to_string(),
        });
    }

    None
}

/// Which marker directory a folder holds, if either.
///
/// Case-insensitively, because a DVD burned on one system and copied on
/// another turns up as `VIDEO_TS`, `Video_TS` or `video_ts`, and only one
/// of those is what the standard says.
fn kind_of_directory(dir: &Path) -> Option<DiscKind> {
    marker_in(dir).map(|(kind, _)| kind)
}

/// The marker directory a folder holds, and where it actually is on disk.
///
/// Split out from `kind_of_directory` because identity needs the real
/// entry: `VIDEO_TS` on a case-preserving filesystem may genuinely be
/// spelled `Video_ts`, and re-deriving the path from the canonical name
/// would fail to open it.
fn marker_in(dir: &Path) -> Option<(DiscKind, PathBuf)> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut found = None;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_ascii_uppercase();
        // Blu-ray wins a tie: a hybrid disc carrying both plays as the
        // better of the two.
        if name == DiscKind::BluRay.marker() {
            return Some((DiscKind::BluRay, entry.path()));
        }
        if name == DiscKind::Dvd.marker() {
            found = Some((DiscKind::Dvd, entry.path()));
        }
    }
    found
}

// ─── Telling one disc from another ────────────────────────────────────────
//
// A mounted disc has no path of its own. On Windows it is `E:\`, on macOS
// `/Volumes/DVD_VIDEO`, and the next disc into the same drive answers to the
// same string. Everything unflick remembers about a source — resume point,
// bookmarks, history — was keyed by that string, so a bookmark left on one
// film was offered on the next, and its resume point was applied to it.
// Disc *images* were never affected: a `.iso` has a path that means one
// thing forever, and still does.
//
// What identifies a disc here is a hash of the disc's own index: the marker
// directory's canonical name, that directory's listing, and the index file
// inside it. One `read_dir` and one small read, over a directory whose
// contents change when the disc changes — which is what a Windows drive
// root and a macOS mount point both are, so nothing about this is
// Windows-only in shape.
//
// What was rejected, and why:
//
//   * **Volume label alone.** DVDs ship labelled `DVD_VIDEO`, and a boxset
//     reuses one label across every disc in the set. It collides exactly
//     where it matters. Kept, but as the disc's *name* — for `recent` and
//     for the "wrong disc" message — never as its key.
//   * **Volume label + capacity**, the obvious pair, and still rejected. It
//     needs a different syscall per OS (`GetVolumeInformationW` +
//     `GetDiskFreeSpaceExW`; `statfs`; nothing usable for `/dev/sr0`), and
//     none of it can be exercised without authoring and mounting a real
//     image — so the Windows path would ship untested, which is how the
//     drive-letter bug got here in the first place.
//   * **The Windows volume serial number.** Windows-only in shape, and for
//     CDFS it is synthesised from the volume creation timestamp, so it
//     describes the authoring run rather than the film.
//   * **libdvdread's `DVDDiscID`** (MD5 over the first ten IFO files) — the
//     right answer if we already linked libdvdread. We do not, and ten
//     reads off a spinning disc for a database key is not "cheap to read".
//   * **Hashing the whole disc.** Correct and unusable: minutes per play.
//   * **Reading the IFO out of a `.iso`.** Unnecessary — an image's path
//     identifies it — and it would mean writing an ISO9660 *file* reader
//     when all we have is a root-directory walker, for a key we do not need.
//   * **`DefaultHasher`.** Its docs explicitly refuse to promise stability
//     across Rust releases, so every user's disc bookmarks would silently
//     detach on a toolchain bump. FNV-1a is ten lines and ours forever.
//
// Two consequences taken deliberately: a rip folder and the disc it came
// from get the same key — same content, so the bookmarks follow the film and
// moving the rip keeps them — and a `.iso` mounted as a drive letter and the
// same `.iso` played by its path get two keys, because the image has a path
// of its own and images must keep working exactly as they did.

/// Prefix every disc identity carries, so a key can be told from a path
/// without asking the filesystem anything.
pub const KEY_PREFIX: &str = "disc:";

/// Who a mounted disc is, as far as anything that remembers things is
/// concerned.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscIdentity {
    /// `disc:dvd:<16 hex>` — stable across sessions, drives and machines.
    pub key: String,
    /// The volume name, for showing a person which disc this is. Never
    /// part of the key.
    pub label: Option<String>,
    pub kind: DiscKind,
}

/// How many directory entries feed the hash.
///
/// A DVD's `VIDEO_TS` holds a handful; a Blu-ray's `BDMV` nests deeper. The
/// cap bounds the work on a pathological disc without weakening the key,
/// since the index file is hashed too.
const MAX_LISTED_ENTRIES: usize = 512;

/// How much of an index file is hashed. `VIDEO_TS.IFO` is tens of
/// kilobytes; the cap is the guard against a directory entry that claims to
/// be one and is a gigabyte.
const MAX_INDEX_BYTES: u64 = 1024 * 1024;

/// The index files that say what is actually on the disc.
fn index_files(kind: DiscKind) -> &'static [&'static str] {
    match kind {
        // VIDEO_TS.IFO *is* the VMG — the video manager the disc opens
        // with — read the cheap way, as bytes, rather than through a parser
        // we would have to write and then keep correct.
        DiscKind::Dvd => &["VIDEO_TS.IFO"],
        DiscKind::BluRay => &["index.bdmv", "MovieObject.bdmv"],
    }
}

/// Who the disc mounted at `device` is, or `None` when `device` is not a
/// mounted disc at all.
///
/// `None` for an image, for a plain file, for a `dvd://` URL and for a
/// drive with nothing readable in it — every one of those either has a path
/// that identifies it already or has nothing to identify. The caller falls
/// back to the path, which is exactly the behaviour that was there before,
/// rather than inventing a key that every empty folder would share.
pub fn identity(device: &str) -> Option<DiscIdentity> {
    let dir = Path::new(device);
    if !dir.is_dir() {
        return None;
    }
    let (kind, marker) = marker_in(dir)?;

    let mut hash = FNV_OFFSET;
    // The canonical marker name first, so a DVD and a Blu-ray can never
    // hash to the same thing even if their contents somehow did.
    hash = fnv_bytes(hash, kind.marker().as_bytes());

    // The listing: name, size, and whether it is a directory. One
    // `read_dir`, no file contents. Sorted, because directory order is the
    // filesystem's business and is not the same twice.
    let mut listed: Vec<(String, u64, bool)> = Vec::new();
    for entry in std::fs::read_dir(&marker).ok()?.flatten() {
        let name = entry.file_name().to_string_lossy().to_ascii_uppercase();
        let (len, is_dir) = match entry.metadata() {
            Ok(m) => (if m.is_dir() { 0 } else { m.len() }, m.is_dir()),
            Err(_) => (0, false),
        };
        listed.push((name, len, is_dir));
        if listed.len() >= MAX_LISTED_ENTRIES {
            break;
        }
    }
    listed.sort();
    for (name, len, is_dir) in &listed {
        hash = fnv_bytes(hash, name.as_bytes());
        hash = fnv_bytes(hash, &len.to_le_bytes());
        hash = fnv_bytes(hash, &[*is_dir as u8]);
    }

    // The index file itself — this is what tells two discs apart whose
    // listings happen to have the same names and sizes. Matched against the
    // listing we already have so a disc copied onto a case-preserving
    // filesystem, where `VIDEO_TS.IFO` comes back as `Video_ts.ifo`, is
    // still found.
    let mut read_any = false;
    for wanted in index_files(kind) {
        let upper = wanted.to_ascii_uppercase();
        if !listed.iter().any(|(n, _, is_dir)| !*is_dir && *n == upper) {
            continue;
        }
        if let Some(bytes) = read_capped(&marker, wanted) {
            hash = fnv_bytes(hash, wanted.as_bytes());
            hash = fnv_bytes(hash, &bytes);
            read_any = true;
        }
    }

    // Nothing in the marker directory at all: an empty `VIDEO_TS`, which is
    // what an unreadable disc can look like. Two of those are
    // indistinguishable, so refuse to claim otherwise. A scratched disc with
    // an unreadable IFO but a readable listing still gets a key — its VOB
    // layout is enough to tell it from the next disc.
    if !read_any && listed.is_empty() {
        return None;
    }

    Some(DiscIdentity {
        key: format!("{}{}:{:016x}", KEY_PREFIX, kind.slug(), hash),
        label: volume_label(device),
        kind,
    })
}

/// Read one file out of `dir`, case-insensitively, capped.
fn read_capped(dir: &Path, name: &str) -> Option<Vec<u8>> {
    use std::io::Read;
    let entry = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .find(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(name))?;
    let file = std::fs::File::open(entry.path()).ok()?;
    let mut buf = Vec::new();
    file.take(MAX_INDEX_BYTES).read_to_end(&mut buf).ok()?;
    (!buf.is_empty()).then_some(buf)
}

/// The disc's name, for a person to read. Never its key — see the note
/// above on why a label collides exactly where it matters.
pub fn volume_label(device: &str) -> Option<String> {
    #[cfg(target_os = "windows")]
    {
        // A drive root is the only thing Windows will report a label for.
        // Anything else is an ordinary directory and falls through below.
        if Path::new(device).parent().is_none() {
            #[link(name = "kernel32")]
            extern "system" {
                fn GetVolumeInformationW(
                    root: *const u16,
                    name_buf: *mut u16,
                    name_len: u32,
                    serial: *mut u32,
                    max_component: *mut u32,
                    flags: *mut u32,
                    fs_buf: *mut u16,
                    fs_len: u32,
                ) -> i32;
            }
            let root: Vec<u16> = device.encode_utf16().chain(std::iter::once(0)).collect();
            let mut name = [0u16; 261];
            let ok = unsafe {
                GetVolumeInformationW(
                    root.as_ptr(),
                    name.as_mut_ptr(),
                    name.len() as u32,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    0,
                )
            };
            if ok == 0 {
                return None;
            }
            let end = name.iter().position(|&c| c == 0).unwrap_or(name.len());
            let label = String::from_utf16_lossy(&name[..end]).trim().to_string();
            return (!label.is_empty()).then_some(label);
        }
    }

    // Everywhere else the mount point carries the name: macOS mounts a disc
    // at `/Volumes/<label>`, and a folder holding VIDEO_TS is named by
    // whoever ripped it.
    Path::new(device)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .filter(|s| !s.is_empty())
}

// FNV-1a, 64 bit. Chosen over `DefaultHasher` for one reason: the standard
// library will not promise its hash is the same next release, and a key that
// changed under a toolchain bump would detach every disc bookmark a user has
// without anyone noticing. Not a cryptographic hash, and does not need to
// be — 64 bits over a household's disc collection is not a collision risk.
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

fn fnv_bytes(mut hash: u64, bytes: &[u8]) -> u64 {
    for b in bytes {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    // A separator, so ("AB", "C") and ("A", "BC") are not the same disc.
    hash ^= 0xff;
    hash.wrapping_mul(FNV_PRIME)
}

// ─── Reading the image ────────────────────────────────────────────────────

const SECTOR: u64 = 2048;
/// Volume descriptors begin here, by definition, in both ISO9660 and the
/// UDF recognition sequence.
const VOLUME_DESCRIPTOR_START: u64 = 16 * SECTOR;
/// How far to keep reading descriptors before giving up. The sequence is
/// terminated properly on any real image; this is the guard against a file
/// that merely happens to have the right bytes at sector 16.
const MAX_DESCRIPTORS: u64 = 32;

/// Which kind of disc an image holds, if either.
fn kind_of_image(path: &Path) -> Option<DiscKind> {
    use std::io::{Read, Seek, SeekFrom};

    let mut f = std::fs::File::open(path).ok()?;

    let mut iso_root: Option<(u64, u64)> = None;
    let mut saw_udf = false;

    for i in 0..MAX_DESCRIPTORS {
        let mut sector = [0u8; SECTOR as usize];
        if f.seek(SeekFrom::Start(VOLUME_DESCRIPTOR_START + i * SECTOR)).is_err() {
            break;
        }
        if f.read_exact(&mut sector).is_err() {
            break;
        }
        let id = &sector[1..6];

        if id == b"CD001" {
            // 1 is the primary volume descriptor, 255 ends the set.
            if sector[0] == 1 {
                iso_root = root_directory_extent(&sector);
            } else if sector[0] == 255 {
                break;
            }
        } else if id == b"NSR02" || id == b"NSR03" {
            // The marker that this really is UDF, rather than merely
            // beginning with the "extended area" descriptor.
            saw_udf = true;
        } else if id != b"BEA01" && id != b"TEA01" && iso_root.is_none() {
            // Not a filesystem we recognise, and nothing found yet.
            break;
        }
    }

    if let Some((lba, len)) = iso_root {
        if let Some(kind) = marker_in_iso_directory(&mut f, lba, len) {
            return Some(kind);
        }
    }

    // UDF with no ISO9660 bridge to read. BD-ROM is UDF 2.50 and usually
    // carries no ISO9660 at all, while DVD-Video images are written with a
    // bridge — so this is Blu-ray by elimination. It is an inference, and
    // the reason the ISO9660 path above is tried first.
    if saw_udf {
        return Some(DiscKind::BluRay);
    }
    None
}

/// The root directory's extent (LBA, byte length) out of a primary volume
/// descriptor.
///
/// The root directory record sits at offset 156 and is 34 bytes. Both
/// fields are stored twice, little-endian then big-endian; we read the
/// little-endian halves.
fn root_directory_extent(pvd: &[u8]) -> Option<(u64, u64)> {
    let rec = pvd.get(156..190)?;
    let lba = u32::from_le_bytes(rec.get(2..6)?.try_into().ok()?) as u64;
    let len = u32::from_le_bytes(rec.get(10..14)?.try_into().ok()?) as u64;
    if len == 0 {
        return None;
    }
    Some((lba, len))
}

/// Walk one ISO9660 directory looking for `VIDEO_TS` or `BDMV`.
fn marker_in_iso_directory(
    f: &mut std::fs::File,
    lba: u64,
    len: u64,
) -> Option<DiscKind> {
    use std::io::{Read, Seek, SeekFrom};

    // A root directory is a few kilobytes. Cap the read so a corrupt length
    // field cannot ask for the whole disc.
    let len = len.min(256 * SECTOR) as usize;
    let mut buf = vec![0u8; len];
    f.seek(SeekFrom::Start(lba * SECTOR)).ok()?;
    f.read_exact(&mut buf).ok()?;

    let mut found = None;
    let mut i = 0usize;
    while i < buf.len() {
        let rec_len = buf[i] as usize;
        if rec_len == 0 {
            // Records never straddle a sector; a zero means "skip to the
            // next one".
            i = (i / SECTOR as usize + 1) * SECTOR as usize;
            continue;
        }
        if rec_len < 33 || i + rec_len > buf.len() {
            break;
        }
        let name_len = buf[i + 32] as usize;
        if let Some(raw) = buf.get(i + 33..i + 33 + name_len) {
            let name = String::from_utf8_lossy(raw).to_ascii_uppercase();
            // ISO9660 pads names with `;1` for files; directories have none,
            // but be forgiving about it.
            let name = name.trim_end_matches(";1");
            if name == DiscKind::BluRay.marker() {
                return Some(DiscKind::BluRay);
            }
            if name == DiscKind::Dvd.marker() {
                found = Some(DiscKind::Dvd);
            }
        }
        i += rec_len;
    }
    found
}

/// Make sure this process has a console before libdvdnav opens a disc.
///
/// Opening a disc kills a console-less process. Not figuratively: the
/// process fast-fails, 0xC0000409 inside `ucrtbase`, with mpv's own log
/// stopping mid-sentence at "Opening dvd://". Reproduced every time by
/// double-clicking a `.iso`, which is a GUI-subsystem launch from Explorer
/// with no console anywhere.
///
/// What was ruled out, in this order, each by measurement: it is not the
/// timing (a delay changes nothing, and an open triggered from outside a
/// third of a second later is already safe); not the thread (running it on
/// a fresh one changes nothing); not the standard handles (pointing all
/// three, Win32 *and* C runtime, at the null device changes nothing). What
/// fixes it is the process having a console at all — attaching to the
/// parent's if there is one, and otherwise allocating one and hiding it.
///
/// So the console is allocated here rather than at startup, because a
/// window flashing on every launch is not a price ordinary playback should
/// pay for a case it never hits. Once per process; a disc that is opened a
/// second time finds it already there.
#[cfg(target_os = "windows")]
pub fn ensure_console() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| unsafe {
        #[link(name = "kernel32")]
        extern "system" {
            fn GetConsoleWindow() -> *mut std::ffi::c_void;
            fn AttachConsole(process_id: u32) -> i32;
            fn AllocConsole() -> i32;
        }
        #[link(name = "user32")]
        extern "system" {
            fn ShowWindow(hwnd: *mut std::ffi::c_void, cmd: i32) -> i32;
        }
        const ATTACH_PARENT_PROCESS: u32 = 0xFFFFFFFF;
        const SW_HIDE: i32 = 0;

        if !GetConsoleWindow().is_null() {
            return;
        }
        if AttachConsole(ATTACH_PARENT_PROCESS) != 0 {
            return;
        }
        if AllocConsole() != 0 {
            // Hidden immediately: the console is there for whatever inside
            // libdvdnav wants one, not for anybody to look at.
            let w = GetConsoleWindow();
            if !w.is_null() {
                ShowWindow(w, SW_HIDE);
            }
        }
    });
}

/// Nothing to do anywhere else — this is a Windows console quirk.
#[cfg(not(target_os = "windows"))]
pub fn ensure_console() {}

/// Every optical drive on the machine, as paths that `detect` accepts.
///
/// Used by `unflick disc list` so someone can find out what is in the
/// machine without knowing what a device path looks like on their platform.
pub fn drives() -> Vec<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        // GetLogicalDrives + GetDriveType, without pulling in another
        // dependency for two calls.
        #[link(name = "kernel32")]
        extern "system" {
            fn GetLogicalDrives() -> u32;
            fn GetDriveTypeW(root: *const u16) -> u32;
        }
        const DRIVE_CDROM: u32 = 5;
        let mut out = Vec::new();
        let mask = unsafe { GetLogicalDrives() };
        for i in 0..26u32 {
            if mask & (1 << i) == 0 {
                continue;
            }
            let letter = (b'A' + i as u8) as char;
            let root: Vec<u16> = format!("{}:\\\\", letter)
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            if unsafe { GetDriveTypeW(root.as_ptr()) } == DRIVE_CDROM {
                out.push(PathBuf::from(format!("{}:\\", letter)));
            }
        }
        out
    }
    #[cfg(target_os = "macos")]
    {
        // Mounted discs appear under /Volumes like any other volume; the
        // marker directory is what tells them apart.
        std::fs::read_dir("/Volumes")
            .map(|entries| {
                entries
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| kind_of_directory(p).is_some())
                    .collect()
            })
            .unwrap_or_default()
    }
    #[cfg(target_os = "linux")]
    {
        // /dev/sr* is the drive itself, which libdvdread reads directly.
        let mut out = Vec::new();
        if let Ok(entries) = std::fs::read_dir("/dev") {
            for e in entries.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if name.starts_with("sr") && name[2..].chars().all(|c| c.is_ascii_digit()) {
                    out.push(e.path());
                }
            }
        }
        out.sort();
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build an ISO9660 image with one directory in its root.
    ///
    /// Small enough to write by hand and exact enough to be a real test of
    /// the parser: sector 16 is the primary volume descriptor, 17 the
    /// terminator, 18 the root directory.
    fn iso_with_root_entry(name: &str, udf: bool) -> Vec<u8> {
        const S: usize = SECTOR as usize;
        let mut img = vec![0u8; 20 * S];

        let mut d = 16;
        if udf {
            // The recognition sequence a UDF image opens with.
            img[d * S + 1..d * S + 6].copy_from_slice(b"BEA01");
            d += 1;
            img[d * S + 1..d * S + 6].copy_from_slice(b"NSR03");
            d += 1;
        }

        // Primary volume descriptor.
        let pvd = d * S;
        img[pvd] = 1;
        img[pvd + 1..pvd + 6].copy_from_slice(b"CD001");
        // Root directory record at offset 156: extent LBA 18, length 2048.
        img[pvd + 156] = 34;
        img[pvd + 158..pvd + 162].copy_from_slice(&18u32.to_le_bytes());
        img[pvd + 166..pvd + 170].copy_from_slice(&(S as u32).to_le_bytes());
        d += 1;

        // Terminator.
        img[d * S] = 255;
        img[d * S + 1..d * S + 6].copy_from_slice(b"CD001");

        // The root directory itself: "." then the entry under test.
        let root = 18 * S;
        img[root] = 34; // "." record, which we skip past
        img[root + 32] = 1;
        img[root + 33] = 0;

        let e = root + 34;
        let rec_len = 33 + name.len();
        img[e] = rec_len as u8;
        img[e + 25] = 0x02; // directory
        img[e + 32] = name.len() as u8;
        img[e + 33..e + 33 + name.len()].copy_from_slice(name.as_bytes());

        img
    }

    fn write_temp(name: &str, bytes: &[u8]) -> PathBuf {
        let p = std::env::temp_dir().join(name);
        std::fs::write(&p, bytes).expect("write test image");
        p
    }

    #[test]
    fn a_dvd_image_is_recognised_by_its_video_ts() {
        let p = write_temp("unflick-disc-dvd.iso", &iso_with_root_entry("VIDEO_TS", false));
        let disc = detect(&p.to_string_lossy()).expect("should be a DVD");
        assert_eq!(disc.kind, DiscKind::Dvd);
        assert_eq!(disc.url, "dvd://");
        assert_eq!(disc.device, p.to_string_lossy());
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn a_bluray_image_is_recognised_by_its_bdmv() {
        let p = write_temp("unflick-disc-bd.iso", &iso_with_root_entry("BDMV", false));
        let disc = detect(&p.to_string_lossy()).expect("should be a Blu-ray");
        assert_eq!(disc.kind, DiscKind::BluRay);
        assert_eq!(disc.url, "bd://");
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn a_udf_only_image_is_taken_for_a_bluray() {
        // BD-ROM carries no ISO9660 bridge, so there is no directory to
        // read — the filesystem itself is the only evidence.
        const S: usize = SECTOR as usize;
        let mut img = vec![0u8; 20 * S];
        img[16 * S + 1..16 * S + 6].copy_from_slice(b"BEA01");
        img[17 * S + 1..17 * S + 6].copy_from_slice(b"NSR03");
        img[18 * S + 1..18 * S + 6].copy_from_slice(b"TEA01");
        let p = write_temp("unflick-disc-udf.iso", &img);
        assert_eq!(detect(&p.to_string_lossy()).map(|d| d.kind), Some(DiscKind::BluRay));
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn a_data_image_with_no_video_on_it_is_not_a_disc() {
        // An .iso of someone's backups must stay an ordinary file, or every
        // one of them becomes an unplayable "DVD".
        let p = write_temp("unflick-disc-data.iso", &iso_with_root_entry("BACKUPS", false));
        assert_eq!(detect(&p.to_string_lossy()), None);
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn a_file_that_is_not_an_image_at_all_is_not_a_disc() {
        let p = write_temp("unflick-disc-junk.iso", b"this is not a filesystem");
        assert_eq!(detect(&p.to_string_lossy()), None);
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn an_ordinary_video_file_is_not_probed() {
        // The cheap path: no extension match, so nothing is opened.
        assert_eq!(detect("D:\\films\\something.mkv"), None);
        assert_eq!(detect("/home/alex/something.mp4"), None);
    }

    #[test]
    fn a_folder_holding_video_ts_is_a_dvd() {
        let dir = std::env::temp_dir().join("unflick-disc-folder");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("VIDEO_TS")).unwrap();
        let disc = detect(&dir.to_string_lossy()).expect("should be a DVD");
        assert_eq!(disc.kind, DiscKind::Dvd);
        assert_eq!(disc.device, dir.to_string_lossy());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn case_does_not_decide_whether_a_dvd_is_a_dvd() {
        // Copied off a disc onto a case-preserving filesystem, `VIDEO_TS`
        // comes back as `Video_TS` often enough to matter.
        let dir = std::env::temp_dir().join("unflick-disc-case");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Video_ts")).unwrap();
        assert_eq!(detect(&dir.to_string_lossy()).map(|d| d.kind), Some(DiscKind::Dvd));
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ─── Identity ─────────────────────────────────────────────────────────

    /// A directory standing in for a drive, whose disc can be swapped.
    struct FakeDrive(PathBuf);

    impl FakeDrive {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(name);
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        /// Put a disc in: wipe whatever was there and write a fresh marker
        /// directory. This is the whole bug in one method — the path does
        /// not change, the disc does.
        fn insert(&self, marker: &str, index: &str, ifo: &[u8], extras: &[(&str, usize)]) {
            let m = self.0.join(marker);
            let _ = std::fs::remove_dir_all(&m);
            std::fs::create_dir_all(&m).unwrap();
            std::fs::write(m.join(index), ifo).unwrap();
            for (name, size) in extras {
                std::fs::write(m.join(name), vec![0u8; *size]).unwrap();
            }
        }

        fn path(&self) -> String {
            self.0.to_string_lossy().into_owned()
        }
    }

    impl Drop for FakeDrive {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn dvd(drive: &FakeDrive, ifo: &[u8], extras: &[(&str, usize)]) {
        drive.insert("VIDEO_TS", "VIDEO_TS.IFO", ifo, extras);
    }

    #[test]
    fn two_discs_in_one_drive_are_not_the_same_disc() {
        // The drive-letter bug at its smallest: one path, two films.
        let drive = FakeDrive::new("unflick-id-swap");

        dvd(&drive, b"VMG for the first film", &[("VTS_01_1.VOB", 4096)]);
        let first = identity(&drive.path()).expect("a mounted DVD has an identity");

        dvd(&drive, b"VMG for the second film", &[("VTS_01_1.VOB", 8192)]);
        let second = identity(&drive.path()).expect("and so does the next one");

        assert_ne!(first.key, second.key, "two discs shared one identity");
        assert!(first.key.starts_with("disc:dvd:"), "{}", first.key);
    }

    #[test]
    fn the_same_disc_reads_the_same_twice() {
        // If it did not, a resume point would never be found again.
        let drive = FakeDrive::new("unflick-id-stable");
        dvd(&drive, b"VMG", &[("VTS_01_1.VOB", 4096), ("VTS_01_0.IFO", 100)]);
        let a = identity(&drive.path()).unwrap();
        let b = identity(&drive.path()).unwrap();
        assert_eq!(a.key, b.key);
    }

    #[test]
    fn a_disc_keeps_its_identity_in_a_different_drive() {
        // The identity is the disc's, not the mount's — otherwise moving a
        // disc from E:\ to F:\ would lose everything remembered about it.
        let one = FakeDrive::new("unflick-id-here");
        let two = FakeDrive::new("unflick-id-there");
        dvd(&one, b"VMG", &[("VTS_01_1.VOB", 4096)]);
        dvd(&two, b"VMG", &[("VTS_01_1.VOB", 4096)]);
        assert_eq!(
            identity(&one.path()).unwrap().key,
            identity(&two.path()).unwrap().key
        );
    }

    #[test]
    fn two_discs_with_identical_listings_are_still_told_apart() {
        // Same file names, same byte lengths — the listing alone cannot
        // separate these, so this is what proves the VMG is actually read.
        let drive = FakeDrive::new("unflick-id-samesize");
        dvd(&drive, b"AAAAAAAAAAAAAAAA", &[("VTS_01_1.VOB", 4096)]);
        let a = identity(&drive.path()).unwrap();
        dvd(&drive, b"BBBBBBBBBBBBBBBB", &[("VTS_01_1.VOB", 4096)]);
        let b = identity(&drive.path()).unwrap();
        assert_ne!(a.key, b.key);
    }

    #[test]
    fn a_disc_with_no_readable_index_still_has_an_identity() {
        // A scratched disc whose IFO will not read is still not the same
        // disc as the next one, and the listing says so.
        let drive = FakeDrive::new("unflick-id-noifo");
        drive.insert("VIDEO_TS", "VTS_01_1.VOB", &vec![0u8; 4096], &[]);
        let a = identity(&drive.path()).expect("the listing is enough");
        drive.insert("VIDEO_TS", "VTS_01_1.VOB", &vec![0u8; 8192], &[]);
        let b = identity(&drive.path()).unwrap();
        assert_ne!(a.key, b.key);
    }

    #[test]
    fn a_bluray_is_keyed_as_a_bluray() {
        let drive = FakeDrive::new("unflick-id-bd");
        drive.insert("BDMV", "index.bdmv", b"INDX0200", &[("MovieObject.bdmv", 512)]);
        let id = identity(&drive.path()).expect("a mounted Blu-ray has an identity");
        assert_eq!(id.kind, DiscKind::BluRay);
        assert!(id.key.starts_with("disc:bluray:"), "{}", id.key);

        // And can never collide with a DVD, whatever is inside it.
        let dvd_drive = FakeDrive::new("unflick-id-bd-vs-dvd");
        dvd(&dvd_drive, b"INDX0200", &[("MovieObject.bdmv", 512)]);
        assert_ne!(id.key, identity(&dvd_drive.path()).unwrap().key);
    }

    #[test]
    fn an_empty_drive_has_no_identity_to_give() {
        // Inventing one would give every empty drive on earth the same key.
        let drive = FakeDrive::new("unflick-id-empty");
        std::fs::create_dir_all(drive.0.join("VIDEO_TS")).unwrap();
        assert_eq!(identity(&drive.path()), None);
    }

    #[test]
    fn images_and_ordinary_files_are_never_re_keyed() {
        // An .iso has a path that means one thing forever; so does a file.
        let p = write_temp("unflick-id-image.iso", &iso_with_root_entry("VIDEO_TS", false));
        assert_eq!(identity(&p.to_string_lossy()), None);
        let _ = std::fs::remove_file(p);

        assert_eq!(identity("/home/alex/film.mkv"), None);
        assert_eq!(identity("dvd://3"), None);
    }

    #[test]
    fn the_label_names_a_disc_but_does_not_key_it() {
        // Every DVD-Video disc in the world can be labelled DVD_VIDEO, and a
        // boxset reuses one label across the set. So the label is reported
        // and the hash decides.
        let drive = FakeDrive::new("DVD_VIDEO");
        dvd(&drive, b"disc one", &[]);
        let first = identity(&drive.path()).unwrap();
        dvd(&drive, b"disc two", &[]);
        let second = identity(&drive.path()).unwrap();

        assert_eq!(first.label.as_deref(), Some("DVD_VIDEO"));
        assert_eq!(second.label.as_deref(), Some("DVD_VIDEO"));
        assert_ne!(first.key, second.key);
    }

    #[test]
    fn a_disc_url_is_taken_at_its_word() {
        // Someone asking for the menu gets the menu, and someone naming a
        // title gets that title — neither is second-guessed.
        let d = detect("dvdnav://").expect("explicit disc url");
        assert_eq!(d.kind, DiscKind::Dvd);
        assert_eq!(d.url, "dvdnav://");
        assert!(d.device.is_empty());

        assert_eq!(detect("bd://2").map(|d| d.kind), Some(DiscKind::BluRay));
        assert_eq!(detect("DVD://1").map(|d| d.url), Some("DVD://1".to_string()));
    }
}
