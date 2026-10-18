//! Filesystem helpers that are not on the per-tick hot path.
//!
//! `smaps_rollup` is expensive (the kernel walks every VMA), so it is fetched
//! only when a PSS/EPSS column is visible, and always through this module so the
//! call site stays obvious.

use std::path::Path;

/// Memory figures from `/proc/<pid>/smaps_rollup`, in bytes.
#[derive(Copy, Clone, Default, Debug, PartialEq, Eq)]
pub struct SmapsRollup {
    pub rss: u64,
    pub pss: u64,
    pub pss_anon: u64,
    pub pss_file: u64,
    pub pss_shmem: u64,
    pub private_clean: u64,
    pub private_dirty: u64,
    pub swap: u64,
    pub swap_pss: u64,
}

impl SmapsRollup {
    /// Effective PSS: the sum of memory and swap PSS, as EPSS displays.
    pub fn epss(&self) -> u64 {
        self.pss.saturating_add(self.swap_pss)
    }

    /// Memory that is exclusively this process's: PSS minus the shared parts.
    pub fn private(&self) -> u64 {
        self.private_clean.saturating_add(self.private_dirty)
    }
}

/// Parse `/proc/<pid>/smaps_rollup`.
pub fn parse_smaps_rollup(buf: &[u8]) -> Option<SmapsRollup> {
    let mut out = SmapsRollup::default();
    let mut seen = false;

    for line in buf.split(|b| *b == b'\n') {
        let Some(colon) = memchr::memchr(b':', line) else {
            continue;
        };
        let key = &line[..colon];
        let rest = &line[colon + 1..];
        let Some(kb) = first_number(rest) else {
            continue;
        };
        let bytes = kb.saturating_mul(1024);

        match key {
            b"Rss" => out.rss = bytes,
            b"Pss" => out.pss = bytes,
            b"Pss_Anon" => out.pss_anon = bytes,
            b"Pss_File" => out.pss_file = bytes,
            b"Pss_Shmem" => out.pss_shmem = bytes,
            b"Private_Clean" => out.private_clean = bytes,
            b"Private_Dirty" => out.private_dirty = bytes,
            b"Swap" => out.swap = bytes,
            b"SwapPss" => out.swap_pss = bytes,
            _ => continue,
        }
        seen = true;
    }

    if seen {
        Some(out)
    } else {
        None
    }
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

/// A cgroup membership entry from `/proc/<pid>/cgroup`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CgroupEntry {
    pub hierarchy: u32,
    pub controllers: String,
    pub path: String,
}

/// Parse `/proc/<pid>/cgroup`.
///
/// Lines are `hierarchy-ID:controller-list:path`. On a unified (v2) hierarchy the
/// controller list is empty.
pub fn parse_cgroup(buf: &[u8]) -> Vec<CgroupEntry> {
    let mut out = Vec::new();
    for line in buf.split(|b| *b == b'\n') {
        let line = trim(line);
        if line.is_empty() {
            continue;
        }
        let mut parts = line.splitn(3, |b| *b == b':');
        let (Some(hier), Some(ctrls), Some(path)) = (parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        let hierarchy = std::str::from_utf8(hier)
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        out.push(CgroupEntry {
            hierarchy,
            controllers: String::from_utf8_lossy(ctrls).into_owned(),
            path: String::from_utf8_lossy(path).into_owned(),
        });
    }
    out
}

/// Best-effort container detection.
///
/// htop uses cgroup membership rather than heuristics; so do we. A process is
/// considered containerised when its cgroup path contains a well-known runtime
/// marker segment.
pub fn container_from_cgroup(entries: &[CgroupEntry]) -> Option<&str> {
    const MARKERS: [&str; 6] = [
        "docker",
        "kubepods",
        "lxc",
        "podman",
        "containerd",
        "libpod",
    ];
    for entry in entries {
        for marker in MARKERS {
            if entry.path.contains(marker) {
                return Some(marker);
            }
        }
    }
    None
}

fn trim(mut b: &[u8]) -> &[u8] {
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

/// Whether `/proc/<pid>/smaps_rollup` exists for this pid.
pub fn smaps_rollup_available(proc_root: &Path, pid: crate::Pid) -> bool {
    proc_root
        .join(pid.to_string())
        .join("smaps_rollup")
        .exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SMAPS: &[u8] = b"55d0e0000000-7ffe00000000 ---p 00000000 00:00 0                  [rollup]\nRss:               4800 kB\nPss:               2400 kB\nPss_Anon:          1600 kB\nPss_File:           500 kB\nPss_Shmem:          300 kB\nPrivate_Clean:      400 kB\nPrivate_Dirty:      900 kB\nSwap:               100 kB\nSwapPss:             50 kB\n";

    #[test]
    fn smaps_rollup_parsing() {
        let r = parse_smaps_rollup(SMAPS).expect("must parse");
        assert_eq!(r.rss, 4800 * 1024);
        assert_eq!(r.pss, 2400 * 1024);
        assert_eq!(r.swap_pss, 50 * 1024);
        assert_eq!(r.epss(), (2400 + 50) * 1024);
        assert_eq!(r.private(), (400 + 900) * 1024);
    }

    #[test]
    fn smaps_rollup_rejects_garbage() {
        assert!(parse_smaps_rollup(b"hello world\n").is_none());
    }

    #[test]
    fn cgroup_v2() {
        let e = parse_cgroup(b"0::/user.slice/user-1000.slice/session-1.scope\n");
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].hierarchy, 0);
        assert!(e[0].controllers.is_empty());
        assert_eq!(e[0].path, "/user.slice/user-1000.slice/session-1.scope");
    }

    #[test]
    fn cgroup_v1_multi() {
        let buf =
            b"12:cpuset:/docker/abc123\n5:memory:/docker/abc123\n1:name=systemd:/user.slice/x\n";
        let e = parse_cgroup(buf);
        assert_eq!(e.len(), 3);
        assert_eq!(container_from_cgroup(&e), Some("docker"));
    }

    #[test]
    fn no_container_for_plain_host_process() {
        let e = parse_cgroup(b"0::/user.slice/session-2.scope\n");
        assert_eq!(container_from_cgroup(&e), None);
    }
}
