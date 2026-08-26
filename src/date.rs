// RFC 822 <-> ISO 8601 conversion for pubDate / date_published.
//
// RSS wants "Tue, 03 Jun 2003 09:39:21 GMT", JSON Feed wants
// "2003-06-03T09:39:21Z". Pulling in chrono for this would violate the
// no-dependencies rule, and the two formats are simple enough to parse and
// print by hand once you have a plain (year, month, day, ...) struct in the
// middle.

struct DateTime {
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
    offset_minutes: i32,
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

fn month_from_name(name: &str) -> Option<u32> {
    MONTHS
        .iter()
        .position(|m| m.eq_ignore_ascii_case(name))
        .map(|i| i as u32 + 1)
}

fn named_zone_offset(name: &str) -> Option<i32> {
    match name.to_ascii_uppercase().as_str() {
        "UT" | "GMT" | "UTC" | "Z" => Some(0),
        "EST" => Some(-5 * 60),
        "EDT" => Some(-4 * 60),
        "CST" => Some(-6 * 60),
        "CDT" => Some(-5 * 60),
        "MST" => Some(-7 * 60),
        "MDT" => Some(-6 * 60),
        "PST" => Some(-8 * 60),
        "PDT" => Some(-7 * 60),
        _ => None,
    }
}

fn parse_offset(zone: &str) -> Option<i32> {
    let zone = zone.trim();
    if zone.is_empty() || zone.eq_ignore_ascii_case("z") {
        return Some(0);
    }
    let sign = match zone.as_bytes()[0] {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let rest = &zone[1..];
    let (h, m) = if let Some(idx) = rest.find(':') {
        (rest[..idx].parse().ok()?, rest[idx + 1..].parse().ok()?)
    } else if rest.len() == 4 {
        (rest[0..2].parse().ok()?, rest[2..4].parse().ok()?)
    } else if rest.len() == 2 {
        (rest.parse().ok()?, 0)
    } else {
        return None;
    };
    Some(sign * (h * 60 + m))
}

fn valid_date(year: i32, month: u32, day: u32, hour: u32, minute: u32, second: u32) -> bool {
    (1..=12).contains(&month) && (1..=31).contains(&day) && hour < 24 && minute < 60 && second < 61 && year > 0
}

// Zeller's congruence (Gregorian). Returns an index into WEEKDAYS.
fn weekday_index(year: i32, month: u32, day: u32) -> usize {
    let (y, m) = if month <= 2 { (year - 1, month + 12) } else { (year, month) };
    let k = y.rem_euclid(100);
    let j = y.div_euclid(100);
    let h = (day as i32 + (13 * (m as i32 + 1)) / 5 + k + k / 4 + j / 4 + 5 * j).rem_euclid(7);
    (h as usize + 6) % 7
}

fn parse_rfc822(s: &str) -> Option<DateTime> {
    let s = s.trim();
    let s = match s.find(',') {
        Some(idx) => s[idx + 1..].trim(),
        None => s,
    };
    let parts: Vec<&str> = s.split_whitespace().collect();
    if parts.len() < 4 {
        return None;
    }
    let day: u32 = parts[0].parse().ok()?;
    let month = month_from_name(parts[1])?;
    let mut year: i32 = parts[2].parse().ok()?;
    if parts[2].len() <= 2 {
        year += if year < 50 { 2000 } else { 1900 };
    }
    let time_parts: Vec<&str> = parts[3].split(':').collect();
    if time_parts.len() < 2 {
        return None;
    }
    let hour: u32 = time_parts[0].parse().ok()?;
    let minute: u32 = time_parts[1].parse().ok()?;
    let second: u32 = if time_parts.len() > 2 { time_parts[2].parse().ok()? } else { 0 };
    let offset_minutes = if parts.len() > 4 {
        named_zone_offset(parts[4]).or_else(|| parse_offset(parts[4]))?
    } else {
        0
    };
    if !valid_date(year, month, day, hour, minute, second) {
        return None;
    }
    Some(DateTime { year, month, day, hour, minute, second, offset_minutes })
}

fn parse_iso8601(s: &str) -> Option<DateTime> {
    let s = s.trim();
    if s.len() < 10 {
        return None;
    }
    let bytes = s.as_bytes();
    if bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    let year: i32 = s[0..4].parse().ok()?;
    let month: u32 = s[5..7].parse().ok()?;
    let day: u32 = s[8..10].parse().ok()?;

    let mut hour = 0u32;
    let mut minute = 0u32;
    let mut second = 0u32;
    let mut offset_minutes = 0i32;

    if s.len() > 10 {
        let rest = &s[10..];
        let rest = rest.strip_prefix('T').or_else(|| rest.strip_prefix(' ')).unwrap_or(rest);
        if rest.len() < 8 {
            return None;
        }
        hour = rest[0..2].parse().ok()?;
        minute = rest[3..5].parse().ok()?;
        second = rest[6..8].parse().ok()?;

        let mut zone_start = 8;
        let rest_bytes = rest.as_bytes();
        if zone_start < rest_bytes.len() && rest_bytes[zone_start] == b'.' {
            zone_start += 1;
            while zone_start < rest_bytes.len() && rest_bytes[zone_start].is_ascii_digit() {
                zone_start += 1;
            }
        }
        if zone_start < rest.len() {
            offset_minutes = parse_offset(&rest[zone_start..])?;
        }
    }

    if !valid_date(year, month, day, hour, minute, second) {
        return None;
    }
    Some(DateTime { year, month, day, hour, minute, second, offset_minutes })
}

fn format_rfc822(dt: &DateTime) -> String {
    let weekday = WEEKDAYS[weekday_index(dt.year, dt.month, dt.day)];
    let month = MONTHS[(dt.month - 1) as usize];
    let sign = if dt.offset_minutes < 0 { '-' } else { '+' };
    let abs = dt.offset_minutes.abs();
    format!(
        "{}, {:02} {} {:04} {:02}:{:02}:{:02} {}{:02}{:02}",
        weekday, dt.day, month, dt.year, dt.hour, dt.minute, dt.second, sign, abs / 60, abs % 60
    )
}

fn format_iso8601(dt: &DateTime) -> String {
    if dt.offset_minutes == 0 {
        format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
            dt.year, dt.month, dt.day, dt.hour, dt.minute, dt.second
        )
    } else {
        let sign = if dt.offset_minutes < 0 { '-' } else { '+' };
        let abs = dt.offset_minutes.abs();
        format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}{}{:02}:{:02}",
            dt.year, dt.month, dt.day, dt.hour, dt.minute, dt.second, sign, abs / 60, abs % 60
        )
    }
}

/// Converts a pubDate/date_published string to RFC 822 (RSS style). Accepts
/// either RFC 822 or ISO 8601 input, since the value may already be in the
/// target format if the source feed was RSS. Anything that fails to parse
/// is passed through unchanged rather than dropped.
pub fn to_rfc822(s: &str) -> String {
    if s.trim().is_empty() {
        return s.to_string();
    }
    match parse_rfc822(s).or_else(|| parse_iso8601(s)) {
        Some(dt) => format_rfc822(&dt),
        None => s.to_string(),
    }
}

/// Converts a pubDate/date_published string to ISO 8601 (JSON Feed style).
/// See `to_rfc822` for the fallback behavior on unparseable input.
pub fn to_iso8601(s: &str) -> String {
    if s.trim().is_empty() {
        return s.to_string();
    }
    match parse_iso8601(s).or_else(|| parse_rfc822(s)) {
        Some(dt) => format_iso8601(&dt),
        None => s.to_string(),
    }
}
