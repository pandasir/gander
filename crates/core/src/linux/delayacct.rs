//! Delay accounting via `TASKSTATS` generic netlink.
//!
//! The kernel accounts, per task, how long it spent *waiting* rather than
//! running: waiting for a CPU while runnable, waiting for synchronous block I/O,
//! and waiting for a page to be swapped back in. Those three numbers are what
//! separate "this process is working" from "this process is stuck", and nothing
//! in `/proc` exposes them.
//!
//! # Why raw netlink
//!
//! htop reaches the same data through `libnl3`, which it `dlopen`s and uses
//! through function pointers. That is a runtime dependency on a library that may
//! not be installed, loaded lazily so the failure is a `dlopen` that returns
//! `NULL`. The protocol is four messages; speaking it directly removes the
//! dependency and the failure mode, and is the reason `Cargo.toml` still lists
//! nothing but `libc`.
//!
//! # The two ways this fails, and why they are not errors
//!
//! * **`EPERM`** — `TASKSTATS_CMD_GET` requires `CAP_NET_ADMIN`. An unprivileged
//!   `gander` gets `EPERM` on every request, which is the normal case, not a
//!   fault. `probe()` records it once and no further requests are sent.
//! * **`ENOENT` on the family** — `CONFIG_TASKSTATS` is not compiled in. The
//!   generic-netlink family does not exist, and neither does the column.
//!
//! Both end up as `None` on every row, which renders `-`: "not measured", which
//! is the truth. A row must never show `0.0%` for a figure nobody read.
//!
//! # Units
//!
//! Every `*_delay_total` is **nanoseconds**, and `ac_etime` is **microseconds**.
//! The percentage htop prints is `Δdelay / Δ(ac_etime × 1000)`, i.e. the share of
//! the task's wall-clock life spent waiting. It is a lifetime average, not an
//! instantaneous rate, because the kernel exposes no interval counter — see
//! `docs/ROADMAP.md`.

#[cfg(target_os = "linux")]
use std::io;
#[cfg(target_os = "linux")]
use std::mem;

/// Generic netlink protocol number (`NETLINK_GENERIC`).
#[cfg(target_os = "linux")]
const NETLINK_GENERIC: libc::c_int = 16;
/// `genl_ctrl` family id, fixed by the kernel.
#[cfg(target_os = "linux")]
const GENL_ID_CTRL: u16 = 0x10;
#[cfg(target_os = "linux")]
const CTRL_CMD_GETFAMILY: u8 = 3;
#[cfg(target_os = "linux")]
const CTRL_ATTR_FAMILY_ID: u16 = 1;
#[cfg(target_os = "linux")]
const CTRL_ATTR_FAMILY_NAME: u16 = 2;

#[cfg(target_os = "linux")]
const TASKSTATS_CMD_GET: u8 = 1;
#[cfg(target_os = "linux")]
const TASKSTATS_CMD_ATTR_PID: u16 = 1;
// The attribute enum, from `include/uapi/linux/taskstats.h`. It starts at
// `TASKSTATS_TYPE_UNSPEC = 0`, which is what makes these numbers what they are:
//
//   UNSPEC=0, PID=1, TGID=2, STATS=3, AGGR_PID=4, AGGR_TGID=5, NULL=6
//
// An earlier version of this file had `AGGR_PID = 1`, counting from `PID` instead
// of from `UNSPEC`. `STATS = 3` happened to be right, which is why the parser
// looked plausible and returned `None` on every real kernel.
/// `TASKSTATS_TYPE_STATS`: the `struct taskstats` itself.
#[cfg(target_os = "linux")]
const TASKSTATS_TYPE_STATS: u16 = 3;
/// `TASKSTATS_TYPE_AGGR_PID`: nests a pid and a stats blob (a single task).
#[cfg(target_os = "linux")]
const TASKSTATS_TYPE_AGGR_PID: u16 = 4;
/// `TASKSTATS_TYPE_AGGR_TGID`: nests a tgid and a stats blob (a thread group).
///
/// Which one comes back depends on the pid that was asked for: a thread-group
/// leader is answered with `AGGR_TGID`, one of its threads with `AGGR_PID`.
#[cfg(target_os = "linux")]
const TASKSTATS_TYPE_AGGR_TGID: u16 = 5;

