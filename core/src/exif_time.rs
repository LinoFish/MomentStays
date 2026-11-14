//! EXIF capture time (DateTimeOriginal) for output file timestamps.
//!
//! When the user enables "EXIF capture time as file times" (Advanced
//! Settings, default off), converted outputs are stamped with the photo's
//! own capture time instead of the source file's filesystem modification
//! time, so the files sort and display by capture time in other systems
//! too. A file that was copied or downloaded keeps its EXIF time while
//! its filesystem time becomes the transfer time; this closes that gap.
//!
//! EXIF wall-clock values carry no timezone, so they are read as local
//! time, matching what cameras record and what other tools do when no
//! offset is present (OffsetTimeOriginal is not consulted).

use crate::tiff::{find, Tiff};
use crate::{heif, jpeg};
use std::path::Path;
use std::time::{Duration, SystemTime};

/// EXIF tag numbers.
const TAG_EXIF_IFD: usize = 0x8769;
const TAG_MODIFY_DATE: usize = 0x0132;
const TAG_DATE_TIME_ORIGINAL: usize = 0x9003;
const TAG_CREATE_DATE: usize = 0x9004;
const TAG_SUB_SEC_TIME_ORIGINAL: usize = 0x9291;

/// Capture time of the still in `data` (JPEG or HEIC), if its EXIF carries
/// one. Tries DateTimeOriginal, then CreateDate, then ModifyDate.
pub fn capture_time_from_bytes(data: &[u8]) -> Option<SystemTime> {
    let tiff = exif_tiff(data)?;
    let t = Tiff::new(&tiff).ok()?;
    let (root, _) = t.root().ok()?;
    if let Some(offset) = find(&root, TAG_EXIF_IFD) {
        if let Ok((fields, _)) = t.directory(offset.value_u32(t.little)) {
            if let Some(time) = datetime_field(&t, &fields, TAG_DATE_TIME_ORIGINAL) {
                let nanos = subsec_nanos(&t, &fields, TAG_SUB_SEC_TIME_ORIGINAL);
                return wall_clock_to_system_time(time, nanos);
            }
            if let Some(time) = datetime_field(&t, &fields, TAG_CREATE_DATE) {
                return wall_clock_to_system_time(time, 0);
            }
        }
    }
    datetime_field(&t, &root, TAG_MODIFY_DATE).and_then(|time| wall_clock_to_system_time(time, 0))
}

/// Capture time of the still file at `path`, if its EXIF carries one.
pub fn capture_time_from_file(path: &Path) -> Option<SystemTime> {
    std::fs::read(path)
        .ok()
        .and_then(|data| capture_time_from_bytes(&data))
}

/// The EXIF TIFF payload of a JPEG (APP1) or HEIC (Exif item) still.
fn exif_tiff(data: &[u8]) -> Option<Vec<u8>> {
    if heif::matches(data) {
        let exif = heif::exif(data).ok()?;
        return exif.strip_prefix(jpeg::EXIF).map(|t| t.to_vec());
    }
    let segments = jpeg::segments(data).ok()?;
    segments
        .iter()
        .find(|s| s.marker == 0xE1)
        .and_then(|s| s.payload(data).strip_prefix(jpeg::EXIF).map(|t| t.to_vec()))
}

/// The ASCII value of `tag` in `fields` (NUL-terminated, as stored).
fn ascii_field(t: &Tiff, fields: &[crate::tiff::Field], tag: usize) -> Option<String> {
    let field = find(fields, tag)?;
    if field.count == 0 {
        return None;
    }
    let bytes = if field.count <= 4 {
        field.value[..field.count].to_vec()
    } else {
        let offset = field.offset(t.little).ok()?;
        t.data
            .get(offset..offset.saturating_add(field.count))?
            .to_vec()
    };
    let text = String::from_utf8_lossy(&bytes);
    Some(text.trim_end_matches(['\0', ' ', '\t']).to_string())
}

