//! `sysctl` and `libkvm` wrappers.
//!
//! Every `unsafe` block in the backend lives here, with the invariant documented
//! next to it. The rest of the module works with safe types.
//!
//! # Two sources, and why not one
//!
//! `sysctl(KERN_PROC_PROC)` returns every process with its own CPU time, memory
//! and thread count in a single call, so the common path needs nothing else. Two
//! things it cannot answer are reached through `libkvm`:
//!
//! * **swap** — `kvm_getswapinfo(3)` is the only interface, and there is no
//!   `sysctl` that reports *used* swap (`vm.swap_total` is the total only).
//! * **per-disk throughput** — `devstat_getdevs(3)` needs a `kvm_t` handle.
//!
//! Both are opened lazily and closed on failure, so a machine where `libkvm`
//! cannot read the kernel still lists processes.

use std::ffi::{CStr, CString};

use libc::{c_char, c_int, c_void};

/// The `kinfo_proc` fields this backend uses, copied out of the kernel's struct.
///
/// Copying rather than holding a reference to the mmap'd `sysctl` buffer keeps
/// the rest of the module safe: the buffer is freed before any row is built.
#[derive(Copy, Clone, Debug, Default)]
pub struct RawProc {
    pub pid: u32,
    /// Thread id. Equal to `pid` for a single-threaded process, and to the
    /// kernel thread's id for a kernel thread, so it is **not** a second process
    /// id.
    pub tid: i32,
    pub ppid: u32,
    pub pgid: u32,
    pub sid: u32,
    pub tpgid: i32,
    /// `kinfo_proc.ki_stat`: the `S*` state code, not a state letter.
    pub stat: i8,
    /// `ki_tdflags & TDF_SINTR`: whether the sleep can be interrupted.
    ///
    /// This is the difference between `S` and `D`: a process sleeping
    /// *uninterruptibly* (usually on disk I/O) shows `D`, and one in an
    /// ordinary interruptible sleep shows `S`. `ps` reads the same flag
    /// (`bin/ps/print.c`), and without it every sleeping process renders `S`,
    /// hiding exactly the state that explains why a machine feels stuck.
    pub tdf_intr: bool,
    /// `ki_slptime`, in seconds, for the same reason: `ps` prints `I` (idle
    /// kernel thread) for an interruptible sleep longer than `MAXSLP` (20 s).
    pub slptime: u32,
    pub nice: i8,
    /// `ki_pri.pri_class`: the scheduling class, not the numeric priority.
    pub pri_class: u8,
    /// `ki_pri.pri_level`: the scheduling priority itself.
    pub pri_level: u8,
    pub uid: u32,
    /// `ki_runtime`, in **microseconds** (`rux_runtime` is the same figure in
    /// microseconds; `ki_rusage.ru_utime` is a `timeval`).
    pub runtime_us: u64,
    /// Start time, absolute (`ki_start.tv_sec/tv_usec`).
    pub start_sec: i64,
    pub start_usec: i64,
    /// Resident set size, in **pages**, so it must be scaled by the page size.
    pub rss_pages: i64,
    /// Virtual size in bytes; already bytes, unlike `rss_pages`.
    pub vsize: u64,
    pub num_threads: i32,
    /// The CPU the thread is on now (`NOCPU` = -1 when it is not running).
    pub oncpu: i32,
    /// The CPU it last ran on.
    pub lastcpu: i32,
    /// Minor and major faults from `ki_rusage`.
    pub min_flt: u64,
    pub maj_flt: u64,
    /// Controlling terminal, already a `dev_t` (`0xffffffffffffffff` for none).
    pub tty_dev: u64,
    pub comm: [c_char; COMM_LEN + 1],
    pub tdname: [c_char; TDNAMLEN + 1],
    /// The jail id, which is FreeBSD's container analogue (`0` = host).
    pub jail: i32,
}

const COMM_LEN: usize = 19; // COMMLEN

/// `TDF_SINTR`: the sleeper can be woken by a signal (`sys/proc.h`).
const TDF_SINTR: u64 = 0x0000_0008;
const TDNAMLEN: usize = 15;

impl RawProc {
    pub fn comm_str(&self) -> String {
        c_array_to_string(&self.comm)
    }

    pub fn tdname_str(&self) -> String {
        c_array_to_string(&self.tdname)
    }

    /// Whether the kernel owns this thread (`PRI_ITHD`).
    pub fn is_kernel_thread(&self) -> bool {
        self.pri_class == PRI_ITHD
    }
}

/// `PRI_*` scheduling classes, from `<sys/priority.h>`.
///
/// These are the values `ki_pri.pri_class` takes. They are *not* what `nice`
/// means, and `PRI_TIMESHARE` covers both normal and `nice`d processes.
pub const PRI_ITHD: u8 = 1; // interrupt thread: kernel thread
pub const PRI_REALTIME: u8 = 2;
pub const PRI_TIMESHARE: u8 = 3;
pub const PRI_IDLE: u8 = 4;