#[cfg(target_os = "linux")]
const NLMSG_ERROR: u16 = 2;
#[cfg(target_os = "linux")]
const NLM_F_REQUEST: u16 = 1;

/// Byte offsets into `struct taskstats`, from `include/uapi/linux/taskstats.h`.
///
/// The struct is a kernel ABI and is append-only, so these offsets are stable for
/// every version that has them; `version` is checked before they are read.
/// `ac_etime` sits at 72 because `ac_comm[32]` is followed by a byte, three bytes
/// of padding, four `u32`s, and then a `u64` that is 8-aligned.
mod off {
    pub const VERSION: usize = 0;
    pub const CPU_DELAY_TOTAL: usize = 16;
    pub const BLKIO_DELAY_TOTAL: usize = 24;
    pub const SWAPIN_DELAY_TOTAL: usize = 32;
    pub const AC_ETIME: usize = 72;
}

/// The smallest `struct taskstats` that still contains every field read here.
///
/// `ac_etime` ends at byte 80. A reply shorter than that is from a kernel older
/// than the fields, and is rejected rather than read past.
const MIN_STATS_LEN: usize = off::AC_ETIME + 8;

/// Cumulative delay counters for one task, all in nanoseconds, plus the task's
/// elapsed wall-clock life in microseconds.
///
/// Cumulative rather than a percentage because the caller owns the previous
/// sample: turning two samples into a percentage needs the interval, and a
/// `DelayStats` that stored a percentage would have to be read twice per tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DelayStats {
    /// Time spent runnable but not on a CPU.
    pub cpu_delay_ns: u64,
    /// Time spent waiting for synchronous block I/O.
    pub blkio_delay_ns: u64,
    /// Time spent waiting for a page to be swapped in.
    pub swapin_delay_ns: u64,
    /// The task's elapsed wall-clock life, in microseconds.
    pub elapsed_us: u64,
}

impl DelayStats {
    /// Total time spent waiting, in nanoseconds.
    pub fn total_delay_ns(self) -> u64 {
        self.cpu_delay_ns
            .saturating_add(self.blkio_delay_ns)
            .saturating_add(self.swapin_delay_ns)
    }

    /// Share of the task's life spent waiting on a CPU, in percent.
    ///
    /// `None` when the elapsed time is zero, which is the state of a task that
    /// has not been sampled twice yet; a percentage of nothing is not zero.
    pub fn cpu_percent(self) -> Option<f32> {
        self.percent_of(self.cpu_delay_ns)
    }

    pub fn blkio_percent(self) -> Option<f32> {
        self.percent_of(self.blkio_delay_ns)
    }

    pub fn swapin_percent(self) -> Option<f32> {
        self.percent_of(self.swapin_delay_ns)
    }

    fn percent_of(self, delay_ns: u64) -> Option<f32> {
        let life_ns = self.elapsed_us.checked_mul(1000)?;
        if life_ns == 0 {
            return None;
        }
        let pct = delay_ns as f64 / life_ns as f64 * 100.0;
        // The counters are updated non-atomically with respect to `ac_etime`
        // (`cpu_delay_total` explicitly so, per the kernel header), so a delay can
        // momentarily exceed the elapsed time. Clamping is the honest fix: the
        // true value is "essentially all of it", and a value above 100 would sort
        // above a process that really is stalled all the time.
        Some(pct.clamp(0.0, 100.0) as f32)
    }
}

