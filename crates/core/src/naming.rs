//! Date and time of a world zip: from the filename, otherwise from the file's modified time.

use chrono::{DateTime, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone};
use regex::Regex;
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StampSource {
    /// Date and time both came from the filename.
    Filename,
    /// Date from the filename, time of day from the file's modified time.
    FilenameDateOnly,
    /// Both from the file's modified time.
    FileTime,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Stamp {
    /// `YYYY-MM-DD`
    pub date: String,
    /// `HHMM`
    pub time: String,
    pub source: StampSource,
}

fn valid(y: i32, mo: u32, d: u32) -> Option<NaiveDate> {
    NaiveDate::from_ymd_opt(y, mo, d)
}

fn valid_time(h: u32, m: u32) -> Option<String> {
    NaiveTime::from_hms_opt(h, m, 0).map(|_| format!("{h:02}{m:02}"))
}

/// Parse a stamp out of a file name only. `None` if there is no usable date.
pub fn parse_filename(name: &str) -> Option<(String, Option<String>)> {
    // 2026-10-22_0005, 2026-10-22 00-05, 2026-10-22T00:05, 2026-10-22
    let dashed =
        Regex::new(r"(\d{4})-(\d{2})-(\d{2})(?:[ _T\-]+(\d{2})[-:.h]?(\d{2}))?").unwrap();
    // 20261022_0005
    let compact = Regex::new(r"(\d{4})(\d{2})(\d{2})[_\- T](\d{2})(\d{2})").unwrap();

    if let Some(c) = dashed.captures(name) {
        let (y, mo, d): (i32, u32, u32) =
            (c[1].parse().ok()?, c[2].parse().ok()?, c[3].parse().ok()?);
        if let Some(date) = valid(y, mo, d) {
            let time = match (c.get(4), c.get(5)) {
                (Some(h), Some(m)) => valid_time(h.as_str().parse().ok()?, m.as_str().parse().ok()?),
                _ => None,
            };
            return Some((date.format("%Y-%m-%d").to_string(), time));
        }
    }
    if let Some(c) = compact.captures(name) {
        let (y, mo, d): (i32, u32, u32) =
            (c[1].parse().ok()?, c[2].parse().ok()?, c[3].parse().ok()?);
        if let Some(date) = valid(y, mo, d) {
            let time = valid_time(c[4].parse().ok()?, c[5].parse().ok()?);
            return Some((date.format("%Y-%m-%d").to_string(), time));
        }
    }
    None
}

fn from_mtime(path: &Path) -> DateTime<Local> {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .map(DateTime::<Local>::from)
        .unwrap_or_else(|_| Local::now())
}

pub fn guess_stamp(path: &Path) -> Stamp {
    let name = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let mtime = from_mtime(path);
    let mtime_date = mtime.format("%Y-%m-%d").to_string();
    let mtime_time = mtime.format("%H%M").to_string();

    match parse_filename(&name) {
        Some((date, Some(time))) => Stamp { date, time, source: StampSource::Filename },
        Some((date, None)) => Stamp { date, time: mtime_time, source: StampSource::FilenameDateOnly },
        None => Stamp { date: mtime_date, time: mtime_time, source: StampSource::FileTime },
    }
}

/// Validate `YYYY-MM-DD` and `HHMM`, returning the normalised strings.
pub fn validate_stamp(date: &str, time: &str) -> Option<(String, String)> {
    let d = NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()?;
    if time.len() != 4 || !time.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let t = valid_time(time[..2].parse().ok()?, time[2..].parse().ok()?)?;
    Some((d.format("%Y-%m-%d").to_string(), t))
}

/// UTC offset (`+02:00`) of the local time zone at the given local date and time.
pub fn local_utc_offset(date: &str, time: &str) -> String {
    let naive = NaiveDateTime::parse_from_str(&format!("{date} {time}"), "%Y-%m-%d %H%M");
    match naive.ok().and_then(|n| Local.from_local_datetime(&n).earliest()) {
        Some(dt) => dt.format("%:z").to_string(),
        None => Local::now().format("%:z").to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_common_names() {
        assert_eq!(
            parse_filename("world_2026-10-22_0005"),
            Some(("2026-10-22".into(), Some("0005".into())))
        );
        assert_eq!(
            parse_filename("My World 2026-10-22 15-37"),
            Some(("2026-10-22".into(), Some("1537".into())))
        );
        assert_eq!(
            parse_filename("backup_20261022_1537"),
            Some(("2026-10-22".into(), Some("1537".into())))
        );
        assert_eq!(parse_filename("world_2026-10-22"), Some(("2026-10-22".into(), None)));
        assert_eq!(parse_filename("world"), None);
        assert_eq!(parse_filename("world_2026-13-40_0005"), None);
    }

    #[test]
    fn validates() {
        assert!(validate_stamp("2026-10-22", "0005").is_some());
        assert!(validate_stamp("2026-10-22", "2561").is_none());
        assert!(validate_stamp("22-10-2026", "0005").is_none());
        assert!(validate_stamp("2026-10-22", "5").is_none());
    }
}