/// A `YYYY:MM:DD HH:MM:SS` value, validated; returns its components.
fn datetime_field(
    t: &Tiff,
    fields: &[crate::tiff::Field],
    tag: usize,
) -> Option<(i32, i32, i32, i32, i32, i32)> {
    let text = ascii_field(t, fields, tag)?;
    let b = text.as_bytes();
    if b.len() != 19
        || b[4] != b':'
        || b[7] != b':'
        || b[10] != b' '
        || b[13] != b':'
        || b[16] != b':'
    {
        return None;
    }
    let num = |a: usize, b: usize| -> Option<i32> { text.get(a..b)?.parse::<i32>().ok() };
    let (year, month, day, hour, min, sec) = (
        num(0, 4)?,
        num(5, 7)?,
        num(8, 10)?,
        num(11, 13)?,
        num(14, 16)?,
        num(17, 19)?,
    );
    if !(1900..=2100).contains(&year)
        || !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || !(0..=23).contains(&hour)
        || !(0..=59).contains(&min)
        || !(0..=59).contains(&sec)
    {
        return None;
    }
    Some((year, month, day, hour, min, sec))
}

/// Sub-second fraction of a DateTimeOriginal, as nanoseconds.
/// "25" means 250 ms; more than 9 digits are truncated.
fn subsec_nanos(t: &Tiff, fields: &[crate::tiff::Field], tag: usize) -> u32 {
    let Some(text) = ascii_field(t, fields, tag) else {
        return 0;
    };
    let digits: String = text.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return 0;
    }
    let mut padded = digits;
    while padded.len() < 9 {
        padded.push('0');
    }
    padded.truncate(9);
    padded.parse::<u32>().unwrap_or(0)
}

/// EXIF wall clock as local time. `libc::mktime` interprets the broken-down
/// time in the machine's local timezone, which is the EXIF convention.
fn wall_clock_to_system_time(
    (year, month, day, hour, min, sec): (i32, i32, i32, i32, i32, i32),
    nanos: u32,
) -> Option<SystemTime> {
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    tm.tm_year = year - 1900;
    tm.tm_mon = month - 1;
    tm.tm_mday = day;
    tm.tm_hour = hour;
    tm.tm_min = min;
    tm.tm_sec = sec;
    tm.tm_isdst = -1;
    let secs = unsafe { libc::mktime(&mut tm) };
    if secs < 0 {
        return None;
    }
    Some(SystemTime::UNIX_EPOCH + Duration::new(secs as u64, nanos))
}

