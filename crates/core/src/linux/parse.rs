//! Pure `/proc` parsers.
//!
//! Every function here takes `&[u8]` rather than a path so that the whole parser
//! layer is testable against committed fixtures on any OS
//! (`docs/DESIGN.md` §10).

use crate::row::{CommBuf, Row, RowFlags};
use crate::system::{CpuTimes, DiskCounters, MemInfo, NetCounters, SwapInfo};

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
    mem.finish_linux();
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

/// Parse `/proc/net/dev` into cumulative host counters.
///
/// The file has two header lines, then one line per interface:
///
/// ```text
///  face |bytes    packets errs drop fifo frame compressed multicast|bytes ...
///     lo: 1234 12 0 0 0 0 0 0 1234 12 0 0 0 0 0 0
///   eth0: 5678 34 0 0 0 0 0 0 5678 34 0 0 0 0 0 0
/// ```
///
/// Note that the interface name and the first counter share a colon-separated
/// field, so the split has to happen before the whitespace tokenisation, not
/// after it. Loopback is skipped: see [`crate::system::NetCounters`].
pub fn parse_net_dev(buf: &[u8]) -> NetCounters {
    let mut out = NetCounters::default();
    for line in buf.split(|b| *b == b'\n') {
        // The two header lines contain no colon followed by digits.
        let Some(colon) = memchr::memchr(b':', line) else {
            continue;
        };
        let name = trim_ascii(&line[..colon]);
        if name.is_empty() {
            continue;
        }
        // Loopback never reaches a wire, and counting it would double every
        // local byte (once received, once transmitted).
        if name == b"lo" {
            continue;
        }
        let mut it = line[colon + 1..]
            .split(|b| b.is_ascii_whitespace())
            .filter(|t| !t.is_empty());
        // Fields, in order: rx bytes, packets, errs, drop, fifo, frame,
        // compressed, multicast, then the same eight for tx. Only bytes matter,
        // so skip the seven rx counters that follow rx bytes.
        let Some(rx) = it.next().and_then(parse_u64) else {
            continue;
        };
        let tx = it.nth(7).and_then(parse_u64).unwrap_or(0);
        out.rx_bytes = out.rx_bytes.saturating_add(rx);
        out.tx_bytes = out.tx_bytes.saturating_add(tx);
    }
    out
}

/// Parse `/proc/diskstats` into cumulative host counters.
///
/// Fields 3 and 6 (1-based) are sectors read and written; a sector is always
/// 512 bytes on Linux regardless of the device's physical sector size. Only the
/// whole-disk entries are counted: a partition's counters are already included
/// in its parent disk's, so summing both would double every byte.
///
/// A device counts as a whole disk when `/sys/block/<name>` exists. Without
/// sysfs (a fixture tree, a container with a masked `/sys`) fall back to a
/// name heuristic, which is what htop's `diskio` does too.
pub fn parse_diskstats(buf: &[u8], is_whole_disk: impl Fn(&[u8]) -> bool) -> DiskCounters {
    const SECTOR: u64 = 512;
    let mut out = DiskCounters::default();
    for line in buf.split(|b| *b == b'\n') {
        let mut it = line
            .split(|b| b.is_ascii_whitespace())
            .filter(|t| !t.is_empty());
        // major, minor, name, then 11+ counters.
        let (_major, _minor) = (it.next(), it.next());
        let Some(name) = it.next() else { continue };
        if !is_whole_disk(name) {
            continue;
        }
        // reads completed, reads merged, sectors read, ms reading,
        // writes completed, writes merged, sectors written
        let Some(read_sectors) = it.nth(2).and_then(parse_u64) else {
            continue;
        };
        // `nth` returns the element it lands on, so from field 3 this skips
        // fields 4 and 5 and lands on 6 — one short. Sectors *written* is field
        // 7, so three steps, not two.
        let write_sectors = it.nth(3).and_then(parse_u64).unwrap_or(0);
        out.read_bytes = out
            .read_bytes
            .saturating_add(read_sectors.saturating_mul(SECTOR));
        out.write_bytes = out
            .write_bytes
            .saturating_add(write_sectors.saturating_mul(SECTOR));
    }
    out
}