fn c_array_to_string(arr: &[c_char]) -> String {
    let bytes: Vec<u8> = arr
        .iter()
        .take_while(|c| **c != 0)
        .map(|c| char_byte(*c))
        .collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

/// One `c_char` as a byte.
///
/// `c_char` is `i8` on x86 and `u8` on arm, so the cast is load-bearing on one
/// architecture and a no-op on the other. Naming it once keeps the `allow` in a
/// single place instead of at every extraction site.
#[inline]
fn char_byte(c: c_char) -> u8 {
    #[allow(clippy::unnecessary_cast)]
    {
        c as u8
    }
}

/// Read a `sysctl` MIB into a fresh buffer, growing on `ENOMEM`.
///
/// FreeBSD's `kern.proc` size query is explicitly racy: the answer is the size
/// *now*, and the table can grow between the two calls. Three retries with
/// headroom is what `ps(1)` does and it is enough in practice; a machine that
/// forks thousands of processes a second can still lose the race, which shows up
/// as an `io::Error` rather than as a truncated table.
fn sysctl_buf(mib: &mut [c_int]) -> std::io::Result<Vec<u8>> {
    let mut size: usize = 0;
    // SAFETY: a null `oldp` with a zero length is the documented way to ask for
    // the required size; `mib` is a live slice of the right length.
    let rc = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            mib.len() as u32,
            std::ptr::null_mut(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 {
        return Err(std::io::Error::last_os_error());
    }

    for attempt in 0..3usize {
        // Headroom grows quadratically: a process table that is growing while we
        // read it needs more room each time, not a fixed pad.
        let capacity = size + 4096 + attempt * attempt * 65_536;
        let mut buf = vec![0u8; capacity];
        let mut got = capacity;
        // SAFETY: `buf` is `got` bytes and `got` is passed as its length; the
        // kernel writes at most that many and returns the real count in `got`.
        let rc = unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                mib.len() as u32,
                buf.as_mut_ptr().cast::<c_void>(),
                &mut got,
                std::ptr::null_mut(),
                0,
            )
        };
        if rc == 0 {
            buf.truncate(got);
            return Ok(buf);
        }
        let err = std::io::Error::last_os_error();
        if err.raw_os_error() != Some(libc::ENOMEM) {
            return Err(err);
        }
        size = got.max(size);
    }
    Err(std::io::Error::other("sysctl table kept growing"))
}

/// One `kinfo_proc`, as the kernel lays it out.
///
/// Taken from `libc`, which declares FreeBSD's struct in full — unlike Darwin's,
/// where the offsets had to be recovered by hand (`darwin/kinfo.rs`). Using the
/// real struct means the compiler computes the offsets, and a `libc` that
/// disagrees with the running kernel is caught by [`validate`] instead of
/// silently misreading every field.
type KInfoProc = libc::kinfo_proc;

/// Read every process (`KERN_PROC_PROC`).
///
/// `KERN_PROC_PROC` rather than `KERN_PROC_ALL` for two reasons:
///
/// * `KERN_PROC_ALL` is documented to return "everything", which includes
///   entries that are not processes, and on this kernel it is rejected outright
///   with `ENOTDIR` when given the four-element MIB.
/// * `PROC` already excludes the zombies' thread-group duplicates.
pub fn list_processes() -> std::io::Result<Vec<RawProc>> {
    let mut mib = [libc::CTL_KERN, libc::KERN_PROC, libc::KERN_PROC_PROC, 0];
    let buf = sysctl_buf(&mut mib)?;
    Ok(decode(&buf))
}

/// One process's *threads*, including the thread-group leader.
///
/// `KERN_PROC_INC_THREAD` ORs into the selector, and only works with a
/// *specific* pid: combining it with `KERN_PROC_PROC` fails with `ENOENT`,
/// because the selector then asks for "all processes, one row per thread" and
/// the kernel has no such table. That is why threads are read per process here
/// rather than as one extra field on the bulk scan.
pub fn list_threads(pid: u32) -> std::io::Result<Vec<RawProc>> {
    if pid == 0 {
        // Pid 0 collects the kernel's own threads. They have no address space and
        // no user, so they are not processes and cannot be listed this way.
        return Ok(Vec::new());
    }
    let mut mib = [
        libc::CTL_KERN,
        libc::KERN_PROC,
        libc::KERN_PROC_PID | libc::KERN_PROC_INC_THREAD,
        pid as c_int,
    ];
    let buf = sysctl_buf(&mut mib)?;
    Ok(decode(&buf))
}