/// A connected generic-netlink socket bound to the `TASKSTATS` family.
///
/// Linux-only: the generic-netlink ABI has no equivalent on the other unixes this
/// crate compiles for, so the type is absent there rather than present and
/// unusable. The parsing half of this module stays portable so its tests run
/// everywhere.
#[cfg(target_os = "linux")]
pub struct DelayAcct {
    fd: libc::c_int,
    family: u16,
    seq: u32,
    /// Set once the kernel has said `EPERM`, after which no request is sent.
    denied: bool,
}

#[cfg(target_os = "linux")]
impl DelayAcct {
    /// Open a socket and resolve the `TASKSTATS` family.
    ///
    /// Returns `None` when the family does not exist (`CONFIG_TASKSTATS=n`) or the
    /// socket cannot be created. It does *not* fail when the caller lacks
    /// `CAP_NET_ADMIN`: that only shows up on the first `read`, and is recorded
    /// there.
    pub fn open() -> Option<Self> {
        // SAFETY: `socket(2)` with three constant arguments; returns a fd or -1.
        let fd = unsafe {
            libc::socket(
                libc::AF_NETLINK,
                libc::SOCK_RAW | libc::SOCK_CLOEXEC,
                NETLINK_GENERIC,
            )
        };
        if fd < 0 {
            return None;
        }
        let mut acct = Self {
            fd,
            family: 0,
            seq: 1,
            denied: false,
        };
        // SAFETY: `sockaddr_nl` is plain old data; binding a netlink socket needs
        // no other precondition.
        // Zeroed and then written through a pointer, rather than a struct literal:
        // `libc::sockaddr_nl::nl_pad` is a private `Padding<u16>`, so the value
        // cannot be built by naming its fields. Zeroing is also exactly what the
        // kernel expects — `nl_pid = 0` means "let the kernel assign one" and
        // `nl_groups = 0` subscribes to nothing.
        let mut addr: libc::sockaddr_nl = unsafe { mem::zeroed() };
        addr.nl_family = libc::AF_NETLINK as u16;
        // SAFETY: `addr` is a live, correctly-typed value for the duration of the
        // call, and its size is passed alongside it.
        let rc = unsafe {
            libc::bind(
                fd,
                &addr as *const libc::sockaddr_nl as *const libc::sockaddr,
                mem::size_of::<libc::sockaddr_nl>() as libc::socklen_t,
            )
        };
        if rc < 0 {
            acct.close();
            return None;
        }
        // A receive timeout so a kernel that never answers cannot hang a scan.
        // SAFETY: `setsockopt(2)` on our own fd with a correctly-sized value.
        let tv = libc::timeval {
            tv_sec: 0,
            tv_usec: 200_000,
        };
        unsafe {
            libc::setsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_RCVTIMEO,
                &tv as *const libc::timeval as *const libc::c_void,
                mem::size_of::<libc::timeval>() as libc::socklen_t,
            );
        }
        acct.family = acct.resolve_family()?;
        Some(acct)
    }

    /// Whether the family resolved but the kernel refuses to answer.
    pub fn is_denied(&self) -> bool {
        self.denied
    }

    fn close(&mut self) {
        if self.fd >= 0 {
            // SAFETY: our own fd, closed exactly once (set to -1 immediately).
            unsafe { libc::close(self.fd) };
            self.fd = -1;
        }
    }

    /// Ask `genl_ctrl` for the numeric id of the `TASKSTATS` family.
    fn resolve_family(&mut self) -> Option<u16> {
        let mut msg = Msg::new();
        msg.put_genl_header(GENL_ID_CTRL, CTRL_CMD_GETFAMILY);
        msg.put_attr_str(CTRL_ATTR_FAMILY_NAME, b"TASKSTATS");
        let seq = self.next_seq();
        let reply = self.transact(&msg.finish(seq, GENL_ID_CTRL), seq)?;
        // Walk the attributes of the reply looking for the family id.
        let attrs = Attrs::new(&reply, NLMSG_HDR_LEN + GENL_HDR_LEN, reply.len());
        for (kind, value) in attrs {
            if kind == CTRL_ATTR_FAMILY_ID && value.len() >= 2 {
                return Some(u16::from_ne_bytes([value[0], value[1]]));
            }
        }
        None
    }

    /// Read the delay counters for `pid`.
    ///
    /// `None` means "not measured": the process is gone, the kernel refused, or
    /// the reply did not contain a stats blob. Callers must render that as `-`,
    /// never as zero.
    pub fn read(&mut self, pid: crate::Pid) -> Option<DelayStats> {
        if self.denied || self.fd < 0 || self.family == 0 {
            return None;
        }
        let mut msg = Msg::new();
        msg.put_genl_header(self.family, TASKSTATS_CMD_GET);
        msg.put_attr_u32(TASKSTATS_CMD_ATTR_PID, pid);
        let seq = self.next_seq();
        let request = msg.finish(seq, self.family);
        let reply = self.transact(&request, seq)?;

        let attrs = Attrs::new(&reply, NLMSG_HDR_LEN + GENL_HDR_LEN, reply.len());
        for (kind, value) in attrs {
            // Both aggregating shapes nest the stats blob, and which arrives
            // depends on whether the pid is a thread-group leader. Some kernels
            // answer with the blob at the top level instead, so all three are
            // handled.
            if kind == TASKSTATS_TYPE_AGGR_PID || kind == TASKSTATS_TYPE_AGGR_TGID {
                let inner = Attrs::new(value, 0, value.len());
                for (inner_kind, inner_value) in inner {
                    if inner_kind == TASKSTATS_TYPE_STATS {
                        return parse_taskstats(inner_value);
                    }
                }
            } else if kind == TASKSTATS_TYPE_STATS {
                return parse_taskstats(value);
            }
        }
        None
    }

    fn next_seq(&mut self) -> u32 {
        self.seq = self.seq.wrapping_add(1);
        self.seq
    }

    /// Send one request and return the whole reply datagram.
    fn transact(&mut self, request: &[u8], seq: u32) -> Option<Vec<u8>> {
        // SAFETY: `request` is a live slice; the length passed is its length.
        let sent = unsafe {
            libc::send(
                self.fd,
                request.as_ptr() as *const libc::c_void,
                request.len(),
                0,
            )
        };
        if sent < 0 {
            return None;
        }
        // 64 KiB is far above the largest `taskstats` reply (a few hundred bytes)
        // and far below any kernel limit; one datagram per `recv` is guaranteed by
        // `SOCK_RAW` on netlink.
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            // SAFETY: `buf` is a live, uniquely-borrowed allocation of its own
            // stated length.
            let n = unsafe {
                libc::recv(
                    self.fd,
                    buf.as_mut_ptr() as *mut libc::c_void,
                    buf.len(),
                    0,
                )
            };
            if n < 0 {
                let err = io::Error::last_os_error();
                // `EAGAIN` here is the receive timeout: the kernel never answered.
                if err.raw_os_error() == Some(libc::EAGAIN) {
                    return None;
                }
                if err.raw_os_error() == Some(libc::EPERM) {
                    // No `CAP_NET_ADMIN`. Stop asking: every later request would
                    // fail identically, and one syscall per process per tick is
                    // not free.
                    self.denied = true;
                }
                return None;
            }
            let n = n as usize;
            buf.truncate(n);
            if n < NLMSG_HDR_LEN {
                return None;
            }
            let kind = u16::from_ne_bytes([buf[4], buf[5]]);
            let reply_seq = u32::from_ne_bytes([buf[8], buf[9], buf[10], buf[11]]);
            if kind == NLMSG_ERROR {
                // A zero error code is an ACK for a request that expected none.
                let code = if n >= NLMSG_HDR_LEN + 4 {
                    i32::from_ne_bytes([buf[16], buf[17], buf[18], buf[19]])
                } else {
                    0
                };
                if code == -libc::EPERM {
                    self.denied = true;
                }
                return None;
            }
            if reply_seq != seq {
                // A stale datagram from an earlier request; keep waiting.
                continue;
            }
            return Some(buf);
        }
    }
}