// Keep Windows CRT compatibility isolated; Unix builds retain their libc calls.
#[cfg(windows)]
#[path = "exif_time/windows.rs"]
mod libc;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tiff::{entry, serialize};

    /// Build a minimal little-endian TIFF: IFD0 (optionally with a
    /// ModifyDate) plus an Exif sub-IFD. Entries are (tag, type, bytes);
    /// ASCII values are stored out of line, LONG values inline.
    fn tiff_bytes(root: &[(usize, usize, &[u8])], exif: &[(usize, usize, &[u8])]) -> Vec<u8> {
        let ifd0_n = root.len() + 1; // + ExifOffset
        let ifd0_size = 2 + ifd0_n * 12 + 4;
        let sub_off = 8 + ifd0_size;
        let sub_size = 2 + exif.len() * 12 + 4;
        let mut blob_at = sub_off + sub_size;
        let mut blobs: Vec<Vec<u8>> = Vec::new();
        let mut place = |value: &[u8], blobs: &mut Vec<Vec<u8>>| -> [u8; 4] {
            let mut v = [0u8; 4];
            if value.len() <= 4 {
                v[..value.len()].copy_from_slice(value);
            } else {
                v.copy_from_slice(&(blob_at as u32).to_le_bytes());
                blob_at += value.len();
                blobs.push(value.to_vec());
            }
            v
        };
        let mut root_entries: Vec<[u8; 12]> = root
            .iter()
            .map(|(tag, ty, value)| {
                let v = place(value, &mut blobs);
                let mut e = entry(true, *tag, *ty, value.len(), 0);
                e[8..12].copy_from_slice(&v);
                e
            })
            .collect();
        root_entries.push(entry(true, TAG_EXIF_IFD, 4, 1, sub_off));
        let sub_entries: Vec<[u8; 12]> = exif
            .iter()
            .map(|(tag, ty, value)| {
                let v = place(value, &mut blobs);
                let mut e = entry(true, *tag, *ty, value.len(), 0);
                e[8..12].copy_from_slice(&v);
                e
            })
            .collect();
        let mut out = Vec::new();
        out.extend_from_slice(b"II");
        out.extend_from_slice(&42u16.to_le_bytes());
        out.extend_from_slice(&8u32.to_le_bytes());
        out.extend_from_slice(&serialize(true, &root_entries, 0));
        out.extend_from_slice(&serialize(true, &sub_entries, 0));
        for blob in blobs {
            out.extend_from_slice(&blob);
        }
        out
    }

    /// Split a SystemTime back into local broken-down fields (thread-safe).
    fn local_parts(t: SystemTime) -> (i32, i32, i32, i32, i32, i32, u32) {
        let secs = t
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("test times are after the epoch")
            .as_secs() as libc::time_t;
        let nanos = t
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("test times are after the epoch")
            .subsec_nanos();
        let mut tm: libc::tm = unsafe { std::mem::zeroed() };
        let raw = unsafe { libc::localtime_r(&secs, &mut tm) };
        assert!(!raw.is_null());
        (
            tm.tm_year + 1900,
            tm.tm_mon + 1,
            tm.tm_mday,
            tm.tm_hour,
            tm.tm_min,
            tm.tm_sec,
            nanos,
        )
    }

    fn jpeg_with_exif(tiff: &[u8]) -> Vec<u8> {
        let mut payload = jpeg::EXIF.to_vec();
        payload.extend_from_slice(tiff);
        let len = u16::try_from(payload.len() + 2).expect("small test payload");
        let mut out = vec![0xFF, 0xD8, 0xFF, 0xE1];
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(&payload);
        out.extend_from_slice(&[0xFF, 0xD9]);
        out
    }

    #[test]
    fn reads_datetime_original_with_subsec() {
        let tiff = tiff_bytes(
            &[],
            &[
                (TAG_DATE_TIME_ORIGINAL, 2, b"2024:05:06 07:08:09\0"),
                (TAG_SUB_SEC_TIME_ORIGINAL, 2, b"25\0"),
            ],
        );
        let time = capture_time_from_bytes(&jpeg_with_exif(&tiff)).expect("parsed");
        assert_eq!(local_parts(time), (2024, 5, 6, 7, 8, 9, 250_000_000));
    }

    #[test]
    fn falls_back_to_create_date_then_modify_date() {
        let created = tiff_bytes(&[], &[(TAG_CREATE_DATE, 2, b"2023:01:02 03:04:05\0")]);
        let time = capture_time_from_bytes(&jpeg_with_exif(&created)).expect("parsed");
        assert_eq!(local_parts(time), (2023, 1, 2, 3, 4, 5, 0));

        let modified = tiff_bytes(&[(TAG_MODIFY_DATE, 2, b"2022:11:30 23:59:58\0")], &[]);
        let time = capture_time_from_bytes(&jpeg_with_exif(&modified)).expect("parsed");
        assert_eq!(local_parts(time), (2022, 11, 30, 23, 59, 58, 0));
    }

    #[test]
    fn prefers_original_over_create_date() {
        let tiff = tiff_bytes(
            &[],
            &[
                (TAG_DATE_TIME_ORIGINAL, 2, b"2024:05:06 07:08:09\0"),
                (TAG_CREATE_DATE, 2, b"2020:01:01 00:00:00\0"),
            ],
        );
        let time = capture_time_from_bytes(&jpeg_with_exif(&tiff)).expect("parsed");
        assert_eq!(local_parts(time), (2024, 5, 6, 7, 8, 9, 0));
    }

    #[test]
    fn rejects_garbage() {
        let bad = tiff_bytes(&[], &[(TAG_DATE_TIME_ORIGINAL, 2, b"not-a-date\0")]);
        assert!(capture_time_from_bytes(&jpeg_with_exif(&bad)).is_none());
        let out_of_range = tiff_bytes(
            &[],
            &[(TAG_DATE_TIME_ORIGINAL, 2, b"2024:13:40 25:61:61\0")],
        );
        assert!(capture_time_from_bytes(&jpeg_with_exif(&out_of_range)).is_none());
        assert!(capture_time_from_bytes(b"not a jpeg at all").is_none());
        assert!(capture_time_from_bytes(&[]).is_none());
    }

    #[test]
    fn reads_from_file() {
        let tiff = tiff_bytes(
            &[],
            &[(TAG_DATE_TIME_ORIGINAL, 2, b"2024:05:06 07:08:09\0")],
        );
        let dir =
            std::env::temp_dir().join(format!("momentstays-exif-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("source.jpg");
        std::fs::write(&path, jpeg_with_exif(&tiff)).expect("write fixture");
        let time = capture_time_from_file(&path).expect("parsed");
        assert_eq!(local_parts(time), (2024, 5, 6, 7, 8, 9, 0));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