/// Decode a `kinfo_proc` array.
///
/// The records are fixed-size and `KERN_PROC` guarantees `ki_structsize` matches
/// the build, so a partially-written tail is dropped instead of being read as a
/// process at pid 0.
fn decode(buf: &[u8]) -> Vec<RawProc> {
    let stride = std::mem::size_of::<KInfoProc>();
    if stride == 0 {
        return Vec::new();
    }
    let count = buf.len() / stride;
    let mut out = Vec::with_capacity(count);

    for i in 0..count {
        let start = i * stride;
        // SAFETY: `start + stride <= buf.len()` by the loop bound, and
        // `kinfo_proc` is a plain-old-data struct of integers and arrays, so any
        // bit pattern is a valid value.
        let kp: KInfoProc = unsafe {
            std::ptr::read_unaligned(buf.as_ptr().add(start).cast::<KInfoProc>())
        };

        // A record whose declared size disagrees with the slice was written by a
        // kernel with a different ABI; reading it would produce plausible-looking
        // nonsense, so stop at the first one.
        if kp.ki_structsize as usize != stride {
            break;
        }

        let mut comm = [0 as c_char; COMM_LEN + 1];
        for (dst, src) in comm.iter_mut().zip(kp.ki_comm.iter()) {
            *dst = *src;
        }
        let mut tdname = [0 as c_char; TDNAMLEN + 1];
        for (dst, src) in tdname.iter_mut().zip(kp.ki_tdname.iter()) {
            *dst = *src;
        }

        out.push(RawProc {
            pid: kp.ki_pid.max(0) as u32,
            tid: kp.ki_tid,
            ppid: kp.ki_ppid.max(0) as u32,
            pgid: kp.ki_pgid.max(0) as u32,
            sid: kp.ki_sid.max(0) as u32,
            tpgid: kp.ki_tpgid,
            stat: kp.ki_stat as i8,
            tdf_intr: (kp.ki_tdflags as u64 & TDF_SINTR) != 0,
            slptime: kp.ki_slptime,
            nice: kp.ki_nice as i8,
            pri_class: kp.ki_pri.pri_class as u8,
            pri_level: kp.ki_pri.pri_level as u8,
            uid: kp.ki_uid,
            runtime_us: kp.ki_runtime,
            start_sec: kp.ki_start.tv_sec as i64,
            start_usec: kp.ki_start.tv_usec as i64,
            rss_pages: kp.ki_rssize as i64,
            vsize: kp.ki_size as u64,
            num_threads: kp.ki_numthreads,
            oncpu: kp.ki_oncpu,
            lastcpu: kp.ki_lastcpu,
            min_flt: kp.ki_rusage.ru_minflt as u64,
            // `ps`'s `MAJFLT` column is `ki_rusage.ru_majflt`; `ki_cow` is the
            // copy-on-write count, which htop prints as MAJFLT and which is a
            // different number. The rusage field is the one the kernel labels
            // "major faults", so it is the one shown.
            maj_flt: kp.ki_rusage.ru_majflt as u64,
            tty_dev: kp.ki_tdev as u64,
            comm,
            tdname,
            jail: kp.ki_jid,
        });
    }
    out
}

/// The runtime layout check, run at startup and by the tests.
///
/// `libc`'s `kinfo_proc` is compiled into the binary; the kernel's is whatever
/// the running system was built with. They agree only if the binary was built
/// for an ABI no newer than the kernel's. A mismatch does not fail:
/// `ki_structsize` reports the *kernel's* size, so a struct that grew makes every
/// field after the addition read from the wrong offset, and the table looks
/// plausible while being wrong.
///
/// Two checks catch it:
///
/// * the kernel's declared `ki_structsize` must equal our `size_of`, and
/// * pid 1 must be `init` with uid 0, which no offset error reproduces.
pub fn validate() -> Result<(), String> {
    let stride = std::mem::size_of::<KInfoProc>();
    let procs = list_processes().map_err(|e| format!("kern.proc: {e}"))?;
    if procs.is_empty() {
        return Err("kern.proc returned no processes".to_string());
    }
    // `decode` already drops records of the wrong size, so an empty result *is*
    // the signal. Confirm the reason explicitly for the error message.
    let mut mib = [libc::CTL_KERN, libc::KERN_PROC, libc::KERN_PROC_PROC, 0];
    let raw = sysctl_buf(&mut mib).map_err(|e| format!("kern.proc: {e}"))?;
    if raw.len() < stride {
        return Err(format!(
            "kern.proc returned {} bytes, less than one {stride}-byte record",
            raw.len()
        ));
    }
    // SAFETY: the buffer is at least one record long.
    let first: KInfoProc =
        unsafe { std::ptr::read_unaligned(raw.as_ptr().cast::<KInfoProc>()) };
    if first.ki_structsize as usize != stride {
        return Err(format!(
            "kinfo_proc layout mismatch: kernel says {}, this build has {stride}; \
             build gander for this FreeBSD version",
            first.ki_structsize
        ));
    }

    let init = procs.iter().find(|p| p.pid == 1);
    match init {
        Some(p) if p.comm_str().starts_with("init") && p.uid == 0 => Ok(()),
        Some(p) => Err(format!(
            "pid 1 looks wrong: comm={:?} uid={}, expected init/0 — kinfo_proc offsets are off",
            p.comm_str(),
            p.uid
        )),
        None => Err("pid 1 missing from kern.proc".to_string()),
    }
}

/// Fetch a single process, for `-p`-style lookups and tests.
pub fn process(pid: u32) -> Option<RawProc> {
    let mut mib = [libc::CTL_KERN, libc::KERN_PROC, libc::KERN_PROC_PID, pid as c_int];
    let buf = sysctl_buf(&mut mib).ok()?;
    decode(&buf).into_iter().next()
}

