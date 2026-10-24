//! Frame-based time. Positions on a sequence are whole frames at the
//! sequence frame rate; frame rates are rationals so 29.97 stays exact.

use std::fmt;

/// A position or duration in frames at the sequence frame rate.
pub type Frame = i64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Fps {
    pub num: u32,
    pub den: u32,
}

impl Fps {
    pub const FPS_24: Fps = Fps::new(24, 1);

    pub const fn new(num: u32, den: u32) -> Self {
        Self { num, den }
    }

    pub fn as_f64(self) -> f64 {
        self.num as f64 / self.den as f64
    }

    /// Whole frames per timecode second (30 for 29.97, non-drop).
    pub fn nominal(self) -> i64 {
        (self.as_f64()).round().max(1.0) as i64
    }

    pub fn frames_to_seconds(self, frames: Frame) -> f64 {
        frames as f64 * self.den as f64 / self.num as f64
    }

    pub fn seconds_to_frames(self, seconds: f64) -> Frame {
        (seconds * self.as_f64()).round() as Frame
    }
}

impl fmt::Display for Fps {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.den == 1 {
            write!(f, "{} fps", self.num)
        } else {
            write!(f, "{:.3} fps", self.as_f64())
        }
    }
}

/// `HH:MM:SS:FF` non-drop timecode. Negative frames clamp to zero.
pub fn timecode(frame: Frame, fps: Fps) -> String {
    let n = fps.nominal();
    let f = frame.max(0);
    let (ff, total_s) = (f % n, f / n);
    format!(
        "{:02}:{:02}:{:02}:{:02}",
        total_s / 3600,
        (total_s / 60) % 60,
        total_s % 60,
        ff
    )
}

/// Compact `m:ss` (plus `:ff` when not on a whole second), for rulers.
pub fn timecode_short(frame: Frame, fps: Fps) -> String {
    let n = fps.nominal();
    let f = frame.max(0);
    let s = f / n;
    let ff = f % n;
    if ff == 0 {
        format!("{}:{:02}", s / 60, s % 60)
    } else {
        format!("{}:{:02}:{:02}", s / 60, s % 60, ff)
    }
}

/// Parses `HH:MM:SS:FF`, any shorter suffix of it (`SS:FF`, `MM:SS:FF`), or
/// a bare frame count. `;` and `.` are accepted as separators.
pub fn parse_timecode(text: &str, fps: Fps) -> Option<Frame> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let parts: Vec<i64> = text
        .split([':', ';', '.'])
        .map(|p| p.trim().parse::<i64>().ok().filter(|v| *v >= 0))
        .collect::<Option<_>>()?;
    if parts.len() == 1 {
        return Some(parts[0]);
    }
    if parts.len() > 4 {
        return None;
    }
    let mut p = [0i64; 4];
    p[4 - parts.len()..].copy_from_slice(&parts);
    let [h, m, s, f] = p;
    Some(((h * 60 + m) * 60 + s) * fps.nominal() + f)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_timecode() {
        assert_eq!(timecode(0, Fps::FPS_24), "00:00:00:00");
        assert_eq!(timecode(24 * 3661 + 5, Fps::FPS_24), "01:01:01:05");
        assert_eq!(timecode(-10, Fps::FPS_24), "00:00:00:00");
        assert_eq!(timecode(30, Fps::new(30000, 1001)), "00:00:01:00");
    }

    #[test]
    fn formats_short() {
        assert_eq!(timecode_short(24 * 65, Fps::FPS_24), "1:05");
        assert_eq!(timecode_short(24 * 5 + 3, Fps::FPS_24), "0:05:03");
    }

    #[test]
    fn parses_timecode() {
        let fps = Fps::FPS_24;
        assert_eq!(parse_timecode("00:00:12:04", fps), Some(12 * 24 + 4));
        assert_eq!(parse_timecode("12:04", fps), Some(12 * 24 + 4));
        assert_eq!(parse_timecode("1:00:00", fps), Some(60 * 24));
        assert_eq!(parse_timecode("300", fps), Some(300));
        assert_eq!(parse_timecode("1;2.3", fps), Some(60 * 24 + 2 * 24 + 3));
        assert_eq!(parse_timecode("", fps), None);
        assert_eq!(parse_timecode("ab:cd", fps), None);
        assert_eq!(parse_timecode("1:2:3:4:5", fps), None);
        assert_eq!(parse_timecode("-5", fps), None);
    }
}