/// Whether a `/proc/diskstats` device name looks like a whole disk.
///
/// The authoritative answer is "does `/sys/block/<name>` exist"; the scan layer
/// checks that and only falls back here when sysfs is unavailable (a fixture
/// tree, or a container with a masked `/sys`).
///
/// Partitions must be excluded, not filtered later: a partition's counters are
/// already included in its parent disk's, so counting both doubles every byte.
/// That is also why `dm-*` is excluded — a device-mapper device wraps the same
/// I/O as the disk below it.
pub fn looks_like_whole_disk(name: &[u8]) -> bool {
    // Pseudo-devices: their I/O is either not disk I/O or already accounted for
    // by something else. `zram` in particular would double-count swap.
    for prefix in [
        b"loop".as_slice(),
        b"ram".as_slice(),
        b"zram".as_slice(),
        b"dm-".as_slice(),
        b"sr".as_slice(),
        b"nbd".as_slice(),
        b"fd".as_slice(),
    ] {
        if name.starts_with(prefix) {
            return false;
        }
    }

    // `nvme0n1` is a whole disk, `nvme0n1p1` a partition: the `p` after the
    // namespace number is the separator. Same shape for `mmcblk0` / `mmcblk0p1`.
    for prefix in [b"nvme".as_slice(), b"mmcblk".as_slice()] {
        if let Some(rest) = name.strip_prefix(prefix) {
            return !rest.contains(&b'p');
        }
    }

    // Everything else: `sda` / `vda` / `hda` are disks, a trailing digit means a
    // partition (`sda1`).
    !name.last().is_some_and(u8::is_ascii_digit)
}

/// Trim ASCII whitespace from both ends of a byte slice.
fn trim_ascii(mut b: &[u8]) -> &[u8] {
    while let Some((first, rest)) = b.split_first() {
        if first.is_ascii_whitespace() {
            b = rest;
        } else {
            break;
        }
    }
    while let Some((last, rest)) = b.split_last() {
        if last.is_ascii_whitespace() {
            b = rest;
        } else {
            break;
        }
    }
    b
}

