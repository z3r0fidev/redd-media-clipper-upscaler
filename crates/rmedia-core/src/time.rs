//! Time values (`Ts`, microseconds) and the user-facing timestamp grammar (FR-002).
//!
//! Accepted positions: `90`, `90.5`, `1:30`, `01:30.5`, `00:01:30.500`, `00:01:30,500`,
//! `1h2m3.5s`, `500ms`. Durations are written with a leading `+`. `end` means end of media.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

const US_PER_SEC: i64 = 1_000_000;

/// A non-negative time value in microseconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, type = "number"))]
pub struct Ts(i64);

impl Ts {
    pub const ZERO: Ts = Ts(0);

    pub const fn from_micros(us: i64) -> Ts {
        Ts(us)
    }

    pub fn from_secs_f64(secs: f64) -> Ts {
        Ts((secs * US_PER_SEC as f64).round() as i64)
    }

    pub const fn as_micros(self) -> i64 {
        self.0
    }

    pub fn as_secs_f64(self) -> f64 {
        self.0 as f64 / US_PER_SEC as f64
    }

    /// Parses a position (no `+`, no `end`).
    pub fn parse(text: &str) -> Result<Ts, TimeError> {
        let s = text.trim();
        if s.is_empty() {
            return Err(TimeError::new(text, "empty time"));
        }
        if s.starts_with('-') {
            return Err(TimeError::new(text, "negative times are not allowed"));
        }
        let bad = || TimeError::new(text, "expected seconds, [HH:]MM:SS[.mmm] or 1h2m3s");
        if s.contains(':') {
            parse_clock(s).ok_or_else(bad)
        } else if s.chars().any(|c| c.is_ascii_alphabetic()) {
            parse_units(s).ok_or_else(bad)
        } else {
            parse_decimal(s, '.').map(Ts).ok_or_else(bad)
        }
    }

    pub fn saturating_sub(self, other: Ts) -> Ts {
        Ts((self.0 - other.0).max(0))
    }

    /// `HH:MM:SS.mmm` (rounded to milliseconds).
    pub fn display(self) -> String {
        let (h, m, s, ms) = self.hms_millis();
        format!("{h:02}:{m:02}:{s:02}.{ms:03}")
    }

    /// `HH-MM-SS.mmm`, safe for file names on every platform.
    pub fn filename(self) -> String {
        let (h, m, s, ms) = self.hms_millis();
        format!("{h:02}-{m:02}-{s:02}.{ms:03}")
    }

    /// Seconds with microsecond precision, as ffmpeg's `-ss`/`-t` expect.
    pub fn ffmpeg(self) -> String {
        format!("{}.{:06}", self.0 / US_PER_SEC, self.0 % US_PER_SEC)
    }

    fn hms_millis(self) -> (i64, i64, i64, i64) {
        let total_ms = (self.0 + 500) / 1000;
        let ms = total_ms % 1000;
        let total_s = total_ms / 1000;
        (total_s / 3600, (total_s / 60) % 60, total_s % 60, ms)
    }
}

impl std::ops::Add for Ts {
    type Output = Ts;
    fn add(self, rhs: Ts) -> Ts {
        Ts(self.0 + rhs.0)
    }
}

impl std::ops::Sub for Ts {
    type Output = Ts;
    fn sub(self, rhs: Ts) -> Ts {
        Ts(self.0 - rhs.0)
    }
}

impl fmt::Display for Ts {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.display())
    }
}

/// Serialized as seconds (number) in all JSON output.
impl Serialize for Ts {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_f64(self.as_secs_f64())
    }
}

/// Accepts a number of seconds or a time string.
impl<'de> Deserialize<'de> for Ts {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Ts, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Num(f64),
            Str(String),
        }
        match Raw::deserialize(d)? {
            Raw::Num(n) if n.is_finite() && n >= 0.0 => Ok(Ts::from_secs_f64(n)),
            Raw::Num(n) => Err(serde::de::Error::custom(format!("invalid time {n}"))),
            Raw::Str(s) => Ts::parse(&s).map_err(serde::de::Error::custom),
        }
    }
}