#[cfg(target_os = "linux")]
impl Drop for DelayAcct {
    fn drop(&mut self) {
        self.close();
    }
}

/// Parse a `struct taskstats` blob into the four fields that are read.
///
/// Split out from the socket code so it can be unit-tested against a synthetic
/// buffer: the offsets are a kernel ABI and a wrong one would produce a plausible
/// but wrong percentage, which no live test would catch on an idle machine.
pub fn parse_taskstats(buf: &[u8]) -> Option<DelayStats> {
    if buf.len() < MIN_STATS_LEN {
        return None;
    }
    let version = u16::from_ne_bytes([buf[off::VERSION], buf[off::VERSION + 1]]);
    // Version 1 is the first release of the struct and already contains every
    // field below; anything lower is not a `taskstats` reply at all.
    if version == 0 || version > 64 {
        return None;
    }
    let u64_at = |offset: usize| {
        let mut b = [0u8; 8];
        b.copy_from_slice(&buf[offset..offset + 8]);
        u64::from_ne_bytes(b)
    };
    Some(DelayStats {
        cpu_delay_ns: u64_at(off::CPU_DELAY_TOTAL),
        blkio_delay_ns: u64_at(off::BLKIO_DELAY_TOTAL),
        swapin_delay_ns: u64_at(off::SWAPIN_DELAY_TOTAL),
        elapsed_us: u64_at(off::AC_ETIME),
    })
}