fn parse_u64(b: &[u8]) -> Option<u64> {
    if b.is_empty() {
        return None;
    }
    let mut v: u64 = 0;
    for &c in b {
        if !c.is_ascii_digit() {
            return None;
        }
        v = v.wrapping_mul(10).wrapping_add((c - b'0') as u64);
    }
    Some(v)
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
    fn meminfo_matches_free_accounting() {
        let mut mem = MemInfo::default();
        let mut swap = SwapInfo::default();
        parse_meminfo(MEMINFO, &mut mem, &mut swap);
        assert_eq!(mem.total, 16_000_000 * 1024);
        assert_eq!(swap.used, 500_000 * 1024);

        // used = total - free - cached - sreclaimable - buffers
        //      = 16e6 - 1e6 - 2e6 - 5e5 - 1e5 = 12.4e6 kB
        assert_eq!(mem.used, 12_400_000 * 1024);
        // MemAvailable is reported verbatim; making it complement `used` would
        // clamp it to 3.6e6 kB and understate reclaimable cache.
        assert_eq!(mem.available, 8_000_000 * 1024);
        assert!(mem.is_consistent(), "{mem:?}");
        // used + available exceeds total on Linux; that overlap is the
        // reclaimable cache counted by both.
        assert!(!mem.is_exactly_partitioned());
        assert!(mem.used + mem.available > mem.total);
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

    /// A real `/proc/net/dev` with the two header lines intact, because the
    /// header is where the colon-splitting trap lives: the first data field is
    /// glued to the interface name (`eth0:1234`).
    const NET_DEV: &[u8] = b"Inter-|   Receive                                                |  Transmit\n face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs drop fifo colls carrier compressed\n    lo:     100       1    0    0    0     0          0         0      100       1    0    0    0     0       0          0\n  eth0:    5678      34    0    0    0     0          0         0     9101      56    0    0    0     0       0          0\n wlan0:    1000       9    0    0    0     0          0         0     2000      10    0    0    0     0       0          0\n";

    #[test]
    fn net_dev_sums_non_loopback_interfaces() {
        let n = parse_net_dev(NET_DEV);
        assert_eq!(n.rx_bytes, 5678 + 1000);
        assert_eq!(n.tx_bytes, 9101 + 2000);
    }

    #[test]
    fn net_dev_without_loopback_still_works() {
        let buf = b"h1\nh2\n  eth0: 10 1 0 0 0 0 0 0 20 1 0 0 0 0 0 0\n";
        let n = parse_net_dev(buf);
        assert_eq!(n.rx_bytes, 10);
        assert_eq!(n.tx_bytes, 20);
    }

    #[test]
    fn net_dev_garbage_is_zero_not_a_panic() {
        assert_eq!(parse_net_dev(b""), NetCounters::default());
        assert_eq!(parse_net_dev(b"no colons here\n"), NetCounters::default());
    }

    /// Real `/proc/diskstats` lines: a whole disk, one of its partitions, and a
    /// loop device. Only the disk may be counted.
    const DISKSTATS: &[u8] = concat!(
        "   8       0 sda 1000 0 8000 100 500 0 4000 50 0 100 150\n",
        "   8       1 sda1 900 0 7000 90 400 0 3000 40 0 90 130\n",
        " 259       0 nvme0n1 200 0 1600 20 100 0 800 10 0 20 30\n",
        " 259       1 nvme0n1p1 150 0 1200 15 80 0 600 8 0 15 22\n",
        "   7       0 loop0 5 0 40 1 0 0 0 0 0 1 1\n",
    )
    .as_bytes();

    #[test]
    fn diskstats_counts_whole_disks_only() {
        let d = parse_diskstats(DISKSTATS, looks_like_whole_disk);
        // sda: 8000 sectors read, 4000 written. nvme0n1: 1600 / 800.
        assert_eq!(d.read_bytes, (8000 + 1600) * 512);
        assert_eq!(d.write_bytes, (4000 + 800) * 512);
    }

    #[test]
    fn diskstats_write_field_is_seven_not_six() {
        // A single disk where read and write sectors are deliberately different:
        // picking field 6 instead of 7 would return 2222 here.
        let buf = b"   8       0 sda 1 0 1111 0 1 0 2222 0 0 0 0\n";
        let d = parse_diskstats(buf, |_| true);
        assert_eq!(d.read_bytes, 1111 * 512);
        assert_eq!(d.write_bytes, 2222 * 512, "sectors written is field 7");
    }

    #[test]
    fn whole_disk_names() {
        for disk in ["sda", "vda", "hdb", "nvme0n1", "mmcblk0", "xvda"] {
            assert!(looks_like_whole_disk(disk.as_bytes()), "{disk} is a disk");
        }
        for part in ["sda1", "vda12", "nvme0n1p1", "mmcblk0p2"] {
            assert!(!looks_like_whole_disk(part.as_bytes()), "{part} is a partition");
        }
        for pseudo in ["loop0", "ram0", "zram0", "dm-0", "sr0"] {
            assert!(!looks_like_whole_disk(pseudo.as_bytes()), "{pseudo} is skipped");
        }
    }

    #[test]
    fn counters_never_report_a_negative_rate() {
        // An interface recreated between samples: the counter goes backwards.
        let before = NetCounters {
            rx_bytes: 1_000_000,
            tx_bytes: 500_000,
        };
        let after = NetCounters {
            rx_bytes: 100,
            tx_bytes: 50,
        };
        assert_eq!(after.delta(&before), NetCounters::default());
        assert_eq!(after.per_sec(&before, 1.0).total(), 0.0);

        // A zero or negative interval is the first sample, not a divide by zero.
        let rate = after.per_sec(&NetCounters::default(), 0.0);
        assert_eq!(rate.total(), 0.0);
        assert!(rate.inbound.is_finite());
    }
}
