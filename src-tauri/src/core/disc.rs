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
    let entries = std::fs::read_dir(dir).ok()?;
    let mut found = None;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_ascii_uppercase();
        // Blu-ray wins a tie: a hybrid disc carrying both plays as the
        // better of the two.
        if name == DiscKind::BluRay.marker() {
            return Some(DiscKind::BluRay);
        }
        if name == DiscKind::Dvd.marker() {
            found = Some(DiscKind::Dvd);
        }
    }
    found
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