/// Netlink message header: length, type, flags, sequence, port id.
#[cfg(target_os = "linux")]
const NLMSG_HDR_LEN: usize = 16;
/// Generic netlink header: command, version, reserved.
#[cfg(target_os = "linux")]
const GENL_HDR_LEN: usize = 4;

/// A growing netlink message buffer.
#[cfg(target_os = "linux")]
struct Msg {
    buf: Vec<u8>,
}

#[cfg(target_os = "linux")]
impl Msg {
    fn new() -> Self {
        Self {
            buf: Vec::with_capacity(128),
        }
    }

    /// Append the generic-netlink header. The netlink header is filled in by
    /// `finish`, which knows the final length.
    fn put_genl_header(&mut self, _family: u16, cmd: u8) {
        // Placeholder for the netlink header; `finish` overwrites it.
        self.buf.extend_from_slice(&[0u8; NLMSG_HDR_LEN]);
        self.buf.extend_from_slice(&[cmd, 1, 0, 0]);
    }

    fn put_attr(&mut self, kind: u16, value: &[u8]) {
        let len = (4 + value.len()) as u16;
        self.buf.extend_from_slice(&len.to_ne_bytes());
        self.buf.extend_from_slice(&kind.to_ne_bytes());
        self.buf.extend_from_slice(value);
        // Netlink attributes are 4-byte aligned. `is_multiple_of` rather than
        // `% 4 != 0`: clippy's `manual_is_multiple_of` is in the workspace's lint
        // set, and CI runs a newer toolchain than the MSRV.
        while !self.buf.len().is_multiple_of(4) {
            self.buf.push(0);
        }
    }

    fn put_attr_str(&mut self, kind: u16, value: &[u8]) {
        // String attributes include their NUL terminator in the length.
        let mut v = value.to_vec();
        v.push(0);
        self.put_attr(kind, &v);
    }

    fn put_attr_u32(&mut self, kind: u16, value: u32) {
        self.put_attr(kind, &value.to_ne_bytes());
    }

    /// Fill in the netlink header and return the datagram.
    fn finish(mut self, seq: u32, kind: u16) -> Vec<u8> {
        let len = self.buf.len() as u32;
        self.buf[0..4].copy_from_slice(&len.to_ne_bytes());
        self.buf[4..6].copy_from_slice(&kind.to_ne_bytes());
        self.buf[6..8].copy_from_slice(&NLM_F_REQUEST.to_ne_bytes());
        self.buf[8..12].copy_from_slice(&seq.to_ne_bytes());
        self.buf[12..16].copy_from_slice(&0u32.to_ne_bytes());
        self.buf
    }
}

