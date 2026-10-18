//! Human-readable formatting.
//!
//! Called from the renderer only. Nothing here is on the per-process scan path:
//! `docs/DESIGN.md` §9.4 — a `format!` inside a scan is a bug.

use std::fmt::Write as _;

use arrayvec::ArrayString;

/// Format a byte count using binary IEC units, matching `htop.1` MEMORY SIZES.
pub fn bytes_human(bytes: u64) -> ArrayString<16> {
    const UNITS: [&str; 6] = ["B", "K", "M", "G", "T", "P"];
    let mut out = ArrayString::new();
    if bytes < 1024 {
        let _ = write!(out, "{bytes}B");
        return out;
    }
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    // htop prints one decimal below 10 units, none above.
    let _ = if value < 10.0 {
        write!(out, "{value:.1}{}", UNITS[unit])
    } else {
        write!(out, "{value:.0}{}", UNITS[unit])
    };
    out
}

/// Format a duration with explicit units, for places with room to spell it out.
///
/// htop prints uptime as `4-23:13:30`, which requires the reader to remember
/// that the leading field is days. The meter has space, so it says so:
///
/// | input | output |
/// |---|---|
/// | 45 s | `45s` |
/// | 13 min 30 s | `13m 30s` |
/// | 23 h 13 min | `23h 13m` |
/// | 4 d 23 h | `4d 23h` |
/// | 400 d | `400d` |
///
/// The largest two fields are kept; seconds are dropped once days appear, because
/// nobody reading a process viewer needs second-resolution uptime measured in
/// months.
pub fn duration_verbose(secs: u64) -> ArrayString<24> {
    let mut out = ArrayString::new();
    let d = secs / 86_400;
    let h = (secs % 86_400) / 3_600;
    let m = (secs % 3_600) / 60;
    let s = secs % 60;

    let _ = if d > 0 {
        if h > 0 {
            write!(out, "{d}d {h}h")
        } else {
            write!(out, "{d}d")
        }
    } else if h > 0 {
        if m > 0 {
            write!(out, "{h}h {m}m")
        } else {
            write!(out, "{h}h")
        }
    } else if m > 0 {
        if s > 0 {
            write!(out, "{m}m {s}s")
        } else {
            write!(out, "{m}m")
        }
    } else {
        write!(out, "{s}s")
    };
    out
}

/// Format a duration as `MM:SS` or `H:MM:SS` / `D-HH:MM:SS` (htop's TIME+ style).
///
/// Used for table cells, where a compact, fixed-width value matters more than
/// immediate legibility. Prefer [`duration_verbose`] when there is room.
pub fn duration_human(secs: u64) -> ArrayString<16> {
    let mut out = ArrayString::new();
    let d = secs / 86_400;
    let h = (secs % 86_400) / 3_600;
    let m = (secs % 3_600) / 60;
    let s = secs % 60;
    let _ = if d > 0 {
        write!(out, "{d}-{h:02}:{m:02}:{s:02}")
    } else if h > 0 {
        write!(out, "{h}:{m:02}:{s:02}")
    } else {
        write!(out, "{m:02}:{s:02}")
    };
    out
}

/// Format CPU ticks as a `MM:SS.cc` figure, as the TIME column shows.
pub fn cpu_time_human(ticks: u64, clk_tck: u64) -> ArrayString<16> {
    let mut out = ArrayString::new();
    if clk_tck == 0 {
        out.push('-');
        return out;
    }
    let centis = ticks * 100 / clk_tck;
    let m = centis / 6_000;
    let s = (centis % 6_000) / 100;
    let c = centis % 100;
    let _ = write!(out, "{m:02}:{s:02}.{c:02}");
    out
}

/// Format a percentage with one decimal, as a fixed-width cell.
pub fn percent_human(p: f32) -> ArrayString<8> {
    let mut out = ArrayString::new();
    if p.is_nan() {
        out.push('-');
        return out;
    }
    let _ = write!(out, "{p:.1}");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_scaling() {
        assert_eq!(&*bytes_human(0), "0B");
        assert_eq!(&*bytes_human(512), "512B");
        assert_eq!(&*bytes_human(1024), "1.0K");
        assert_eq!(&*bytes_human(1536), "1.5K");
        assert_eq!(&*bytes_human(1024 * 1024 * 200), "200M");
    }

    #[test]
    fn durations() {
        assert_eq!(&*duration_human(0), "00:00");
        assert_eq!(&*duration_human(75), "01:15");
        assert_eq!(&*duration_human(3_725), "1:02:05");
        assert_eq!(&*duration_human(90_061), "1-01:01:01");
    }

    #[test]
    fn verbose_durations_have_units() {
        assert_eq!(&*duration_verbose(0), "0s");
        assert_eq!(&*duration_verbose(45), "45s");
        assert_eq!(&*duration_verbose(60), "1m");
        assert_eq!(&*duration_verbose(90), "1m 30s");
        assert_eq!(&*duration_verbose(3_600), "1h");
        assert_eq!(&*duration_verbose(3_725), "1h 2m");
        assert_eq!(&*duration_verbose(86_400), "1d");
        assert_eq!(&*duration_verbose(90_061), "1d 1h");
        assert_eq!(
            &*duration_verbose(4 * 86_400 + 23 * 3_600 + 13 * 60),
            "4d 23h"
        );
    }

    #[test]
    fn verbose_durations_fit_the_buffer() {
        // 24 chars: the longest realistic value is a machine up for ~10 years.
        let ten_years = 10 * 365 * 86_400u64;
        let text = duration_verbose(ten_years);
        assert!(!text.is_empty());
        assert!(text.len() < 24, "got {text:?}");
    }

    #[test]
    fn cpu_time() {
        // 100 ticks at 100 Hz == 1 second
        assert_eq!(&*cpu_time_human(100, 100), "00:01.00");
        assert_eq!(&*cpu_time_human(0, 0), "-");
    }
}
