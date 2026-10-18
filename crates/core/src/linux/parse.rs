//! Pure `/proc` parsers.
//!
//! Every function here takes `&[u8]` rather than a path so that the whole parser
//! layer is testable against committed fixtures on any OS
//! (`docs/DESIGN.md` §10).

use crate::row::{CommBuf, Row, RowFlags};
use crate::system::{CpuTimes, MemInfo, SwapInfo};

/// Parse `/proc/<pid>/stat`.
///
/// Returns `None` when the buffer is malformed. Note the trap: `comm` may
/// contain spaces, `(` and `)`, so we anchor on the **last** `)`.
pub fn parse_stat(buf: &[u8]) -> Option<(u32, CommBuf, StatFields)> {
    let l = memchr::memchr(b'(', buf)?;
    let r = memchr::memrchr(b')', buf)?;
    if r <= l {
        return None;
    }

    let pid_part = &buf[..l];
    let pid: u32 = std::str::from_utf8(pid_part).ok()?.trim().parse().ok()?;

    let mut comm = CommBuf::new();
    // comm is arbitrary bytes in principle; lossy-convert rather than fail.
    let comm_raw = String::from_utf8_lossy(&buf[l + 1..r]);
    comm.try_push_str(&comm_raw).ok()?;

    let rest = buf.get(r + 1..)?;
    let mut it = rest
        .split(|b| *b == b' ' || *b == b'\n')
        .filter(|t| !t.is_empty());

    let mut f: [&[u8]; 22] = [b""; 22];
    for slot in f.iter_mut() {
        *slot = it.next()?;
    }

    let num = |s: &[u8]| -> Option<u64> {
        // Fast path for unsigned decimal.
        let mut v: u64 = 0;
        for &c in s {
            if !c.is_ascii_digit() {
                return None;
            }
            v = v.wrapping_mul(10).wrapping_add((c - b'0') as u64);
        }
        Some(v)
    };
    let signed = |s: &[u8]| -> Option<i64> {
        let (neg, s) = match s.first() {
            Some(b'-') => (true, &s[1..]),
            Some(b'+') => (false, &s[1..]),
            _ => (false, s),
        };
        let v = num(s)? as i64;
        Some(if neg { -v } else { v })
    };

    let fields = StatFields {
        state: *f[0].first()?,
        ppid: num(f[1])? as u32,
        pgrp: num(f[2])? as u32,
        session: num(f[3])? as u32,
        tty_nr: signed(f[4])?,
        min_flt: num(f[7])?,
        maj_flt: num(f[9])?,
        utime: num(f[11])?,
        stime: num(f[12])?,
        priority: signed(f[15])? as i8,
        nice: signed(f[16])? as i8,
        nlwp: num(f[17])? as i32,
        starttime: num(f[19])?,
        vsize: num(f[20])?,
        rss_pages: num(f[21])?,
    };

    Some((pid, comm, fields))
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct StatFields {
    pub state: u8,
    pub ppid: u32,
    pub pgrp: u32,
    pub session: u32,
    pub tty_nr: i64,
    pub min_flt: u64,
    pub maj_flt: u64,
    pub utime: u64,
    pub stime: u64,
    pub priority: i8,
    pub nice: i8,
    pub nlwp: i32,
    pub starttime: u64,
    pub vsize: u64,
    pub rss_pages: u64,
}

impl StatFields {
    pub fn cpu_ticks(&self) -> u64 {
        self.utime.saturating_add(self.stime)
    }
}

/// Apply parsed stat fields to a row, converting pages to bytes.
pub fn fill_row(row: &mut Row, st: &StatFields, page_size: u64) {
    row.state = st.state;
    row.ppid = st.ppid;
    row.pgrp = st.pgrp;
    row.session = st.session;
    row.tty_nr = st.tty_nr;
    row.min_flt = st.min_flt;
    row.maj_flt = st.maj_flt;
    row.priority = st.priority;
    row.nice = st.nice;
    row.nlwp = st.nlwp;
    row.starttime = st.starttime;
    row.virt = st.vsize;
    row.res = st.rss_pages.saturating_mul(page_size);
    row.time_ticks = st.cpu_ticks();
}

/// Parse `/proc/<pid>/status` for the effective UID and thread count.
///
/// `status` is expensive relative to `stat`; this is only called when the
/// columns that need it are active, or when a row is new.
pub fn parse_status_uid(buf: &[u8]) -> Option<u32> {
    for line in buf.split(|b| *b == b'\n') {
        if let Some(rest) = line.strip_prefix(b"Uid:") {
            let mut it = rest.split(|b| *b == b'\t' || *b == b' ');
            it.find(|t| !t.is_empty())?; // real uid
            let effective = it.find(|t| !t.is_empty())?;
            return std::str::from_utf8(effective).ok()?.trim().parse().ok();
        }
    }
    None
}

/// Parse `/proc/meminfo` into `MemInfo`/`SwapInfo` (values in bytes).
pub fn parse_meminfo(buf: &[u8], mem: &mut MemInfo, swap: &mut SwapInfo) {
    for line in buf.split(|b| *b == b'\n') {
        let Some(colon) = memchr::memchr(b':', line) else {
            continue;
        };
        let key = &line[..colon];
        let rest = &line[colon + 1..];
        let Some(val) = first_number(rest) else {
            continue;
        };
        // /proc/meminfo is in kB.
        let bytes = val.saturating_mul(1024);
        match key {
            b"MemTotal" => mem.total = bytes,
            b"MemFree" => mem.free = bytes,
            b"MemAvailable" => mem.available = bytes,
            b"Buffers" => mem.buffers = bytes,
            b"Cached" => mem.cached = bytes,
            b"SReclaimable" => mem.sreclaimable = bytes,
            b"Shmem" => mem.shmem = bytes,
            b"SwapTotal" => swap.total = bytes,
            b"SwapFree" => swap.free = bytes,
            _ => {}
        }
    }
    mem.compute_used();
    swap.used = swap.total.saturating_sub(swap.free);
}

fn first_number(buf: &[u8]) -> Option<u64> {
    let start = buf.iter().position(|c| c.is_ascii_digit())?;
    let mut v: u64 = 0;
    for &c in &buf[start..] {
        if !c.is_ascii_digit() {
            break;
        }
        v = v.wrapping_mul(10).wrapping_add((c - b'0') as u64);
    }
    Some(v)
}

/// Parse `/proc/stat` into `[aggregate, cpu0, cpu1, ...]`.
pub fn parse_proc_stat(buf: &[u8]) -> Vec<CpuTimes> {
    let mut out = Vec::new();
    for line in buf.split(|b| *b == b'\n') {
        let Some(rest) = line.strip_prefix(b"cpu") else {
            continue;
        };
        // The aggregate line is "cpu  100 ...": the digits after `cpu` are the
        // first value. A per-core line is "cpu0 50 ...", where the leading `0`
        // is the core index and must be skipped.
        let rest = match rest.first() {
            Some(c) if c.is_ascii_digit() => match memchr::memchr(b' ', rest) {
                Some(sp) => &rest[sp..],
                None => continue,
            },
            _ => rest,
        };

        let mut vals = [0u64; 10];
        let mut n = 0;
        for tok in rest.split(|b| *b == b' ').filter(|t| !t.is_empty()) {
            if n == vals.len() {
                break;
            }
            vals[n] = first_number(tok).unwrap_or(0);
            n += 1;
        }
        if n == 0 {
            continue;
        }
        out.push(CpuTimes {
            user: vals[0],
            nice: vals[1],
            system: vals[2],
            idle: vals[3],
            iowait: vals[4],
            irq: vals[5],
            softirq: vals[6],
            steal: vals[7],
            guest: vals[8],
            guest_nice: vals[9],
        });
    }
    out
}

/// Parse `/proc/loadavg` : `0.52 0.58 0.59 1/1234 5678`.
pub fn parse_loadavg(buf: &[u8]) -> [f64; 3] {
    let s = String::from_utf8_lossy(buf);
    let mut it = s.split_whitespace();
    let a = it.next().and_then(|v| v.parse().ok()).unwrap_or(0.0);
    let b = it.next().and_then(|v| v.parse().ok()).unwrap_or(0.0);
    let c = it.next().and_then(|v| v.parse().ok()).unwrap_or(0.0);
    [a, b, c]
}

/// Classify a row as a kernel thread or a userland thread.
///
/// Kernel threads have an empty `/proc/<pid>/cmdline` and no `exe`; threads have
/// a `Tgid != Pid`.
pub fn classify(row: &mut Row, tgid: u32, cmdline_empty: bool, has_exe: bool) {
    if tgid != row.pid {
        row.flags = row.flags.insert(RowFlags::USERLAND_THREAD);
    } else if cmdline_empty && !has_exe {
        row.flags = row.flags.insert(RowFlags::KERNEL_THREAD);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STAT_TRICKY: &[u8] =
        b"1234 (my ) we(ird comm) S 1 1234 1234 0 -1 4194560 100 0 5 0 12 34 0 0 20 0 3 0 987654 1048576 256 18446744073709551615\n";

    #[test]
    fn stat_handles_parens_in_comm() {
        let (pid, comm, f) = parse_stat(STAT_TRICKY).expect("must parse");
        assert_eq!(pid, 1234);
        assert_eq!(&*comm, "my ) we(ird comm");
        assert_eq!(f.state, b'S');
        assert_eq!(f.ppid, 1);
        assert_eq!(f.min_flt, 100);
        assert_eq!(f.maj_flt, 5);
        assert_eq!(f.utime, 12);
        assert_eq!(f.stime, 34);
        assert_eq!(f.cpu_ticks(), 46);
        assert_eq!(f.nlwp, 3);
        assert_eq!(f.starttime, 987_654);
        assert_eq!(f.vsize, 1_048_576);
        assert_eq!(f.rss_pages, 256);
    }

    #[test]
    fn stat_rejects_truncated() {
        assert!(parse_stat(b"1234 (bash) S 1 2").is_none());
        assert!(parse_stat(b"garbage").is_none());
    }

    const MEMINFO: &[u8] = b"MemTotal:       16000000 kB\nMemFree:         1000000 kB\nMemAvailable:    8000000 kB\nBuffers:          100000 kB\nCached:          2000000 kB\nSReclaimable:     500000 kB\nShmem:            200000 kB\nSwapTotal:       2000000 kB\nSwapFree:        1500000 kB\n";

    #[test]
    fn meminfo_parsing_and_used() {
        let mut mem = MemInfo::default();
        let mut swap = SwapInfo::default();
        parse_meminfo(MEMINFO, &mut mem, &mut swap);
        assert_eq!(mem.total, 16_000_000 * 1024);
        assert_eq!(swap.used, 500_000 * 1024);
        // 16e6 - 8e6 - (5e5 - 2e5) = 7.7e6 kB
        assert_eq!(mem.used, 7_700_000 * 1024);
    }

    const PROC_STAT: &[u8] = b"cpu  100 10 50 1000 20 5 5 0 0 0\ncpu0 50 5 25 500 10 2 3 0 0 0\ncpu1 50 5 25 500 10 3 2 0 0 0\nintr 12345 0 0\nctxt 999\n";

    #[test]
    fn proc_stat_parsing() {
        let v = parse_proc_stat(PROC_STAT);
        assert_eq!(v.len(), 3, "aggregate plus two cores");
        assert_eq!(v[0].user, 100);
        assert_eq!(v[1].softirq, 3);
        assert_eq!(v[2].irq, 3);
        assert_eq!(v[0].total(), 100 + 10 + 50 + 1000 + 20 + 5 + 5);
    }

    #[test]
    fn loadavg_parsing() {
        assert_eq!(
            parse_loadavg(b"0.52 0.58 0.59 1/1234 5678\n"),
            [0.52, 0.58, 0.59]
        );
    }

    #[test]
    fn status_uid() {
        let buf = b"Name:\tbash\nUid:\t1000\t1000\t1000\t1000\nGid:\t1000\t1000\t1000\t1000\n";
        assert_eq!(parse_status_uid(buf), Some(1000));
    }
}
