//! Date/time extraction from world-zip file names.
//!
//! Dependency-free on purpose so it can be unit-tested in isolation.
//! Understands, among others:
//!   world_2026-10-22_0005.zip      world_2026-10-22_00-05.zip
//!   world-20261022-0005.zip        backup 2026-10-22 15.37.zip
//!   world_2026-10-22.zip (date only)

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParsedName {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: Option<u32>,
    pub minute: Option<u32>,
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
            if leap {
                29
            } else {
                28
            }
        }
        _ => 0,
    }
}

fn valid_date(y: i32, m: u32, d: u32) -> bool {
    (1970..=2200).contains(&y) && (1..=12).contains(&m) && d >= 1 && d <= days_in_month(y, m)
}

fn valid_time(h: u32, m: u32) -> bool {
    h < 24 && m < 60
}

/// Split into groups of consecutive ASCII digits.
fn digit_groups(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() {
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            out.push(&s[start..i]);
        } else {
            i += 1;
        }
    }
    out
}

fn num(s: &str) -> Option<u32> {
    s.parse::<u32>().ok()
}

/// Try to read a time starting at `groups[i]`. Returns (hour, minute).
fn time_at(groups: &[&str], i: usize) -> Option<(u32, u32)> {
    let g = groups.get(i)?;
    // HHMM or HHMMSS in one group
    if g.len() == 4 || g.len() == 6 {
        let h = num(&g[0..2])?;
        let m = num(&g[2..4])?;
        if valid_time(h, m) {
            return Some((h, m));
        }
        return None;
    }
    // HH, MM in separate groups
    if g.len() == 2 {
        let next = groups.get(i + 1)?;
        if next.len() == 2 {
            let h = num(g)?;
            let m = num(next)?;
            if valid_time(h, m) {
                return Some((h, m));
            }
        }
    }
    None
}

/// Strip a trailing extension and any directory part.
fn stem(name: &str) -> &str {
    let base = name
        .rsplit(|c| c == '/' || c == '\\')
        .next()
        .unwrap_or(name);
    match base.rfind('.') {
        Some(idx) if idx > 0 && base.len() - idx <= 6 => &base[..idx],
        _ => base,
    }
}

pub fn parse_filename(name: &str) -> Option<ParsedName> {
    let s = stem(name);
    let groups = digit_groups(s);
    for i in 0..groups.len() {
        let g = groups[i];
        // Compact YYYYMMDD
        if g.len() == 8 {
            let y = g[0..4].parse::<i32>().ok()?;
            let m = num(&g[4..6])?;
            let d = num(&g[6..8])?;
            if valid_date(y, m, d) {
                let t = time_at(&groups, i + 1);
                return Some(ParsedName {
                    year: y,
                    month: m,
                    day: d,
                    hour: t.map(|x| x.0),
                    minute: t.map(|x| x.1),
                });
            }
        }
        // YYYY MM DD as three groups
        if g.len() == 4
            && i + 2 < groups.len()
            && groups[i + 1].len() == 2
            && groups[i + 2].len() == 2
        {
            let y = g.parse::<i32>().ok()?;
            let m = num(groups[i + 1])?;
            let d = num(groups[i + 2])?;
            if valid_date(y, m, d) {
                let t = time_at(&groups, i + 3);
                return Some(ParsedName {
                    year: y,
                    month: m,
                    day: d,
                    hour: t.map(|x| x.0),
                    minute: t.map(|x| x.1),
                });
            }
        }
    }
    None
}

impl ParsedName {
    pub fn date_string(&self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
    pub fn time_string(&self) -> Option<String> {
        match (self.hour, self.minute) {
            (Some(h), Some(m)) => Some(format!("{:02}{:02}", h, m)),
            _ => None,
        }
    }
}

/// Validate "YYYY-MM-DD".
pub fn is_valid_date_str(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return false;
    }
    match (
        s[0..4].parse::<i32>(),
        s[5..7].parse::<u32>(),
        s[8..10].parse::<u32>(),
    ) {
        (Ok(y), Ok(m), Ok(d)) => valid_date(y, m, d),
        _ => false,
    }
}

/// Validate "HHMM".
pub fn is_valid_time_str(s: &str) -> bool {
    if s.len() != 4 || !s.bytes().all(|c| c.is_ascii_digit()) {
        return false;
    }
    valid_time(s[0..2].parse().unwrap_or(99), s[2..4].parse().unwrap_or(99))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(name: &str) -> Option<(String, Option<String>)> {
        parse_filename(name).map(|x| (x.date_string(), x.time_string()))
    }

    #[test]
    fn iso_with_hhmm() {
        assert_eq!(
            p("world_2026-10-22_0005.zip"),
            Some(("2026-10-22".into(), Some("0005".into())))
        );
    }
    #[test]
    fn iso_with_dash_time() {
        assert_eq!(
            p("world_2026-10-22_15-37.zip"),
            Some(("2026-10-22".into(), Some("1537".into())))
        );
    }
    #[test]
    fn dotted_time_with_spaces() {
        assert_eq!(
            p("backup 2026-10-22 15.37.zip"),
            Some(("2026-10-22".into(), Some("1537".into())))
        );
    }
    #[test]
    fn compact_date() {
        assert_eq!(
            p("world-20261022-0005.zip"),
            Some(("2026-10-22".into(), Some("0005".into())))
        );
    }
    #[test]
    fn seconds_are_ignored() {
        assert_eq!(
            p("w_2026-10-22_153742.zip"),
            Some(("2026-10-22".into(), Some("1537".into())))
        );
    }
    #[test]
    fn date_only() {
        assert_eq!(p("world_2026-10-22.zip"), Some(("2026-10-22".into(), None)));
    }
    #[test]
    fn no_date() {
        assert_eq!(p("world.zip"), None);
        assert_eq!(p("world_1234.zip"), None);
    }
    #[test]
    fn invalid_calendar_dates_rejected() {
        assert_eq!(p("world_2026-13-40_0005.zip"), None);
        assert_eq!(p("world_2026-02-30.zip"), None);
    }
    #[test]
    fn leap_day() {
        assert_eq!(p("world_2028-02-29.zip"), Some(("2028-02-29".into(), None)));
        assert_eq!(p("world_2027-02-29.zip"), None);
    }
    #[test]
    fn bad_time_is_dropped_not_fatal() {
        assert_eq!(
            p("world_2026-10-22_2575.zip"),
            Some(("2026-10-22".into(), None))
        );
    }
    #[test]
    fn path_prefix_is_ignored() {
        assert_eq!(
            p("/tmp/2020-01-01/world_2026-10-22_0005.zip").map(|x| x.0),
            Some("2026-10-22".to_string())
        );
    }
    #[test]
    fn validators() {
        assert!(is_valid_date_str("2026-10-22"));
        assert!(!is_valid_date_str("2026-10-32"));
        assert!(!is_valid_date_str("26-10-22"));
        assert!(is_valid_time_str("0005"));
        assert!(!is_valid_time_str("2460"));
        assert!(!is_valid_time_str("005"));
    }
}