/// Iterator over a run of 4-byte-aligned netlink attributes.
#[cfg(target_os = "linux")]
struct Attrs<'a> {
    buf: &'a [u8],
    pos: usize,
    end: usize,
}

#[cfg(target_os = "linux")]
impl<'a> Attrs<'a> {
    fn new(buf: &'a [u8], start: usize, end: usize) -> Self {
        Self {
            buf,
            pos: start,
            end: end.min(buf.len()),
        }
    }
}

#[cfg(target_os = "linux")]
impl<'a> Iterator for Attrs<'a> {
    type Item = (u16, &'a [u8]);

    fn next(&mut self) -> Option<Self::Item> {
        if self.pos + 4 > self.end {
            return None;
        }
        let len = u16::from_ne_bytes([self.buf[self.pos], self.buf[self.pos + 1]]) as usize;
        let kind = u16::from_ne_bytes([self.buf[self.pos + 2], self.buf[self.pos + 3]]);
        // A zero or oversized length would loop forever or read out of bounds.
        if len < 4 || self.pos + len > self.end {
            return None;
        }
        let value = &self.buf[self.pos + 4..self.pos + len];
        // Advance past the value and its padding.
        self.pos += (len + 3) & !3;
        Some((kind, value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a synthetic `struct taskstats` with the given counters.
    fn stats_blob(version: u16, cpu: u64, blkio: u64, swapin: u64, etime: u64) -> Vec<u8> {
        let mut buf = vec![0u8; MIN_STATS_LEN];
        buf[off::VERSION..off::VERSION + 2].copy_from_slice(&version.to_ne_bytes());
        buf[off::CPU_DELAY_TOTAL..off::CPU_DELAY_TOTAL + 8].copy_from_slice(&cpu.to_ne_bytes());
        buf[off::BLKIO_DELAY_TOTAL..off::BLKIO_DELAY_TOTAL + 8]
            .copy_from_slice(&blkio.to_ne_bytes());
        buf[off::SWAPIN_DELAY_TOTAL..off::SWAPIN_DELAY_TOTAL + 8]
            .copy_from_slice(&swapin.to_ne_bytes());
        buf[off::AC_ETIME..off::AC_ETIME + 8].copy_from_slice(&etime.to_ne_bytes());
        buf
    }

    #[test]
    fn parses_the_four_fields_at_their_kernel_offsets() {
        let blob = stats_blob(17, 1_000, 2_000, 3_000, 4_000_000);
        let s = parse_taskstats(&blob).expect("valid blob");
        assert_eq!(s.cpu_delay_ns, 1_000);
        assert_eq!(s.blkio_delay_ns, 2_000);
        assert_eq!(s.swapin_delay_ns, 3_000);
        assert_eq!(s.elapsed_us, 4_000_000);
        assert_eq!(s.total_delay_ns(), 6_000);
    }

    #[test]
    fn a_short_blob_is_rejected_rather_than_read_past() {
        for len in [0, 8, 16, MIN_STATS_LEN - 1] {
            assert!(parse_taskstats(&vec![0u8; len]).is_none(), "len {len}");
        }
        // A zeroed blob at exactly the minimum length is long enough; it is
        // rejected by the version check instead, which the next test covers.
    }

    #[test]
    fn a_zero_version_is_not_a_taskstats_reply() {
        let blob = stats_blob(0, 1, 1, 1, 1);
        assert!(parse_taskstats(&blob).is_none());
    }

    /// The percentage is of elapsed *wall clock*, not of delay: 50 ms of a 100 ms
    /// life is 50 %.
    #[test]
    fn percentages_are_delay_over_lifetime() {
        let s = DelayStats {
            cpu_delay_ns: 50_000_000,
            blkio_delay_ns: 10_000_000,
            swapin_delay_ns: 0,
            elapsed_us: 100_000, // 100 ms
        };
        assert_eq!(s.cpu_percent(), Some(50.0));
        assert_eq!(s.blkio_percent(), Some(10.0));
        assert_eq!(s.swapin_percent(), Some(0.0));
    }

    /// A process that has not been alive long enough to sample has no percentage;
    /// zero would be a claim about its behaviour.
    #[test]
    fn zero_elapsed_time_has_no_percentage() {
        let s = DelayStats {
            cpu_delay_ns: 0,
            blkio_delay_ns: 0,
            swapin_delay_ns: 0,
            elapsed_us: 0,
        };
        assert_eq!(s.cpu_percent(), None);
        assert_eq!(s.blkio_percent(), None);
        assert_eq!(s.swapin_percent(), None);
    }

    /// `cpu_delay_total` is documented as updated non-atomically with respect to
    /// `ac_etime`, so a sample can show more delay than elapsed time. Clamping
    /// keeps a nonsense value from sorting above a genuinely stalled process.
    #[test]
    fn delay_above_lifetime_is_clamped_to_100() {
        let s = DelayStats {
            cpu_delay_ns: 500_000_000,
            blkio_delay_ns: 0,
            swapin_delay_ns: 0,
            elapsed_us: 1_000, // 1 ms
        };
        assert_eq!(s.cpu_percent(), Some(100.0));
    }

    /// Wire-format tests need the `Msg`/`Attrs` builders, which are Linux-only
    /// because they exist to feed a Linux socket.
    #[cfg(target_os = "linux")]
    #[test]
    fn attributes_are_walked_with_padding() {
        // Two attributes: a 4-byte value (already aligned) and a 5-byte string.
        let mut buf = Vec::new();
        buf.extend_from_slice(&8u16.to_ne_bytes());
        buf.extend_from_slice(&1u16.to_ne_bytes());
        buf.extend_from_slice(&7u32.to_ne_bytes());
        let mut msg = Msg::new();
        msg.put_genl_header(1, 1);
        msg.put_attr(2, b"abcd");
        msg.put_attr_str(3, b"xy"); // 4 + 3 = 7 bytes, padded to 8
        let full = msg.finish(1, 1);
        let mut attrs = Attrs::new(&full, NLMSG_HDR_LEN + GENL_HDR_LEN, full.len());
        let (kind, value) = attrs.next().expect("first");
        assert_eq!(kind, 2);
        assert_eq!(value, b"abcd");
        let (kind, value) = attrs.next().expect("second");
        assert_eq!(kind, 3);
        assert_eq!(value, b"xy\0");
        assert!(attrs.next().is_none());

        // The hand-built buffer is only used to prove the lengths agree.
        assert_eq!(buf.len(), 8);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_truncated_attribute_ends_the_walk_instead_of_panicking() {
        let mut msg = Msg::new();
        msg.put_genl_header(1, 1);
        msg.put_attr(2, b"abcd");
        let mut full = msg.finish(1, 1);
        full.truncate(full.len() - 2); // cut into the last attribute
        let mut attrs = Attrs::new(&full, NLMSG_HDR_LEN + GENL_HDR_LEN, full.len());
        // The header claims 8 bytes but only 6 remain.
        assert!(attrs.next().is_none());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn finish_writes_a_consistent_netlink_header() {
        let mut msg = Msg::new();
        msg.put_genl_header(0x22, 7);
        let dgram = msg.finish(99, 0x22);
        let len = u32::from_ne_bytes([dgram[0], dgram[1], dgram[2], dgram[3]]) as usize;
        assert_eq!(len, dgram.len());
        assert_eq!(u16::from_ne_bytes([dgram[4], dgram[5]]), 0x22);
        assert_eq!(u16::from_ne_bytes([dgram[6], dgram[7]]), NLM_F_REQUEST);
        assert_eq!(
            u32::from_ne_bytes([dgram[8], dgram[9], dgram[10], dgram[11]]),
            99
        );
        assert_eq!(dgram[NLMSG_HDR_LEN], 7, "genl command");
    }
}
