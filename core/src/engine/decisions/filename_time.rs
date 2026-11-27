//! Capture time inferred from an original filename: a port of Swift
//! `PhotoTimeRepair.inferredTime` (macOS `PhotoTimeRepair.swift`).
//!
//! Only complete date+time tokens or Unix timestamps count; anything
//! invalid or ambiguous yields None so the caller falls back to its own
//! ordering, never to file system dates.
//!
//! Difference by design: a token without an explicit offset is returned as
//! local wall-clock components. Resolving them in an IANA zone — and
//! rejecting a wall time the zone skips (DST gap), as Swift's `Calendar`
//! round trip does — stays with the host, which owns the time zone
//! database (Foundation on Apple, GLib `GTimeZone` on GNOME). Digits are
//! ASCII only (ICU's `\d` also accepts other Unicode digits).
use serde_json::{json, Value};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    FilenameDateTime,
    UnixTimestamp,
}
impl Source {
    /// Swift `TimeSource` raw values.
    pub fn code(self) -> &'static str {
        match self {
            Source::FilenameDateTime => "filenameDateTime",
            Source::UnixTimestamp => "unixTimestamp",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Inferred {
    pub source: Source,
    /// Wall-clock year, month, day, hour, minute, second (calendar tokens).
    pub local: Option<[u32; 6]>,
    /// Explicit offset (`Z`, `+08:00`, `-0530`) in seconds east of UTC.
    pub offset_seconds: Option<i32>,
    /// Sub-second fraction from `.123`.
    pub fraction: f64,
    /// Seconds since 1970-01-01 UTC, when known without a zone database
    /// (Unix tokens, or calendar tokens with an explicit offset).
    pub unix: Option<f64>,
    /// The matched digits (`.123` fraction digits, or the whole Unix token),
    /// so a host can redo the floating-point steps exactly as it did before
    /// (Swift: `Double("0." + digits)`, `Double(token) / pow(10, n - 10)`).
    pub digits: Option<String>,
}

impl Inferred {
    pub fn json(&self) -> Value {
        json!({"source": self.source.code(), "local": self.local,
            "offset_seconds": self.offset_seconds, "fraction": self.fraction, "unix": self.unix,
            "digits": self.digits})
    }
}

/// `NSString.lastPathComponent.deletingPathExtension`, as observed on
/// macOS: trailing slashes are not a component, and text after the last dot
/// that contains a space is not an extension (`a.29 02-30-00` keeps its dot).
/// Apple hosts pass Foundation's own stem instead (`infer_stem`).
pub fn stem(filename: &str) -> &str {
    let trimmed = filename.trim_end_matches('/');
    let name = if trimmed.is_empty() {
        filename
    } else {
        trimmed.rsplit('/').next().unwrap_or(trimmed)
    };
    match name.rfind('.') {
        Some(i) if i > 0 && !name[i + 1..].contains(' ') => &name[..i],
        _ => name,
    }
}

#[derive(Clone, Copy)]
enum Tok {
    Digits(usize),
    Sep(&'static [u8]),
    OptSep(&'static [u8]),
}

const DATE_SEP: &[u8] = b"-_";
const TIME_SEP: &[u8] = b"-_:";
const T_SEP: &[u8] = b"T _-";

const DASHED: [Tok; 11] = [
    Tok::Digits(4),
    Tok::Sep(DATE_SEP),
    Tok::Digits(2),
    Tok::Sep(DATE_SEP),
    Tok::Digits(2),
    Tok::Sep(T_SEP),
    Tok::Digits(2),
    Tok::OptSep(TIME_SEP),
    Tok::Digits(2),
    Tok::OptSep(TIME_SEP),
    Tok::Digits(2),
];
const COMPACT: [Tok; 9] = [
    Tok::Digits(4),
    Tok::Digits(2),
    Tok::Digits(2),
    Tok::OptSep(T_SEP),
    Tok::Digits(2),
    Tok::OptSep(TIME_SEP),
    Tok::Digits(2),
    Tok::OptSep(TIME_SEP),
    Tok::Digits(2),
];

struct Match {
    end: usize,
    parts: Vec<u32>,
    fraction: Option<std::ops::Range<usize>>,
    zone: Option<std::ops::Range<usize>>,
}

fn sep_ok(set: &[u8], c: u8) -> bool {
    // (?i): `T` also matches `t`.
    set.contains(&c) || (set.contains(&b'T') && c == b't')
}

/// Backtracking match of `toks` + suffix at `at`, in regex priority order.
fn match_at(s: &[u8], toks: &[Tok], at: usize, parts: &mut Vec<u32>) -> Option<Match> {
    let Some((first, rest)) = toks.split_first() else {
        return suffix(s, at, parts);
    };
    match *first {
        Tok::Digits(n) => {
            let d = s.get(at..at + n)?;
            if !d.iter().all(u8::is_ascii_digit) {
                return None;
            }
            parts.push(std::str::from_utf8(d).ok()?.parse().ok()?);
            let m = match_at(s, rest, at + n, parts);
            if m.is_none() {
                parts.pop();
            }
            m
        }
        Tok::Sep(set) => match s.get(at) {
            Some(&c) if sep_ok(set, c) => match_at(s, rest, at + 1, parts),
            _ => None,
        },
        Tok::OptSep(set) => {
            if let Some(&c) = s.get(at) {
                if sep_ok(set, c) {
                    if let Some(m) = match_at(s, rest, at + 1, parts) {
                        return Some(m);
                    }
                }
            }
            match_at(s, rest, at, parts)
        }
    }
}

/// `(?:\.(\d{1,9}))?(Z|[+-]\d{2}:?\d{2})?(?!\d)` with greedy backtracking.
fn suffix(s: &[u8], at: usize, parts: &[u32]) -> Option<Match> {
    let digit = |i: usize| s.get(i).is_some_and(u8::is_ascii_digit);
    let mut fractions: Vec<Option<std::ops::Range<usize>>> = Vec::new();
    if s.get(at) == Some(&b'.') && digit(at + 1) {
        let run = (at + 1..s.len()).take_while(|&i| digit(i)).count().min(9);
        for n in (1..=run).rev() {
            fractions.push(Some(at + 1..at + 1 + n));
        }
    }
    fractions.push(None);
    for fraction in fractions {
        let p = fraction.as_ref().map_or(at, |r| r.end);
        let mut zones: Vec<Option<std::ops::Range<usize>>> = Vec::new();
        match s.get(p) {
            Some(b'Z') | Some(b'z') => zones.push(Some(p..p + 1)),
            Some(b'+') | Some(b'-') if digit(p + 1) && digit(p + 2) => {
                if s.get(p + 3) == Some(&b':') && digit(p + 4) && digit(p + 5) {
                    zones.push(Some(p..p + 6));
                }
                if digit(p + 3) && digit(p + 4) {
                    zones.push(Some(p..p + 5));
                }
            }
            _ => {}
        }
        zones.push(None);
        for zone in zones {
            let end = zone.as_ref().map_or(p, |r| r.end);
            if !digit(end) {
                return Some(Match {
                    end,
                    parts: parts.to_vec(),
                    fraction,
                    zone,
                });
            }
        }
    }
    None
}

fn find_all(s: &[u8], toks: &[Tok]) -> Vec<(usize, Match)> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < s.len() {
        let preceded = at > 0 && s[at - 1].is_ascii_digit();
        if !preceded {
            if let Some(m) = match_at(s, toks, at, &mut Vec::new()) {
                let end = m.end;
                out.push((at, m));
                at = end.max(at + 1);
                continue;
            }
        }
        at += 1;
    }
    out
}

fn leap(y: u32) -> bool {
    (y.is_multiple_of(4) && !y.is_multiple_of(100)) || y.is_multiple_of(400)
}

fn valid(p: &[u32]) -> bool {
    let days = [
        31,
        if leap(p[0]) { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    (1..=9999).contains(&p[0])
        && (1..=12).contains(&p[1])
        && p[2] >= 1
        && p[2] <= days[(p[1] - 1) as usize]
        && p[3] < 24
        && p[4] < 60
        && p[5] < 60
}

/// Days from 1970-01-01 (proleptic Gregorian).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// Seconds since the Unix epoch for wall-clock `p` at `offset` east of UTC.
pub fn unix_at_offset(p: [u32; 6], offset_seconds: i32) -> i64 {
    days_from_civil(p[0] as i64, p[1] as i64, p[2] as i64) * 86400
        + p[3] as i64 * 3600
        + p[4] as i64 * 60
        + p[5] as i64
        - offset_seconds as i64
}

/// Swift `inferredTime(filename:zone:)` without the zone (see module docs).
pub fn infer(filename: &str) -> Option<Inferred> {
    infer_stem(stem(filename))
}

/// `infer` for a name whose directory and extension the host already
/// removed (Swift: `lastPathComponent.deletingPathExtension`).
pub fn infer_stem(stem: &str) -> Option<Inferred> {
    let s = stem.as_bytes();
    let mut candidates = Vec::new();
    let mut ranges: Vec<std::ops::Range<usize>> = Vec::new();
    let mut invalid = false;
    for toks in [&DASHED[..], &COMPACT[..]] {
        for (start, m) in find_all(s, toks) {
            ranges.push(start..m.end);
            let mut offset = None;
            if let Some(z) = &m.zone {
                let token = &s[z.clone()];
                if token.eq_ignore_ascii_case(b"Z") {
                    offset = Some(0);
                } else {
                    let digits: Vec<u8> =
                        token[1..].iter().copied().filter(|c| *c != b':').collect();
                    let h: i32 = std::str::from_utf8(&digits[..2]).ok()?.parse().ok()?;
                    let mi: i32 = std::str::from_utf8(&digits[2..]).ok()?.parse().ok()?;
                    if h > 14 || mi >= 60 || (h == 14 && mi != 0) {
                        invalid = true;
                        continue;
                    }
                    let sign = if token[0] == b'-' { -1 } else { 1 };
                    offset = Some(sign * (h * 3600 + mi * 60));
                }
            }
            let p: [u32; 6] = m.parts[..6].try_into().ok()?;
            if !valid(&p) {
                invalid = true;
                continue;
            }
            let digits = m
                .fraction
                .map(|r| String::from_utf8_lossy(&s[r]).into_owned());
            let fraction = digits
                .as_ref()
                .map_or(0.0, |d| format!("0.{d}").parse().unwrap_or(0.0));
            candidates.push(Inferred {
                source: Source::FilenameDateTime,
                local: Some(p),
                offset_seconds: offset,
                fraction,
                unix: offset.map(|o| unix_at_offset(p, o) as f64 + fraction),
                digits,
            });
        }
    }
    // (?<!\d)(\d{10}|\d{13}|\d{16}|\d{19})(?!\d): whole digit runs.
    let mut at = 0;
    while at < s.len() {
        if !s[at].is_ascii_digit() {
            at += 1;
            continue;
        }
        let end = (at..s.len())
            .find(|&i| !s[i].is_ascii_digit())
            .unwrap_or(s.len());
        let n = end - at;
        if [10, 13, 16, 19].contains(&n) && !ranges.iter().any(|r| r.start < end && at < r.end) {
            let token = std::str::from_utf8(&s[at..end]).ok()?;
            let value: f64 = token.parse().ok()?;
            candidates.push(Inferred {
                source: Source::UnixTimestamp,
                local: None,
                offset_seconds: Some(0),
                fraction: 0.0,
                unix: Some(value / 10f64.powi(n as i32 - 10)),
                digits: Some(token.into()),
            });
        }
        at = end;
    }
    if invalid || candidates.len() != 1 {
        return None;
    }
    candidates.pop()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Vectors from apple/macos/tests/PhotoTimeRepairTests.swift `parsing()`.
    const EXPECTED: i64 = 1_709_181_296; // 2024-02-29 12:34:56 +08:00

    fn shanghai(i: &Inferred) -> f64 {
        i.unix
            .unwrap_or_else(|| unix_at_offset(i.local.unwrap(), 8 * 3600) as f64 + i.fraction)
    }

    #[test]
    fn calendar_names() {
        for name in [
            "IMG_20240229_123456.jpg",
            "20240229123456.heic",
            "2024-02-29_123456.jpg",
            "20240229_12-34-56.jpg",
            "2024-02-29 12-34-56.png",
            "2024_02_29_12_34_56.tif",
            "2024-02-29T12:34:56+08:00.jpg",
            "2024-02-29T04:34:56Z.jpg",
            "2024-02-29t04:34:56z.jpg",
        ] {
            let i = infer(name).unwrap_or_else(|| panic!("{name}"));
            assert_eq!(i.source, Source::FilenameDateTime, "{name}");
            assert_eq!(shanghai(&i), EXPECTED as f64, "{name}");
        }
        assert_eq!(
            infer("IMG_20240229_123456.jpg").unwrap().local,
            Some([2024, 2, 29, 12, 34, 56])
        );
        let f = infer("20240229_123456.123.jpg").unwrap();
        assert!((shanghai(&f) - EXPECTED as f64 - 0.123).abs() < 1e-5);
        assert_eq!(f.digits.as_deref(), Some("123"));
        assert_eq!(infer("IMG_20240229_123456.jpg").unwrap().digits, None);
        let z = infer("2024-02-29T12:34:56-0530.jpg").unwrap();
        assert_eq!(z.offset_seconds, Some(-(5 * 3600 + 30 * 60)));
    }

    #[test]
    fn unix_names() {
        for factor in [1i64, 1000, 1_000_000, 1_000_000_000] {
            let name = format!("photo_{}.jpg", EXPECTED * factor);
            let i = infer(&name).unwrap();
            assert_eq!(i.source, Source::UnixTimestamp);
            assert_eq!(i.unix, Some(EXPECTED as f64));
            assert_eq!(i.digits, Some((EXPECTED * factor).to_string()));
        }
        let ms = infer(&format!("{}.jpg", EXPECTED * 1000 + 123)).unwrap();
        assert!((ms.unix.unwrap() - EXPECTED as f64 - 0.123).abs() < 1e-5);
    }

    #[test]
    fn invalid_and_ambiguous_names() {
        for name in [
            "1.jpg",
            "2024-02-29.jpg",
            "2023-02-29_12-34-56.jpg",
            "2024-02-30_12-34-56.jpg",
            "2024-01-01_24-00-00.jpg",
            "2024-01-01T12:00:00+25:00.jpg",
            "12345678901234567890.jpg",
            "123456789012.jpg",
            "202402291234567.jpg",
            "1700000000_1800000000.jpg",
            "20240229_123456_1700000000.jpg",
            "20240229_123456_20240301_123456.jpg",
        ] {
            assert_eq!(infer(name), None, "{name}");
        }
        // A DST-gap wall time is returned as components; the host's zone
        // database rejects it (Swift: America/Los_Angeles test vector).
        let gap = infer("2024-03-10_02-30-00.jpg").unwrap();
        assert_eq!(gap.local, Some([2024, 3, 10, 2, 30, 0]));
        assert_eq!(gap.unix, None);
    }

    #[test]
    fn stems_follow_foundation() {
        assert_eq!(stem("IMG_1.jpg"), "IMG_1");
        assert_eq!(stem("dir/IMG_1.jpg/"), "IMG_1");
        assert_eq!(stem(".hidden"), ".hidden");
        assert_eq!(stem("a.tar.gz"), "a.tar");
        // An "extension" with a space is part of the name on macOS.
        assert_eq!(
            stem("VID_2000.13-07 013000_1700000000"),
            "VID_2000.13-07 013000_1700000000"
        );
        assert_eq!(
            infer("VID_2000.13-07 013000_1700000000").unwrap().unix,
            Some(1_700_000_000.0)
        );
        // The host's stem is taken as is.
        assert_eq!(
            infer_stem("20240229_123456.123").unwrap().digits.as_deref(),
            Some("123")
        );
        assert_eq!(
            infer_stem("x.1700000000").unwrap().source,
            Source::UnixTimestamp
        );
    }

    #[test]
    fn civil_days() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2000, 3, 1), 11017);
        assert_eq!(unix_at_offset([2024, 2, 29, 4, 34, 56], 0), EXPECTED);
    }
}