/// How a clip's end is specified.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndSpec {
    At(Ts),
    Duration(Ts),
    EndOfMedia,
}

impl EndSpec {
    /// Parses `end`, `+<time>` (duration) or a position.
    pub fn parse(text: &str) -> Result<EndSpec, TimeError> {
        let s = text.trim();
        if s.eq_ignore_ascii_case("end") {
            Ok(EndSpec::EndOfMedia)
        } else if let Some(rest) = s.strip_prefix('+') {
            Ts::parse(rest).map(EndSpec::Duration)
        } else {
            Ts::parse(s).map(EndSpec::At)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid time \"{input}\": {reason}")]
pub struct TimeError {
    pub input: String,
    pub reason: &'static str,
}

impl TimeError {
    fn new(input: &str, reason: &'static str) -> Self {
        TimeError { input: input.to_owned(), reason }
    }
}

/// `digits[<sep>digits]` → microseconds. At most 6 fractional digits.
fn parse_decimal(s: &str, sep: char) -> Option<i64> {
    let (int, frac) = match s.split_once(sep) {
        Some((i, f)) => (i, Some(f)),
        None => (s, None),
    };
    if int.is_empty() || !int.bytes().all(|b| b.is_ascii_digit()) || int.len() > 12 {
        return None;
    }
    let mut us = int.parse::<i64>().ok()?.checked_mul(US_PER_SEC)?;
    if let Some(f) = frac {
        if f.is_empty() || f.len() > 6 || !f.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let padded = format!("{f:0<6}");
        us += padded.parse::<i64>().ok()?;
    }
    Some(us)
}

fn parse_clock(s: &str) -> Option<Ts> {
    let parts: Vec<&str> = s.split(':').collect();
    if !(2..=3).contains(&parts.len()) {
        return None;
    }
    let (secs_part, rest) = parts.split_last()?;
    // Seconds field: 1-2 integer digits, optional '.' or ',' fraction.
    let sep = if secs_part.contains(',') { ',' } else { '.' };
    let sec_int = secs_part.split(sep).next()?;
    if sec_int.is_empty() || sec_int.len() > 2 {
        return None;
    }
    let sec_us = parse_decimal(secs_part, sep)?;
    if sec_us >= 60 * US_PER_SEC {
        return None;
    }
    let mut total = sec_us;
    for (i, field) in rest.iter().rev().enumerate() {
        let leading = i == rest.len() - 1;
        if field.is_empty() || !field.bytes().all(|b| b.is_ascii_digit()) || field.len() > 9 {
            return None;
        }
        if !leading && field.len() > 2 {
            return None;
        }
        let v: i64 = field.parse().ok()?;
        if !leading && v >= 60 {
            return None;
        }
        let unit = if i == 0 { 60 } else { 3600 };
        total = total.checked_add(v.checked_mul(unit * US_PER_SEC)?)?;
    }
    Some(Ts(total))
}

/// `1h2m3.5s`, `90s`, `500ms`: units in descending order, each at most once.
fn parse_units(s: &str) -> Option<Ts> {
    const UNITS: [(&str, i64); 4] =
        [("h", 3600 * US_PER_SEC), ("m", 60 * US_PER_SEC), ("s", US_PER_SEC), ("ms", 1000)];
    let mut rest = s;
    let mut total: i64 = 0;
    let mut next_unit = 0;
    while !rest.is_empty() {
        let num_len = rest.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(rest.len());
        let (num, tail) = rest.split_at(num_len);
        if num.is_empty() {
            return None;
        }
        let unit_len = tail.find(|c: char| !c.is_ascii_alphabetic()).unwrap_or(tail.len());
        let (unit, tail) = tail.split_at(unit_len);
        let idx = UNITS.iter().position(|(u, _)| *u == unit)?;
        if idx < next_unit {
            return None;
        }
        next_unit = idx + 1;
        // value in microseconds-of-a-second, scaled by the unit
        let v_us = parse_decimal(num, '.')?;
        let scaled = (v_us as i128 * UNITS[idx].1 as i128) / US_PER_SEC as i128;
        total = total.checked_add(i64::try_from(scaled).ok()?)?;
        rest = tail;
    }
    Some(Ts(total))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn us(s: f64) -> Ts {
        Ts::from_secs_f64(s)
    }

    #[test]
    fn parses_valid_positions() {
        let cases: &[(&str, f64)] = &[
            ("0", 0.0),
            ("90", 90.0),
            ("90.5", 90.5),
            ("90.123456", 90.123456),
            (" 7 ", 7.0),
            ("1:30", 90.0),
            ("01:30", 90.0),
            ("1:30.5", 90.5),
            ("01:30,5", 90.5),
            ("00:01:30", 90.0),
            ("00:01:30.500", 90.5),
            ("00:01:30,500", 90.5),
            ("1:00:00", 3600.0),
            ("90:00", 5400.0),
            ("100:00:00", 360000.0),
            ("0:0", 0.0),
            ("0:05", 5.0),
            ("1:2:3", 3723.0),
            ("1h", 3600.0),
            ("2m", 120.0),
            ("1m30s", 90.0),
            ("1h2m3.5s", 3723.5),
            ("1h30s", 3630.0),
            ("90s", 90.0),
            ("1.5m", 90.0),
            ("500ms", 0.5),
            ("1s500ms", 1.5),
            ("0.000001", 0.000001),
        ];
        for (input, secs) in cases {
            assert_eq!(Ts::parse(input), Ok(us(*secs)), "input {input:?}");
        }
    }

    #[test]
    fn rejects_malformed_positions() {
        let cases = [
            "",
            " ",
            "-1",
            "-0:30",
            "abc",
            "1:7x",
            "1:75",
            "1:60",
            "01:60:00",
            "1:2:3:4",
            ":30",
            "1:",
            "1::2",
            "1.2.3",
            "1e3",
            "90,5",
            "1.1234567",
            "0x10",
            "1m1h",
            "1s1s",
            "5 s",
            "1:30.",
            "1:123",
            "1,5:00",
            "1q",
            "m",
            "inf",
            "NaN",
            "１２",
        ];
        for input in cases {
            assert!(Ts::parse(input).is_err(), "input {input:?} should be rejected");
        }
    }

    #[test]
    fn parses_end_specs() {
        assert_eq!(EndSpec::parse("end"), Ok(EndSpec::EndOfMedia));
        assert_eq!(EndSpec::parse("END"), Ok(EndSpec::EndOfMedia));
        assert_eq!(EndSpec::parse("+15"), Ok(EndSpec::Duration(us(15.0))));
        assert_eq!(EndSpec::parse("+1:00"), Ok(EndSpec::Duration(us(60.0))));
        assert_eq!(EndSpec::parse("2:00"), Ok(EndSpec::At(us(120.0))));
        assert!(EndSpec::parse("+").is_err());
        assert!(EndSpec::parse("++1").is_err());
        assert!(EndSpec::parse("+-1").is_err());
    }

    #[test]
    fn formats() {
        let t = us(3723.4567);
        assert_eq!(t.display(), "01:02:03.457");
        assert_eq!(t.filename(), "01-02-03.457");
        assert_eq!(t.ffmpeg(), "3723.456700");
        assert_eq!(Ts::ZERO.ffmpeg(), "0.000000");
        assert_eq!(us(59.9996).display(), "00:01:00.000");
    }

    #[test]
    fn serde_round_trip() {
        assert_eq!(serde_json::to_string(&us(1.5)).unwrap(), "1.5");
        let a: Ts = serde_json::from_str("\"1:30\"").unwrap();
        let b: Ts = serde_json::from_str("90").unwrap();
        assert_eq!(a, b);
        assert!(serde_json::from_str::<Ts>("-1").is_err());
        assert!(serde_json::from_str::<Ts>("\"x\"").is_err());
    }
}