/// A string from a per-process `sysctl` (`KERN_PROC_PATHNAME`, `KERN_PROC_CWD`,
/// `KERN_PROC_ARGS`).
///
/// The reply is a NUL-terminated C string for the first two, and a run of
/// NUL-separated strings for `ARGS`; `max` bounds the buffer because the size
/// query for these returns `ENOMEM` rather than a length.
pub fn proc_string(selector: c_int, pid: u32, max: usize) -> Option<Vec<u8>> {
    let mut mib = [libc::CTL_KERN, libc::KERN_PROC, selector, pid as c_int];
    let mut buf = vec![0u8; max];
    let mut size = buf.len();
    // SAFETY: `buf` is `size` bytes and `size` is passed as its length.
    let rc = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            mib.len() as u32,
            buf.as_mut_ptr().cast::<c_void>(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 || size == 0 {
        return None;
    }
    buf.truncate(size);
    Some(buf)
}

/// The executable path of one process.
pub fn exe_path(pid: u32) -> Option<String> {
    let raw = proc_string(libc::KERN_PROC_PATHNAME, pid, libc::PATH_MAX as usize)?;
    let text = cstr_prefix(&raw);
    (!text.is_empty()).then_some(text)
}

/// The working directory of one process.
///
/// `KERN_PROC_CWD` returns a **`struct kinfo_file`**, not a path: the sysctl
/// mnemonic says "cwd" but the payload is the file-descriptor record for it, so
/// the path is at `kf_path`. Reading the buffer as a string yields `\0` for every
/// process, which looks like "cwd unavailable" rather than a parsing bug — and
/// that is exactly what a `KERN_PROC_CWD` buffer of `PATH_MAX` bytes with 0
/// returned on a live system (`ENOMEM`, because the struct is 1392 bytes and the
/// requested buffer was too small).
pub fn cwd_path(pid: u32) -> Option<String> {
    let want = std::mem::size_of::<libc::kinfo_file>();
    let raw = proc_string(libc::KERN_PROC_CWD, pid, want + 256)?;
    if raw.len() < want {
        return None;
    }
    // SAFETY: `raw.len() >= want`, and `kinfo_file` is plain old data.
    let kf: libc::kinfo_file = unsafe { std::ptr::read_unaligned(raw.as_ptr().cast()) };
    let bytes: Vec<u8> = kf
        .kf_path
        .iter()
        .take_while(|c| **c != 0)
        .map(|c| char_byte(*c))
        .collect();
    let text = String::from_utf8_lossy(&bytes).into_owned();
    (!text.is_empty()).then_some(text)
}

/// The command line, as `argv` joined by spaces.
///
/// `KERN_PROC_ARGS` returns the strings NUL-separated with no trailing
/// terminator, so the walk is by `strlen`, and a buffer that does not end in a
/// NUL is truncated rather than trusted. It is also settable
/// (`setproctitle`), so the value is whatever the process chose to show.
pub fn cmdline(pid: u32) -> Option<String> {
    // `ARG_MAX` is 256 KiB on FreeBSD; ask for that rather than for the exact
    // length, because the size query fails with `ENOMEM` for this selector.
    let raw = proc_string(libc::KERN_PROC_ARGS, pid, 256 * 1024)?;
    let mut out = String::with_capacity(raw.len());
    let mut rest = &raw[..];
    while !rest.is_empty() {
        let end = rest.iter().position(|b| *b == 0).unwrap_or(rest.len());
        if end > 0 {
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str(&String::from_utf8_lossy(&rest[..end]));
        }
        if end == rest.len() {
            break;
        }
        rest = &rest[end + 1..];
    }
    (!out.is_empty()).then_some(out)
}

fn cstr_prefix(raw: &[u8]) -> String {
    let end = raw.iter().position(|b| *b == 0).unwrap_or(raw.len());
    String::from_utf8_lossy(&raw[..end]).into_owned()
}

/// `kern.boottime`, as seconds since the epoch.
#[allow(clippy::unnecessary_cast)] // `time_t` is 32-bit on i386
pub fn boot_time() -> Option<i64> {
    let mut tv = std::mem::MaybeUninit::<libc::timeval>::zeroed();
    let mut size = std::mem::size_of::<libc::timeval>();
    // SAFETY: `tv` is exactly `size` bytes and writable.
    let rc = unsafe {
        libc::sysctlbyname(
            c"kern.boottime".as_ptr(),
            tv.as_mut_ptr().cast::<c_void>(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 {
        return None;
    }
    // SAFETY: a zero return means the struct was filled.
    let tv = unsafe { tv.assume_init() };
    (tv.tv_sec > 0).then_some(tv.tv_sec as i64)
}

/// `kern.cp_time`, the aggregate CPU time counters.
///
/// A `long[CPUSTATES]` in clock ticks, in the order `CP_USER, CP_NICE, CP_SYS,
/// CP_INTR, CP_IDLE`. Unlike Linux's `/proc/stat` there is no `iowait`, `steal`
/// or `guest` — FreeBSD accounts I/O wait as idle, so `iowait` is 0 rather than
/// unknown.
pub fn cp_time() -> Option<[i64; 5]> {
    let mut buf = [0 as libc::c_long; 5];
    let mut size = std::mem::size_of::<[libc::c_long; 5]>();
    // SAFETY: `buf` is exactly `size` bytes.
    let rc = unsafe {
        libc::sysctlbyname(
            c"kern.cp_time".as_ptr(),
            buf.as_mut_ptr().cast::<c_void>(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 || size != std::mem::size_of::<[libc::c_long; 5]>() {
        return None;
    }
    // `c_long` is 64-bit on amd64/arm64 and 32-bit on i386, so the cast is
    // load-bearing on one target and a no-op on the others. `as` is the only
    // spelling that compiles on both without needing a target-specific branch.
    Some([
        wide(buf[0]),
        wide(buf[1]),
        wide(buf[2]),
        wide(buf[3]),
        wide(buf[4]),
    ])
}

/// `kern.cp_times`, the per-CPU counters, one `CPUSTATES`-long group per CPU.
pub fn cp_times() -> Option<Vec<[i64; 5]>> {
    let mut size: usize = 0;
    // SAFETY: a null buffer asks for the size only.
    let rc = unsafe {
        libc::sysctlbyname(
            c"kern.cp_times".as_ptr(),
            std::ptr::null_mut(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 || size == 0 || !size.is_multiple_of(5 * std::mem::size_of::<libc::c_long>()) {
        return None;
    }
    let mut buf = vec![0 as libc::c_long; size / std::mem::size_of::<libc::c_long>()];
    // SAFETY: `buf` is `size` bytes and `size` is passed as its length.
    let rc = unsafe {
        libc::sysctlbyname(
            c"kern.cp_times".as_ptr(),
            buf.as_mut_ptr().cast::<c_void>(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 {
        return None;
    }
    let groups = buf.as_chunks::<5>().0;
    Some(
        groups
            .iter()
            .map(|c| [wide(c[0]), wide(c[1]), wide(c[2]), wide(c[3]), wide(c[4])])
            .collect(),
    )
}

/// An unsigned `sysctl` by name, for the `vm.stats` counters.
///
/// Those are all `u_int` (32 bits) on FreeBSD, including `v_free_count`, which a
/// machine with more than 16 TiB of RAM would overflow — the same wrap Linux's
/// `MemFree` had before 64-bit counters. `u64` is requested and the kernel's
/// actual length checked, so a future 64-bit counter works unchanged.
pub fn sysctl_u64(name: &CStr) -> Option<u64> {
    let mut size: usize = 0;
    // SAFETY: a null destination asks for the size only.
    let rc = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            std::ptr::null_mut(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 || size == 0 {
        return None;
    }
    match size {
        4 => {
            let mut v: u32 = 0;
            let mut len = 4usize;
            // SAFETY: the destination is exactly `len` bytes.
            let rc = unsafe {
                libc::sysctlbyname(
                    name.as_ptr(),
                    (&mut v as *mut u32).cast::<c_void>(),
                    &mut len,
                    std::ptr::null_mut(),
                    0,
                )
            };
            (rc == 0).then_some(v as u64)
        }
        8 => {
            let mut v: u64 = 0;
            let mut len = 8usize;
            // SAFETY: as above, with the 64-bit width the kernel reported.
            let rc = unsafe {
                libc::sysctlbyname(
                    name.as_ptr(),
                    (&mut v as *mut u64).cast::<c_void>(),
                    &mut len,
                    std::ptr::null_mut(),
                    0,
                )
            };
            (rc == 0).then_some(v)
        }
        _ => None,
    }
}

/// Widen a `c_long` counter to `i64`.
///
/// `c_long` is 64-bit on amd64 and arm64 and 32-bit on i386, so the cast is
/// load-bearing on one target and a no-op on the others. One function keeps the
/// allow in one place instead of at eleven call sites.
#[inline]
fn wide(v: libc::c_long) -> i64 {
    #[allow(clippy::unnecessary_cast)]
    {
        v as i64
    }
}

/// Swap usage, from `kvm_getswapinfo(3)`.
///
/// Returns `None` when `libkvm` cannot open the kernel — a jailed process is
/// denied, and the meter then says `n/a here` rather than drawing an empty bar
/// on a machine that does have swap.
pub fn swap_info() -> Option<crate::system::SwapInfo> {
    let kd = Kvm::open()?;
    // One entry is enough for the totals: `kvm_getswapinfo` accumulates every
    // applicable device into entry 0 when asked for a single slot.
    let mut entries = [std::mem::MaybeUninit::<libc::kvm_swap>::zeroed(); 1];
    // SAFETY: `entries` is one `kvm_swap` and the count is passed as 1.
    let n = unsafe {
        libc::kvm_getswapinfo(kd.0, entries.as_mut_ptr().cast(), 1, 0)
    };
    if n < 0 {
        return None;
    }
    // SAFETY: a non-negative return means entry 0 was filled.
    let entry = unsafe { entries[0].assume_init() };
    // `ksw_total`/`ksw_used` are in **pages**, not bytes and not kilobytes.
    // FreeBSD's own `swapinfo(8)` multiplies by `pagesize`, and using 1024 here
    // would report a 1 GiB swap file as 4 MiB.
    let page = crate::freebsd::FreeBsdPlatform::page_size();
    let total = (entry.ksw_total as u64).saturating_mul(page);
    let used = (entry.ksw_used as u64).saturating_mul(page);
    Some(crate::system::SwapInfo {
        total,
        used,
        free: total.saturating_sub(used),
    })
}

/// A `libkvm` handle, closed on drop.
struct Kvm(*mut libc::kvm_t);

impl Kvm {
    fn open() -> Option<Self> {
        let mut err = [0 as c_char; 256];
        // SAFETY: the four NULLs mean "this running kernel", and `err` is a
        // 256-byte buffer, which is the documented minimum for `kvm_openfiles`.
        let kd = unsafe {
            libc::kvm_openfiles(
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                libc::O_RDONLY,
                err.as_mut_ptr(),
            )
        };
        if kd.is_null() {
            return None;
        }
        Some(Kvm(kd))
    }
}

impl Drop for Kvm {
    fn drop(&mut self) {
        // SAFETY: `self.0` came from `kvm_openfiles` and is closed exactly once,
        // here, because `Kvm` is not `Clone` and owns the handle.
        unsafe {
            libc::kvm_close(self.0);
        }
    }
}

/// Host-wide disk byte counters, from `devstat_getdevs(3)`.
///
/// `devstat` is the kernel's own per-device statistics interface, so unlike
/// Linux's `/proc/diskstats` there is no partition-versus-disk ambiguity to
/// resolve: each entry is a whole device (`ada0`, `da1`, …) and its counters
/// already include everything written through its partitions.
///
/// `DEVSTAT_TYPE_PASS` entries are skipped. Those are `pass(4)` devices, which
/// see the *same* I/O as the `da`/`ada` device they front, so summing both would
/// double every byte — the FreeBSD equivalent of counting `/dev/sda` and
/// `/dev/sda1`.
pub fn disk_counters() -> crate::system::DiskCounters {
    let mut out = crate::system::DiskCounters::default();
    let Some(kd) = Kvm::open() else {
        return out;
    };
    // `devstat_checkversion` must run before the device list is trusted: a
    // `devstat` struct that changed size would make every counter read from the
    // wrong offset, which is the same failure mode as a `kinfo_proc` mismatch.
    // SAFETY: `kd.0` is a live handle from `kvm_openfiles`.
    let version = unsafe { libc::devstat_checkversion(kd.0) };
    if version == 0 {
        return out;
    }

    let mut statinfo = StatInfo {
        dinfo: std::ptr::null_mut(),
        ..StatInfo::default()
    };
    // SAFETY: `statinfo` is a live `StatInfo` whose `dinfo` the callee replaces
    // with an allocation owned by the kvm handle.
    let rc = unsafe { devstat_getdevs(kd.0, &mut statinfo) };
    if rc != 0 || statinfo.dinfo.is_null() {
        return out;
    }
    // SAFETY: a zero return means `dinfo` points at a `devinfo` owned by the kvm
    // handle; it stays valid until `kvm_close`, which runs after this function
    // has finished reading.
    let dinfo = unsafe { &*statinfo.dinfo };
    if dinfo.devices.is_null() || dinfo.numdevs <= 0 {
        return out;
    }
    // SAFETY: `numdevs` is the length of the array `devstat_getdevs` allocated.
    let devices = unsafe { std::slice::from_raw_parts(dinfo.devices, dinfo.numdevs as usize) };

    for d in devices {
        // `DEVSTAT_TYPE_PASS` marks pass-through devices, which observe the same
        // requests as the physical device beneath them; summing both would
        // double every byte.
        if (d.device_type as u32) & (libc::devstat_type_flags::DEVSTAT_TYPE_PASS as u32) != 0 {
            continue;
        }
        out.read_bytes = out
            .read_bytes
            .saturating_add(d.bytes[libc::devstat_trans_flags::DEVSTAT_READ as usize]);
        out.write_bytes = out
            .write_bytes
            .saturating_add(d.bytes[libc::devstat_trans_flags::DEVSTAT_WRITE as usize]);
    }
    out
}

/// `struct statinfo` from `<devstat.h>`.
///
/// `libc` declares `devstat`, `devinfo` and `device_selection` but not this
/// wrapper, so it is declared here with the same field order. Only `dinfo` is
/// read; the `busy_time` fields are doubles the library fills in for its own
/// `devstat_compute_statistics` helper.
#[repr(C)]
struct StatInfo {
    busy_time: f64,
    busy_time2: f64,
    busy_other: std::ffi::c_long,
    dcpyps: i32,
    _pad: i32,
    dinfo: *mut libc::devinfo,
    busy_time_total: f64,
    busy_other_total: std::ffi::c_long,
}

impl Default for StatInfo {
    fn default() -> Self {
        Self {
            busy_time: 0.0,
            busy_time2: 0.0,
            busy_other: 0,
            dcpyps: 0,
            _pad: 0,
            dinfo: std::ptr::null_mut(),
            busy_time_total: 0.0,
            busy_other_total: 0,
        }
    }
}

#[link(name = "devstat")]
extern "C" {
    /// Fill `stats` with the current per-device statistics.
    ///
    /// Returns 0 on success, -1 on failure (with `kvm_geterr` explaining why).
    fn devstat_getdevs(kd: *mut libc::kvm_t, stats: *mut StatInfo) -> c_int;
}

/// Logical CPU count, from `hw.ncpu`.
pub fn cpu_count() -> usize {
    sysctl_u64(c"hw.ncpu").unwrap_or(1).max(1) as usize
}

/// Host-wide network byte counters, from `sysctl(NET_RT_IFLIST)`.
///
/// The reply is a routing-socket dump: a sequence of variable-length messages
/// that must be walked by `ifm_msglen`. `RTM_IFINFO` messages carry
/// `struct if_data` with 64-bit `ifi_ibytes`/`ifi_obytes` — unlike Darwin, where
/// `if_data` is 32-bit and `NET_RT_IFLIST2`/`if_data64` is required.
///
/// Loopback is excluded for the same reason as on the other two backends: it
/// never reaches a wire, and counting it double-counts every local byte.
pub fn net_counters() -> crate::system::NetCounters {
    let mut out = crate::system::NetCounters::default();
    let mut mib = [libc::CTL_NET, libc::PF_ROUTE, 0, 0, libc::NET_RT_IFLIST, 0];
    let Ok(buf) = sysctl_buf(&mut mib) else {
        return out;
    };

    let mut offset = 0usize;
    let hdr_len = std::mem::size_of::<libc::if_msghdr>();
    while offset + hdr_len <= buf.len() {
        // The messages are not necessarily 8-byte aligned, so the header is
        // copied out rather than referenced.
        // SAFETY: `offset + hdr_len <= buf.len()` is the loop condition.
        let hdr: libc::if_msghdr = unsafe {
            std::ptr::read_unaligned(buf.as_ptr().add(offset).cast::<libc::if_msghdr>())
        };
        let msglen = hdr.ifm_msglen as usize;
        if msglen == 0 {
            break; // malformed; stop rather than spin
        }
        // Only interface-info messages carry the counters; the reply also holds
        // address and route messages, which are skipped by length.
        if hdr.ifm_type == libc::RTM_IFINFO as u8 {
            // Loopback never reaches a wire, and counting it would double every
            // local byte.
            if hdr.ifm_flags & libc::IFF_LOOPBACK == 0 {
                out.rx_bytes = out.rx_bytes.saturating_add(hdr.ifm_data.ifi_ibytes);
                out.tx_bytes = out.tx_bytes.saturating_add(hdr.ifm_data.ifi_obytes);
            }
        }
        offset = offset.saturating_add(msglen);
    }
    out
}

/// The login name for a uid, via `getpwuid_r`.
pub fn user_name(uid: u32) -> Option<String> {
    let mut buf = vec![0u8; 1024];
    let mut pwd = std::mem::MaybeUninit::<libc::passwd>::zeroed();
    let mut result: *mut libc::passwd = std::ptr::null_mut();
    // SAFETY: `buf` and `pwd` are live and correctly sized, and `result` is a
    // valid out-pointer; `getpwuid_r` writes at most `buf.len()` bytes.
    let rc = unsafe {
        libc::getpwuid_r(
            uid,
            pwd.as_mut_ptr(),
            buf.as_mut_ptr().cast::<c_char>(),
            buf.len(),
            &mut result,
        )
    };
    if rc != 0 || result.is_null() {
        return None;
    }
    // SAFETY: a non-null `result` points at the filled `pwd`.
    let pwd = unsafe { pwd.assume_init() };
    if pwd.pw_name.is_null() {
        return None;
    }
    // SAFETY: `pw_name` is a NUL-terminated string owned by the caller's buffer.
    let name = unsafe { CStr::from_ptr(pwd.pw_name) };
    Some(name.to_string_lossy().into_owned())
}

/// Resolve a `dev_t` to a device name (`ttyu0`, `pts/0`, …).
pub fn devname(dev: u64) -> Option<String> {
    if dev == u64::MAX {
        return None; // no controlling terminal
    }
    let mut buf = [0 as c_char; 256];
    // SAFETY: `buf` is 256 bytes and its length is passed; `devname_r` writes a
    // NUL-terminated name or returns NULL.
    let ptr = unsafe { libc::devname_r(dev, libc::S_IFCHR, buf.as_mut_ptr(), buf.len() as c_int) };
    if ptr.is_null() {
        return None;
    }
    // SAFETY: a non-null return points into `buf`.
    let name = unsafe { CStr::from_ptr(buf.as_ptr()) };
    let text = name.to_string_lossy().into_owned();
    (!text.is_empty()).then_some(text)
}

/// Hostname, from `sysctl(kern.hostname)`.
pub fn hostname() -> Option<String> {
    let mut buf = [0u8; 256];
    let mut size = buf.len();
    // SAFETY: `buf` is `size` bytes and `size` is passed as its length.
    let rc = unsafe {
        libc::sysctlbyname(
            c"kern.hostname".as_ptr(),
            buf.as_mut_ptr().cast::<c_void>(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 {
        return None;
    }
    let text = cstr_prefix(&buf[..size.min(buf.len())]);
    (!text.is_empty()).then_some(text)
}

/// CPU affinity of one process, via `cpuset_getaffinity(2)`.
pub fn affinity(pid: u32, cores: usize) -> Option<Vec<u64>> {
    // `cpuset_t` is 16 `c_long`s on FreeBSD 14+, i.e. 1024 bits, but the syscall
    // takes a size and rejects anything smaller than the kernel's set, so the
    // struct is used rather than a hand-rolled word count.
    let mut set = std::mem::MaybeUninit::<libc::cpuset_t>::zeroed();
    // SAFETY: `set` is one `cpuset_t` and its size is passed.
    let rc = unsafe {
        libc::cpuset_getaffinity(
            libc::CPU_LEVEL_WHICH,
            libc::CPU_WHICH_PID,
            pid as libc::id_t,
            std::mem::size_of::<libc::cpuset_t>(),
            set.as_mut_ptr(),
        )
    };
    if rc != 0 {
        return None;
    }
    // SAFETY: a zero return means the whole set was filled.
    let set = unsafe { set.assume_init() };
    // `cpuset_t`'s bits are private, so the struct is reinterpreted as bytes.
    // SAFETY: `cpuset_t` is a plain array of integers with no padding.
    let bytes = unsafe {
        std::slice::from_raw_parts(
            (&set as *const libc::cpuset_t).cast::<u8>(),
            std::mem::size_of::<libc::cpuset_t>(),
        )
    };
    let mut words = vec![0u64; cores.div_ceil(64).max(1)];
    for i in 0..cores {
        let byte = bytes.get(i / 8).copied().unwrap_or(0);
        if byte & (1 << (i % 8)) != 0 {
            words[i / 64] |= 1 << (i % 64);
        }
    }
    Some(words)
}

/// Set CPU affinity, via `cpuset_setaffinity(2)`.
pub fn set_affinity(pid: u32, words: &[u64], cores: usize) -> Result<(), String> {
    let mut set = std::mem::MaybeUninit::<libc::cpuset_t>::zeroed();
    // SAFETY: `set` is one `cpuset_t`, writable for the duration.
    let set_ptr = set.as_mut_ptr();
    unsafe {
        std::ptr::write_bytes(set_ptr.cast::<u8>(), 0, std::mem::size_of::<libc::cpuset_t>());
        let bytes = std::slice::from_raw_parts_mut(
            set_ptr.cast::<u8>(),
            std::mem::size_of::<libc::cpuset_t>(),
        );
        for i in 0..cores {
            let word = words.get(i / 64).copied().unwrap_or(0);
            if word & (1 << (i % 64)) != 0 {
                if let Some(byte) = bytes.get_mut(i / 8) {
                    *byte |= 1 << (i % 8);
                }
            }
        }
    }
    // SAFETY: `set` was fully initialised above and its size is passed.
    let rc = unsafe {
        libc::cpuset_setaffinity(
            libc::CPU_LEVEL_WHICH,
            libc::CPU_WHICH_PID,
            pid as libc::id_t,
            std::mem::size_of::<libc::cpuset_t>(),
            set.as_ptr(),
        )
    };
    if rc == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error().to_string())
    }
}

/// A `CString` for a sysctl name, for callers that need one.
#[allow(dead_code)]
pub fn cstring(name: &str) -> Option<CString> {
    CString::new(name).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_runtime_layout_check_passes() {
        // The whole backend depends on `libc`'s `kinfo_proc` matching the kernel's.
        validate().expect("kinfo_proc layout must match the running kernel");
    }

    #[test]
    fn our_own_process_is_listed_with_a_command_line() {
        let me = std::process::id();
        let procs = list_processes().expect("kern.proc");
        let mine = procs
            .iter()
            .find(|p| p.pid == me)
            .expect("own pid must appear");
        assert!(mine.runtime_us > 0, "cpu time must be non-zero");
        assert!(mine.vsize > 0, "virtual size must be non-zero");
        assert!(mine.rss_pages > 0, "resident size must be non-zero");
        assert_eq!(mine.uid, unsafe { libc::getuid() });
        assert!(cmdline(me).is_some(), "own cmdline must be readable");
        assert!(exe_path(me).is_some(), "own exe must be readable");
        assert!(cwd_path(me).is_some(), "own cwd must be readable");
    }

    #[test]
    fn threads_of_our_own_process_include_the_leader() {
        let me = std::process::id();
        let threads = list_threads(me).expect("thread list");
        assert!(!threads.is_empty());
        assert!(
            threads.iter().any(|t| t.pid == me),
            "the leader is one of its own threads"
        );
        assert!(threads.iter().all(|t| t.tid != 0), "a tid is never zero");
    }

    #[test]
    fn boot_time_is_in_the_past() {
        let boot = boot_time().expect("kern.boottime");
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        assert!(boot > 0 && boot <= now, "boot {boot} now {now}");
    }

    #[test]
    fn cpu_counters_are_monotonic() {
        let a = cp_time().expect("kern.cp_time");
        std::thread::sleep(std::time::Duration::from_millis(30));
        let b = cp_time().expect("kern.cp_time");
        for i in 0..5 {
            assert!(b[i] >= a[i], "CPUSTATES[{i}] went backwards: {} -> {}", a[i], b[i]);
        }
        let per = cp_times().expect("kern.cp_times");
        assert_eq!(per.len(), crate::freebsd::FreeBsdPlatform::cpu_count());
    }

    #[test]
    fn swap_is_readable_when_configured() {
        // A machine with no swap is legitimate; what must not happen is a panic
        // or a figure larger than the machine.
        if let Some(s) = swap_info() {
            assert!(s.used <= s.total, "{s:?}");
        }
    }

    #[test]
    fn devname_resolves_our_tty_or_says_nothing() {
        let me = std::process::id();
        let procs = list_processes().unwrap();
        if let Some(mine) = procs.iter().find(|p| p.pid == me) {
            if mine.tty_dev != u64::MAX {
                assert!(devname(mine.tty_dev).is_some());
            }
        }
        assert_eq!(devname(u64::MAX), None, "no terminal is not a device name");
    }
}
