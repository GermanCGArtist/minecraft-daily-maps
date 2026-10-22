//! Local time handling for snapshots.

use crate::namefmt;
use anyhow::{anyhow, Result};
use chrono::{DateTime, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone};
use serde::Serialize;
use std::path::Path;
use std::time::SystemTime;

#[derive(Debug, Clone, Serialize)]
pub struct GuessedTime {
    /// YYYY-MM-DD
    pub date: String,
    /// HHMM
    pub time: String,
    /// "filename" when date came from the file name, otherwise "modified".
    pub source: String,
}

/// Date and time for a dropped zip: from the file name when possible,
/// otherwise from the file's modified time.
pub fn guess_import_time(path: &Path) -> GuessedTime {
    let mtime: DateTime<Local> = std::fs::metadata(path)
        .and_then(|m| m.modified())
        .map(DateTime::<Local>::from)
        .unwrap_or_else(|_| DateTime::<Local>::from(SystemTime::now()));
    let m_date = mtime.format("%Y-%m-%d").to_string();
    let m_time = mtime.format("%H%M").to_string();

    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    match namefmt::parse_filename(&name) {
        Some(p) => {
            let date = p.date_string();
            let time = p.time_string().unwrap_or_else(|| {
                if date == m_date {
                    m_time.clone()
                } else {
                    "0000".to_string()
                }
            });
            GuessedTime {
                date,
                time,
                source: "filename".into(),
            }
        }
        None => GuessedTime {
            date: m_date,
            time: m_time,
            source: "modified".into(),
        },
    }
}

/// `("2026-10-22T00:05:00", "+02:00")` for a local date and HHMM time.
pub fn local_datetime_info(date: &str, time: &str) -> Result<(String, String)> {
    if !namefmt::is_valid_date_str(date) {
        return Err(anyhow!("invalid date '{}', expected YYYY-MM-DD", date));
    }
    if !namefmt::is_valid_time_str(time) {
        return Err(anyhow!("invalid time '{}', expected HHMM", time));
    }
    let d = NaiveDate::parse_from_str(date, "%Y-%m-%d")?;
    let h: u32 = time[0..2].parse()?;
    let m: u32 = time[2..4].parse()?;
    let t = NaiveTime::from_hms_opt(h, m, 0).ok_or_else(|| anyhow!("invalid time"))?;
    let ndt = NaiveDateTime::new(d, t);
    let offset = match Local.from_local_datetime(&ndt).earliest() {
        Some(dt) => dt.format("%:z").to_string(),
        // Time falls into a daylight-saving gap: use the current offset.
        None => Local::now().format("%:z").to_string(),
    };
    Ok((ndt.format("%Y-%m-%dT%H:%M:00").to_string(), offset))
}

pub fn now_rfc3339() -> String {
    Local::now().to_rfc3339()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn datetime_info_format() {
        let (dt, off) = local_datetime_info("2026-10-22", "0005").unwrap();
        assert_eq!(dt, "2026-10-22T00:05:00");
        assert_eq!(off.len(), 6);
        assert!(off.starts_with('+') || off.starts_with('-'));
    }

    #[test]
    fn rejects_bad_input() {
        assert!(local_datetime_info("2026-13-01", "0005").is_err());
        assert!(local_datetime_info("2026-10-22", "2460").is_err());
    }

    #[test]
    fn guess_from_filename() {
        let g = guess_import_time(Path::new("/nonexistent/world_2026-10-22_1537.zip"));
        assert_eq!(g.date, "2026-10-22");
        assert_eq!(g.time, "1537");
        assert_eq!(g.source, "filename");
    }
}
