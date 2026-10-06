//! Parser for ffmpeg `-progress pipe:1` output (key=value blocks ending in `progress=…`).

use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProgressBlock {
    /// Output position in microseconds, if known.
    pub out_time_us: Option<i64>,
    /// Encoding speed relative to real time (e.g. 31.5 for "31.5x").
    pub speed: Option<f32>,
    /// True for the final block (`progress=end`).
    pub done: bool,
}

#[derive(Debug, Default)]
pub struct ProgressParser {
    out_time_us: Option<i64>,
    out_time_fallback: Option<i64>,
    speed: Option<f32>,
}

impl ProgressParser {
    /// Feeds one line; returns a block when a `progress=` line completes it.
    pub fn feed(&mut self, line: &str) -> Option<ProgressBlock> {
        let (key, value) = line.trim().split_once('=')?;
        let value = value.trim();
        match key {
            "out_time_us" => self.out_time_us = value.parse().ok().filter(|v: &i64| *v >= 0),
            // Historically mislabelled: out_time_ms is also microseconds.
            "out_time_ms" if self.out_time_us.is_none() => {
                self.out_time_fallback = value.parse().ok().filter(|v: &i64| *v >= 0)
            }
            "out_time" if self.out_time_us.is_none() && self.out_time_fallback.is_none() => {
                self.out_time_fallback = parse_clock_us(value)
            }
            "speed" => {
                self.speed = value
                    .strip_suffix('x')
                    .and_then(|v| v.trim().parse::<f32>().ok())
                    .filter(|v| v.is_finite() && *v > 0.0)
            }
            "progress" => {
                let block = ProgressBlock {
                    out_time_us: self.out_time_us.or(self.out_time_fallback),
                    speed: self.speed,
                    done: value == "end",
                };
                *self = ProgressParser::default();
                return Some(block);
            }
            _ => {}
        }
        None
    }
}

/// `HH:MM:SS.micro` → µs.
fn parse_clock_us(v: &str) -> Option<i64> {
    let mut parts = v.split(':');
    let h: i64 = parts.next()?.parse().ok()?;
    let m: i64 = parts.next()?.parse().ok()?;
    let s: f64 = parts.next()?.parse().ok()?;
    if h < 0 || m < 0 || s < 0.0 {
        return None;
    }
    Some((h * 3600 + m * 60) * 1_000_000 + (s * 1_000_000.0).round() as i64)
}

/// Limits event rate (≤ 10 Hz per job by default); always lets the final event through.
#[derive(Debug)]
pub struct Throttle {
    interval: Duration,
    last: Option<Instant>,
}

impl Throttle {
    pub fn new(interval: Duration) -> Self {
        Throttle { interval, last: None }
    }

    pub fn ready(&mut self, force: bool) -> bool {
        let now = Instant::now();
        if force || self.last.is_none_or(|t| now.duration_since(t) >= self.interval) {
            self.last = Some(now);
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed_all(text: &str) -> Vec<ProgressBlock> {
        let mut p = ProgressParser::default();
        text.lines().filter_map(|l| p.feed(l)).collect()
    }

    #[test]
    fn parses_blocks() {
        let blocks = feed_all(
            "frame=10\nfps=0.0\nout_time_us=400000\nout_time_ms=400000\nout_time=00:00:00.400000\n\
             speed=31.5x\nprogress=continue\n\
             frame=125\nout_time_us=5000000\nspeed=N/A\nprogress=end\n",
        );
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].out_time_us, Some(400_000));
        assert_eq!(blocks[0].speed, Some(31.5));
        assert!(!blocks[0].done);
        assert_eq!(blocks[1].out_time_us, Some(5_000_000));
        assert_eq!(blocks[1].speed, None);
        assert!(blocks[1].done);
    }

    #[test]
    fn handles_na_and_fallbacks() {
        let blocks = feed_all("out_time_us=N/A\nout_time=00:01:02.500000\nprogress=continue\n");
        assert_eq!(blocks[0].out_time_us, Some(62_500_000));
        let blocks = feed_all("out_time_ms=1500000\nprogress=continue\n");
        assert_eq!(blocks[0].out_time_us, Some(1_500_000));
        let blocks = feed_all("out_time_us=-9223372036854775807\nprogress=continue\n");
        assert_eq!(blocks[0].out_time_us, None);
        let blocks = feed_all("garbage\n=\nprogress=continue\n");
        assert_eq!(blocks.len(), 1);
    }

    #[test]
    fn throttle_limits_rate() {
        let mut t = Throttle::new(Duration::from_secs(60));
        assert!(t.ready(false));
        assert!(!t.ready(false));
        assert!(t.ready(true));
    }
}
