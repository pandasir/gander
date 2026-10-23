//! Darwin syscall wrappers.
//!
//! Every `unsafe` block in the backend lives here, with the invariant documented
//! next to it. The rest of the module works with safe types.

// `libc` marks its Mach bindings as deprecated in favour of the `mach2` crate.
// They are still correct and stable; taking `mach2` would add a dependency for
// four functions, so the warnings are silenced here, once, with intent.
#![allow(deprecated)]

use std::ffi::CStr;

use libc::{c_int, c_void};

/// One entry of the `sysctl(KERN_PROC_ALL)` result that we actually use.
///
/// Sourced from `proc_bsdinfo` rather than `kinfo_proc`: `libc` does not expose
/// Darwin's `kinfo_proc` (its layout is large and version-sensitive), while
/// `proc_bsdinfo` carries every field we need and arrives in the same
/// `PROC_PIDTASKALLINFO` call as the task info.
#[derive(Copy, Clone, Debug, Default)]
pub struct KInfoProc {
    pub pid: u32,
    pub ppid: u32,
    pub uid: u32,
    pub nice: i32,
    pub p_stat: i8,
    pub start_sec: u64,
    pub start_usec: u64,
    /// Process group and foreground process group of the controlling terminal.
    ///
    /// Both are in `proc_bsdinfo` (`pbi_pgid`, `e_tpgid`), so they cost nothing
    /// extra. `session` is *not* there and comes from `getsid(2)`, which is a
    /// syscall — it is cached with the process identity instead, in
    /// `darwin::system::Sampler::identity`.
    ///
    /// `tpgid` is `-1` when the terminal has no foreground job — the same wire
    /// value Linux uses in field 8 of `stat`, so the renderer needs no per-platform
    /// special case.
    pub pgrp: u32,
    pub tpgid: i64,
    /// The controlling terminal's `dev_t`, or `-1` for none.
    ///
    /// Darwin spells "no controlling terminal" as `e_tdev == (dev_t)-1`, where Linux
    /// uses `0`. Both are normalised to `0` here so the renderer has one rule.
    pub tty_dev: i64,
    pub comm: [u8; 17],
}

/// List all process IDs.
///
/// `proc_listallpids` returns a *count* of PIDs, not a byte size, so a null
/// buffer yields an estimate that can already be stale. Ask once for the
/// estimate, over-allocate, then truncate to the count actually written.
pub fn list_pids() -> std::io::Result<Vec<u32>> {
    let elem = std::mem::size_of::<i32>();

    // SAFETY: a null buffer is the documented way to ask for the estimate.
    let estimate = unsafe { libc::proc_listallpids(std::ptr::null_mut(), 0) };
    if estimate <= 0 {
        return Err(std::io::Error::last_os_error());
    }

    for attempt in 0..4i32 {
        // Headroom grows quadratically; the table can grow between calls.
        let extra = 16 + attempt as usize * attempt as usize * 64;
        let capacity = estimate.max(16) as usize + extra;
        let mut pids: Vec<i32> = vec![0; capacity];

        // SAFETY: the buffer is `capacity * size_of::<i32>()` bytes and the
        // length is passed in bytes, as the API requires.
        let count = unsafe {
            libc::proc_listallpids(pids.as_mut_ptr().cast::<c_void>(), (capacity * elem) as i32)
        };
        if count <= 0 {
            return Err(std::io::Error::last_os_error());
        }

        let count = (count as usize).min(capacity);
        if count == capacity {
            continue; // probably truncated: retry with more headroom
        }

        return Ok(pids[..count]
            .iter()
            .filter(|p| **p > 0)
            .map(|p| *p as u32)
            .collect());
    }

    Err(std::io::Error::other(
        "proc_listallpids kept returning a full buffer",
    ))
}

/// BSD + task info for one process, from a single `PROC_PIDTASKALLINFO` call.
#[derive(Copy, Clone, Debug, Default)]
pub struct TaskAllInfo {
    pub bsd: KInfoProc,
    pub task: TaskInfo,
}

/// Fetch everything `libproc` can tell us about one process.
///
/// `EPERM`/`ESRCH` are normal: system processes are not readable by a normal
/// user, and processes die mid-scan.
pub fn task_all_info(pid: u32) -> Option<TaskAllInfo> {
    let mut info = std::mem::MaybeUninit::<libc::proc_taskallinfo>::zeroed();
    let want = std::mem::size_of::<libc::proc_taskallinfo>() as c_int;
    // SAFETY: the buffer is exactly `size_of::<proc_taskallinfo>()` bytes and
    // writable; the kernel writes that many and returns the byte count.
    let got = unsafe {
        libc::proc_pidinfo(
            pid as c_int,
            libc::PROC_PIDTASKALLINFO,
            0,
            info.as_mut_ptr().cast::<c_void>(),
            want,
        )
    };
    if got != want {
        return None;
    }
    // SAFETY: `got == want` means the whole struct was initialised.
    let info = unsafe { info.assume_init() };

    let b = &info.pbsd;
    let mut comm = [0u8; 17];
    for (dst, src) in comm.iter_mut().zip(b.pbi_comm.iter()) {
        *dst = *src as u8;
        if *src == 0 {
            break;
        }
    }

    Some(TaskAllInfo {
        bsd: KInfoProc {
            pid: b.pbi_pid,
            ppid: b.pbi_ppid,
            uid: b.pbi_uid,
            nice: b.pbi_nice,
            p_stat: b.pbi_status as i8,
            start_sec: b.pbi_start_tvsec,
            start_usec: b.pbi_start_tvusec,
            pgrp: b.pbi_pgid,
            // `e_tpgid` is `pid_t`, i.e. signed: `-1` means "no foreground job"
            // and must not be widened to a huge unsigned value.
            tpgid: b.e_tpgid as i32 as i64,
            // `e_tdev` is `dev_t`, unsigned; the "no terminal" value is all ones.
            tty_dev: if b.e_tdev == u32::MAX {
                0
            } else {
                b.e_tdev as i64
            },
            comm,
        },
        task: TaskInfo {
            user_ns: info.ptinfo.pti_total_user,
            system_ns: info.ptinfo.pti_total_system,
            virtual_size: info.ptinfo.pti_virtual_size,
            resident_size: info.ptinfo.pti_resident_size,
            faults: info.ptinfo.pti_faults as u32,
            threads: info.ptinfo.pti_threadnum as u32,
            running_threads: info.ptinfo.pti_numrunning as u32,
            priority: info.ptinfo.pti_priority,
        },
    })
}

/// One thread of a process, from `proc_pidinfo(PROC_PIDTHREADINFO)`.
///
/// `pth_user_time`/`pth_system_time` are **nanoseconds**, unlike
/// `proc_taskinfo.pti_total_user`, which is in Mach absolute ticks. Verified by
/// burning CPU for two seconds on Apple Silicon: the thread sums came to 1.141 s
/// while `pti_total_user * 125/3` came to the same 1.141 s. Scaling these through
/// [`super::units::mach_ticks_to_nanos`] would therefore inflate them 41x.
#[derive(Copy, Clone, Debug)]
pub struct ThreadInfo {
    /// Opaque per-thread handle used as the `arg` of `PROC_PIDTHREADINFO`.
    ///
    /// **Not globally unique.** Every process reports the same handle for its
    /// main thread (`8681251360` on this machine, for the shell, `fork` children
    /// and this test binary alike), so it is only an identity *within* a pid.
    pub handle: u64,
    /// `pth_name`, empty for most threads (54% were named on a desktop).
    pub name: [u8; 64],
    pub user_ns: u64,
    pub system_ns: u64,
    /// `pth_run_state`: 1 running, 2 stopped, 3 waiting, 4 uninterruptible.
    pub run_state: i32,
}

impl Default for ThreadInfo {
    fn default() -> Self {
        Self {
            handle: 0,
            name: [0; 64],
            user_ns: 0,
            system_ns: 0,
            run_state: 0,
        }
    }
}

impl ThreadInfo {
    /// Thread name as a lossy string, empty when the thread is unnamed.
    pub fn name_str(&self) -> &str {
        let len = self.name.iter().position(|b| *b == 0).unwrap_or(self.name.len());
        // `pth_name` is a `char[64]`; it is not guaranteed UTF-8 but always is
        // in practice (it comes from `pthread_setname_np`).
        std::str::from_utf8(&self.name[..len]).unwrap_or("")
    }
}

/// List the thread handles of one process (`proc_pidinfo(PROC_PIDLISTTHREADS)`).
///
/// Returns `None` when the process cannot be inspected at all — which is the
/// normal case for other users' processes, since `proc_pidinfo` denies them. An
/// empty `Some` means the process genuinely has no listable threads.
///
/// # Ordering and zeroes
///
/// Two properties of the kernel's reply matter to the caller and are preserved
/// here rather than papered over:
///
/// * **The first entry is the main thread.** Verified against
///   `pthread_mach_thread_np(pthread_self())` + `THREAD_IDENTIFIER_INFO` on a
///   three-thread process: `raw[0]` was the main thread's handle every time. The
///   caller skips it because it is already the process row.
/// * **A slot can be `0` even in the middle of the list.** `pid 508`
///   (`CarbonComponentScannerXPC`) reports `[0, 13046116576, 0]` with
///   `pti_threadnum == 3`: its main thread has no readable handle at all. So the
///   main thread is identified by *position*, never by being the first non-zero
///   entry, and zeroes are only dropped from the tail.
///
/// The kernel returns the *buffer* size when the buffer is large enough, not the
/// number of threads, and zero-fills the unused tail. A full buffer therefore has
/// to be retried with more room rather than treated as exact.
pub fn list_threads(pid: u32) -> Option<Vec<u64>> {
    // No documented upper bound on the thread count, and the kernel does not
    // report a size hint here (a null buffer returns 0), so grow on demand.
    for capacity in [64usize, 512, 4096, 16384] {
        let mut buf = vec![0u64; capacity];
        let want = (capacity * std::mem::size_of::<u64>()) as c_int;
        // SAFETY: `buf` has `want` writable bytes and `want` is that length; the
        // kernel writes at most `want` bytes and returns the byte count.
        let got = unsafe {
            libc::proc_pidinfo(
                pid as c_int,
                PROC_PIDLISTTHREADS,
                0,
                buf.as_mut_ptr().cast::<c_void>(),
                want,
            )
        };
        if got < 0 {
            return None;
        }
        if got == 0 {
            // Either the process is not ours, or it has no threads. Distinguish
            // by asking for the task info: a readable process always has >= 1.
            return scan_task_thread_count(pid).map(|_| Vec::new());
        }
        let count = got as usize / std::mem::size_of::<u64>();
        // A full buffer means the thread count may have been truncated.
        if count < capacity {
            buf.truncate(count);
            // Drop only the zero-filled tail: a zero *before* a real handle is a
            // thread whose handle the kernel would not give us, and removing it
            // would shift every later thread's position, including the main
            // thread's. Position is what identifies the main thread.
            while buf.last() == Some(&0) {
                buf.pop();
            }
            return Some(buf);
        }
    }
    None
}

/// `PROC_PIDLISTTHREADS` flavor number, absent from `libc`'s constants.
const PROC_PIDLISTTHREADS: c_int = 6;

/// Thread count from `proc_taskinfo`, used only to tell "unreadable" from
/// "empty" in [`list_threads`].
fn scan_task_thread_count(pid: u32) -> Option<u32> {
    task_all_info(pid).map(|info| info.task.threads)
}

/// Everything `proc_pidinfo` reports about one thread.
pub fn thread_info(pid: u32, handle: u64) -> Option<ThreadInfo> {
    let mut info = std::mem::MaybeUninit::<libc::proc_threadinfo>::zeroed();
    let want = std::mem::size_of::<libc::proc_threadinfo>() as c_int;
    // SAFETY: the buffer is exactly `want` bytes and writable; the kernel writes
    // that many and returns the byte count, or <= 0 on failure.
    let got = unsafe {
        libc::proc_pidinfo(
            pid as c_int,
            libc::PROC_PIDTHREADINFO,
            handle,
            info.as_mut_ptr().cast::<c_void>(),
            want,
        )
    };
    if got != want {
        return None;
    }
    // SAFETY: `got == want` means the whole struct was initialised.
    let info = unsafe { info.assume_init() };

    let mut name = [0u8; 64];
    for (dst, src) in name.iter_mut().zip(info.pth_name.iter()) {
        *dst = *src as u8;
    }

    Some(ThreadInfo {
        handle,
        name,
        user_ns: info.pth_user_time,
        system_ns: info.pth_system_time,
        run_state: info.pth_run_state,
    })
}

/// Per-process task info from `proc_pidinfo(PROC_PIDTASKINFO)`.
#[derive(Copy, Clone, Debug, Default)]
pub struct TaskInfo {
    /// Monotonic nanoseconds of CPU time.
    pub user_ns: u64,
    pub system_ns: u64,
    pub virtual_size: u64,
    pub resident_size: u64,
    pub faults: u32,
    pub threads: u32,
    pub running_threads: u32,
    pub priority: i32,
}

/// Task counters from the same libproc flavor used by htop's Darwin backend.
pub fn task_info(pid: u32) -> Option<TaskInfo> {
    let mut info = std::mem::MaybeUninit::<libc::proc_taskinfo>::zeroed();
    let want = std::mem::size_of::<libc::proc_taskinfo>() as c_int;
    // SAFETY: the buffer and size match PROC_PIDTASKINFO's output structure.
    let got = unsafe {
        libc::proc_pidinfo(
            pid as c_int,
            libc::PROC_PIDTASKINFO,
            0,
            info.as_mut_ptr().cast(),
            want,
        )
    };
    if got != want {
        return None;
    }
    // SAFETY: libproc returned the full initialized structure.
    let info = unsafe { info.assume_init() };
    Some(TaskInfo {
        user_ns: info.pti_total_user,
        system_ns: info.pti_total_system,
        virtual_size: info.pti_virtual_size,
        resident_size: info.pti_resident_size,
        faults: info.pti_faults as u32,
        threads: info.pti_threadnum as u32,
        running_threads: info.pti_numrunning as u32,
        priority: info.pti_priority,
    })
}

/// The executable path of a process, if readable.
pub fn exe_path(pid: u32) -> Option<String> {
    let mut buf = [0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    // SAFETY: the buffer is sized to PROC_PIDPATHINFO_MAXSIZE as required; the
    // call writes a NUL-terminated path and returns its length, or <= 0.
    let len = unsafe {
        libc::proc_pidpath(
            pid as c_int,
            buf.as_mut_ptr().cast::<c_void>(),
            buf.len() as u32,
        )
    };
    if len <= 0 {
        return None;
    }
    let cstr = CStr::from_bytes_until_nul(&buf).ok()?;
    Some(cstr.to_string_lossy().into_owned())
}

/// The working directory of a process, if readable.
///
/// `proc_pidinfo(PROC_PIDVNODEPATHINFO)` returns both the current and the root
/// directory; `pvi_cdir.vip_path` is the one `ps`, `lsof` and htop call cwd.
/// The buffer needs no length juggling: the kernel fills a fixed-size
/// `proc_vnodepathinfo`, and since the string lives at a fixed offset this simply
/// reads it out of the raw struct rather than defining one. `libc` *does* define the
/// struct, so it is used directly here.
pub fn cwd_path(pid: u32) -> Option<String> {
    let mut info = std::mem::MaybeUninit::<libc::proc_vnodepathinfo>::zeroed();
    let want = std::mem::size_of::<libc::proc_vnodepathinfo>() as c_int;
    // SAFETY: `info` is `want` bytes, writable, and the kernel writes exactly
    // `want` bytes or returns a different count.
    let got = unsafe {
        libc::proc_pidinfo(
            pid as c_int,
            libc::PROC_PIDVNODEPATHINFO,
            0,
            info.as_mut_ptr().cast::<c_void>(),
            want,
        )
    };
    if got != want {
        return None;
    }
    // SAFETY: `got == want` means every byte was written by the kernel.
    let info = unsafe { info.assume_init() };

    // `vip_path` is `[c_char; MAXPATHLEN]` split into 32-byte chunks by libc, so
    // flatten it back before looking for the terminator.
    let mut path: Vec<u8> = Vec::with_capacity(32 * 32);
    for chunk in info.pvi_cdir.vip_path.iter() {
        for byte in chunk.iter() {
            let b = *byte as u8;
            if b == 0 {
                let text = String::from_utf8_lossy(&path).into_owned();
                return (!text.is_empty()).then_some(text);
            }
            path.push(b);
        }
    }
    let text = String::from_utf8_lossy(&path).into_owned();
    (!text.is_empty()).then_some(text)
}

/// Disk I/O counters from `proc_pid_rusage`.
///
/// **Not wired into the sampler.** No column displays these yet; the call site that
/// used to read them existed only to fill two `#[allow(dead_code)]` fields and was
/// removed. What is kept is the mechanism, because it is verified against a live
/// kernel and a future `IO_READ_RATE` column would otherwise be rewritten from the
/// man page by someone who cannot run macOS. `crates/core/tests/` exercises it
/// through `rusage_is_readable_for_self`.
///
/// `ri_diskio_bytesread`/`written` are bytes actually hitting the block layer, not
/// bytes returned by `read(2)` — a page-cache hit increments neither. That is the
/// distinction htop's `RBYTES` makes on Linux and the one a column would have to
/// state in its description. See `docs/ROADMAP.md` § M5.
#[derive(Copy, Clone, Debug, Default)]
pub struct Rusage {
    pub disk_read: u64,
    pub disk_written: u64,
}

pub fn rusage(pid: u32) -> Option<Rusage> {
    let mut ri = std::mem::MaybeUninit::<libc::rusage_info_v2>::zeroed();
    // SAFETY: the flavour constant matches the struct type passed in.
    let rc = unsafe {
        libc::proc_pid_rusage(
            pid as c_int,
            libc::RUSAGE_INFO_V2,
            ri.as_mut_ptr().cast::<libc::rusage_info_t>(),
        )
    };
    if rc != 0 {
        return None;
    }
    // SAFETY: a zero return means the struct was filled.
    let ri = unsafe { ri.assume_init() };
    Some(Rusage {
        disk_read: ri.ri_diskio_bytesread,
        disk_written: ri.ri_diskio_byteswritten,
    })
}

/// `KERN_PROCARGS2` for one process.
pub fn procargs2(pid: u32) -> Option<Vec<u8>> {
    let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid as c_int];
    let mut size: usize = crate::darwin::parse::ARG_MAX_CAP;
    let mut buf = vec![0u8; size];

    // SAFETY: the buffer has exactly `size` bytes; the kernel clamps to it.
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
    if rc != 0 || size < 4 {
        return None;
    }
    buf.truncate(size);
    Some(buf)
}

/// String-valued `sysctlbyname`, for `vm.swapusage` and friends.
pub fn sysctl_string(name: &CStr) -> Option<String> {
    let mut size: usize = 0;
    // SAFETY: a null buffer asks for the size only.
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
    let mut buf = vec![0u8; size];
    // SAFETY: the buffer is `size` bytes, matching the size query.
    let rc = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            buf.as_mut_ptr().cast::<c_void>(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 {
        return None;
    }
    buf.truncate(size);
    while buf.last() == Some(&0) {
        buf.pop();
    }
    Some(String::from_utf8_lossy(&buf).into_owned())
}

/// `u64`-valued `sysctlbyname`, for `hw.memsize` and similar.
pub fn sysctl_u64(name: &CStr) -> Option<u64> {
    let mut value: u64 = 0;
    let mut size = std::mem::size_of::<u64>();
    // SAFETY: the destination is exactly `size` bytes and matches the type the
    // named sysctl returns for these keys.
    let rc = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            (&mut value as *mut u64).cast::<c_void>(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc == 0 {
        Some(value)
    } else {
        None
    }
}

/// Logical CPU count.
pub fn cpu_count() -> usize {
    static NAME: &CStr = c"hw.logicalcpu";
    sysctl_u64(NAME).unwrap_or(1).max(1) as usize
}

/// Total physical memory in bytes.
pub fn total_memory() -> u64 {
    static NAME: &CStr = c"hw.memsize";
    sysctl_u64(NAME).unwrap_or(0)
}

/// Swap usage, from `vm.swapusage`.
///
/// The sysctl returns a binary `struct xsw_usage`, not text. `sysctl(8)` prints
/// it formatted on the command line, which is an easy trap: reading it with
/// `sysctlbyname` into a string buffer yields binary garbage.
pub fn swap_usage() -> crate::system::SwapInfo {
    let mut usage = std::mem::MaybeUninit::<libc::xsw_usage>::zeroed();
    let mut size = std::mem::size_of::<libc::xsw_usage>();
    static NAME: &CStr = c"vm.swapusage";

    // SAFETY: the destination is exactly `size_of::<xsw_usage>()` bytes, and the
    // named sysctl returns that struct.
    let rc = unsafe {
        libc::sysctlbyname(
            NAME.as_ptr(),
            usage.as_mut_ptr().cast::<c_void>(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 {
        return crate::system::SwapInfo::default();
    }
    // SAFETY: a zero return means the struct was filled.
    let usage = unsafe { usage.assume_init() };

    crate::system::SwapInfo {
        total: usage.xsu_total,
        free: usage.xsu_avail,
        used: usage.xsu_used,
    }
}

/// Host-wide network byte counters from `sysctl(NET_RT_IFLIST2)`.
///
/// # Why not `getifaddrs`
///
/// `getifaddrs` works and is what most code uses, but on 64-bit Darwin it hands
/// back `struct if_data`, whose `ifi_ibytes` / `ifi_obytes` are **32-bit**. A
/// machine that has moved more than 4 GiB wraps those counters constantly, and
/// the wrap is indistinguishable from a genuine counter reset, so the meter
/// would spike to zero every few seconds on any active link.
///
/// `NET_RT_IFLIST2` returns `if_msghdr2` with a `struct if_data64` — 64-bit
/// counters, no wrap. It is a routing-socket dump, so the reply is a sequence of
/// variable-length messages that have to be walked by `ifm_msglen`, not a flat
/// array. A zero `ifm_msglen` would loop forever, so it is treated as the end.
///
/// Loopback is skipped for the same reason as on Linux: it never reaches a wire.
pub fn net_counters() -> crate::system::NetCounters {
    let mut mib = [libc::CTL_NET, libc::PF_ROUTE, 0, 0, libc::NET_RT_IFLIST2, 0];

    // Size first. The routing table changes between calls, so a short reply is
    // retried rather than truncated.
    let mut len: usize = 0;
    // SAFETY: a null buffer with a zero length is the documented way to ask for
    // the size of the reply.
    let rc = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            mib.len() as u32,
            std::ptr::null_mut(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 || len == 0 {
        return crate::system::NetCounters::default();
    }

    for _ in 0..4 {
        // Headroom: interfaces can be added between the size query and the read.
        let mut buf = vec![0u8; len + 1024];
        let mut got = buf.len();
        // SAFETY: the buffer is `got` bytes and `got` is passed as its length.
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
        if rc != 0 {
            return crate::system::NetCounters::default();
        }
        if got >= buf.len() {
            // Filled the buffer exactly: the reply was truncated, retry larger.
            len = got;
            continue;
        }
        buf.truncate(got);
        return parse_iflist2(&buf);
    }
    crate::system::NetCounters::default()
}

/// Walk the `NET_RT_IFLIST2` reply, summing the non-loopback interfaces.
fn parse_iflist2(buf: &[u8]) -> crate::system::NetCounters {
    let mut out = crate::system::NetCounters::default();
    let mut offset = 0usize;

    while offset + std::mem::size_of::<libc::if_msghdr2>() <= buf.len() {
        // The messages are packed and *unaligned*: `if_msghdr2` is declared
        // `repr(packed(4))`, and consecutive messages are not 8-byte aligned. A
        // direct reference would be UB on a strict-alignment target, so copy it
        // out instead of casting.
        let mut hdr = std::mem::MaybeUninit::<libc::if_msghdr2>::uninit();
        // SAFETY: `offset + size_of::<if_msghdr2>() <= buf.len()` is the loop
        // condition, so the copy stays inside the buffer.
        unsafe {
            std::ptr::copy_nonoverlapping(
                buf.as_ptr().add(offset),
                hdr.as_mut_ptr().cast::<u8>(),
                std::mem::size_of::<libc::if_msghdr2>(),
            );
        }
        // SAFETY: every byte was initialised by the copy above.
        let hdr = unsafe { hdr.assume_init() };

        let msglen = hdr.ifm_msglen as usize;
        if msglen == 0 {
            break; // malformed; stop rather than spin
        }

        // Only interface-info messages carry the counters; the reply also
        // contains address and other messages that must simply be skipped.
        if hdr.ifm_type == libc::RTM_IFINFO2 as u8 {
            // The packed struct's field reads are fine; taking a *reference* to
            // one would not be.
            let data = hdr.ifm_data;
            let loopback = hdr.ifm_flags & libc::IFF_LOOPBACK != 0;
            if !loopback {
                out.rx_bytes = out.rx_bytes.saturating_add(data.ifi_ibytes);
                out.tx_bytes = out.tx_bytes.saturating_add(data.ifi_obytes);
            }
        }

        offset = offset.saturating_add(msglen);
    }

    out
}

/// Memory page size reported by the VM subsystem.
pub fn page_size() -> u64 {    // SAFETY: plain static, initialised by dyld.
    let v = unsafe { libc::vm_page_size };
    if v > 0 {
        v as u64
    } else {
        16_384
    }
}

/// Per-core CPU tick counters, plus the aggregate.
///
/// `host_processor_info` allocates a Mach array that the caller must release with
/// `vm_deallocate`; forgetting that leaks a page per sample.
pub fn cpu_ticks() -> Vec<crate::system::CpuTimes> {
    let mut info: libc::processor_info_array_t = std::ptr::null_mut();
    let mut count: libc::mach_msg_type_number_t = 0;
    let mut ncpu: libc::natural_t = 0;

    // SAFETY: `mach_host_self()` returns the host port for this process and does
    // not fail. The out-parameters are valid for the duration of the call.
    let host = unsafe { libc::mach_host_self() };
    // SAFETY: all four out-parameters point at live locals of the right types.
    let rc = unsafe {
        libc::host_processor_info(
            host,
            libc::PROCESSOR_CPU_LOAD_INFO,
            &mut ncpu,
            &mut info,
            &mut count,
        )
    };
    if rc != 0 || info.is_null() || ncpu == 0 {
        return Vec::new();
    }

    let per_cpu = libc::CPU_STATE_MAX as usize;
    let mut out = Vec::with_capacity(ncpu as usize + 1);

    // SAFETY: the kernel allocated `count` `integer_t`s in the Mach array.
    let ticks = unsafe { std::slice::from_raw_parts(info, count as usize) };

    let mut aggregate = crate::system::CpuTimes::default();
    for core in 0..ncpu as usize {
        let base = core * per_cpu;
        if base + per_cpu > ticks.len() {
            break;
        }
        let user = ticks[base + libc::CPU_STATE_USER as usize] as u64;
        let system = ticks[base + libc::CPU_STATE_SYSTEM as usize] as u64;
        let idle = ticks[base + libc::CPU_STATE_IDLE as usize] as u64;
        let nice = ticks[base + libc::CPU_STATE_NICE as usize] as u64;

        out.push(crate::system::CpuTimes {
            user,
            system,
            idle,
            nice,
            ..Default::default()
        });

        aggregate.user += user;
        aggregate.system += system;
        aggregate.idle += idle;
        aggregate.nice += nice;
    }

    // SAFETY: the array came from `host_processor_info`, which allocates with
    // `vm_allocate` in the caller's address space; `count * sizeof(integer_t)` is
    // exactly what must be released.
    unsafe {
        libc::vm_deallocate(
            libc::mach_task_self(),
            info as libc::vm_address_t,
            (count as usize * std::mem::size_of::<libc::integer_t>()) as libc::vm_size_t,
        );
    }

    out.insert(0, aggregate);
    out
}

/// Memory page counts from `host_statistics64`, converted to bytes.
///
/// The class semantics that matter for accounting (verified against
/// `top`/Activity Monitor on macOS 15):
///
/// * `internal_page_count` counts anonymous pages and **already includes** the
///   wired and compressed pages.
/// * `external_page_count` is the file cache and is reclaimable.
/// * `inactive_count` holds dirty anonymous pages, not only cache.
#[derive(Copy, Clone, Debug, Default)]
pub struct VmStats {
    pub free: u64,
    pub active: u64,
    pub inactive: u64,
    pub wired: u64,
    pub speculative: u64,
    pub compressed: u64,
    pub purgeable: u64,
    pub internal: u64,
    pub external: u64,
}

pub fn vm_stats() -> VmStats {
    let mut stats = std::mem::MaybeUninit::<libc::vm_statistics64>::zeroed();
    let mut count = libc::HOST_VM_INFO64_COUNT;

    // SAFETY: `mach_host_self()` is valid; the out-parameters match the flavour.
    let host = unsafe { libc::mach_host_self() };
    // SAFETY: `count` is initialised to the expected element count and the buffer
    // is large enough for `vm_statistics64`.
    let rc = unsafe {
        libc::host_statistics64(
            host,
            libc::HOST_VM_INFO64,
            stats.as_mut_ptr().cast::<libc::integer_t>(),
            &mut count,
        )
    };
    if rc != 0 {
        return VmStats::default();
    }
    // SAFETY: a zero return means the struct was filled.
    let stats = unsafe { stats.assume_init() };
    let page = page_size();
    let b = |pages: u64| pages.saturating_mul(page);

    VmStats {
        free: b(stats.free_count as u64),
        active: b(stats.active_count as u64),
        inactive: b(stats.inactive_count as u64),
        wired: b(stats.wire_count as u64),
        speculative: b(stats.speculative_count as u64),
        compressed: b(stats.compressor_page_count as u64),
        purgeable: b(stats.purgeable_count as u64),
        internal: b(stats.internal_page_count as u64),
        external: b(stats.external_page_count as u64),
    }
}

/// 1/5/15-minute load average.
pub fn loadavg() -> [f64; 3] {
    let mut loads = [0.0f64; 3];
    // SAFETY: the slice has exactly the three elements `getloadavg` fills.
    let n = unsafe { libc::getloadavg(loads.as_mut_ptr(), 3) };
    if n == 3 {
        loads
    } else {
        [0.0; 3]
    }
}

/// Wall-clock seconds since the Unix epoch.
pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Seconds since boot, from `kern.boottime`.
pub fn uptime_secs() -> u64 {
    let mut tv = libc::timeval {
        tv_sec: 0,
        tv_usec: 0,
    };
    let mut size = std::mem::size_of::<libc::timeval>();
    let mut mib = [libc::CTL_KERN, libc::KERN_BOOTTIME];
    // SAFETY: the destination is a live `timeval` of the declared size.
    let rc = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            mib.len() as u32,
            (&mut tv as *mut libc::timeval).cast::<c_void>(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 || tv.tv_sec <= 0 {
        return 0;
    }
    now_secs().saturating_sub(tv.tv_sec as u64)
}

/// Send a signal to a process.
pub fn kill(pid: u32, signal: i32) -> std::io::Result<()> {
    // SAFETY: `kill` takes only integers.
    let rc = unsafe { libc::kill(pid as libc::pid_t, signal as c_int) };
    if rc == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

/// Change a process's nice value.
pub fn setpriority(pid: u32, nice: i32) -> std::io::Result<()> {
    // SAFETY: `setpriority` takes only integers.
    let rc = unsafe { libc::setpriority(libc::PRIO_PROCESS, pid, nice) };
    if rc == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

/// Read a process's current nice value.
pub fn getpriority(pid: u32) -> i32 {
    // SAFETY: `getpriority` takes only integers.
    unsafe { libc::getpriority(libc::PRIO_PROCESS, pid) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_process_list_is_non_empty() {
        let pids = list_pids().expect("proc_listallpids must work");
        assert!(!pids.is_empty());
        // pid 1 always exists.
        assert!(pids.contains(&1));
        let mut sorted = pids.clone();
        sorted.dedup();
        assert_eq!(sorted.len(), pids.len(), "no duplicate pids");
    }

    #[test]
    fn self_task_info_is_readable() {
        let me = std::process::id();
        let info = task_all_info(me).expect("own task info is always readable");
        assert_eq!(info.bsd.pid, me);
        assert!(info.bsd.ppid > 0);
        assert!(info.task.resident_size > 0);
        assert!(info.task.threads >= 1);
        assert!(info.task.virtual_size >= info.task.resident_size);
        assert!(info.bsd.start_sec > 0, "start time must be known");
    }

    #[test]
    fn self_uid_matches_the_real_uid() {
        let me = std::process::id();
        let info = task_all_info(me).unwrap();
        // SAFETY: `getuid` has no preconditions.
        let uid = unsafe { libc::getuid() };
        assert_eq!(info.bsd.uid, uid);
    }

    #[test]
    fn self_exe_path_is_absolute() {
        let me = std::process::id();
        let path = exe_path(me).expect("own exe is readable");
        assert!(path.starts_with('/'), "got {path:?}");
    }

    #[test]
    fn rusage_is_readable_for_self() {
        // The only caller of `rusage` is this test, by design: the figures have no
        // column yet. Asserting it *reads* is the whole point — it proves the
        // `RUSAGE_INFO_V2` struct layout and the flavour constant still match the
        // running kernel, which is exactly what an OS update breaks.
        //
        // Deliberately not asserted: that the counters are non-zero. A test binary
        // served from the page cache has done no block I/O at all, so
        // `disk_read == disk_written == 0` is the *correct* answer here — an
        // earlier version of this test asserted otherwise and failed, which is the
        // same "asserting the machine instead of the contract" mistake that broke
        // CI on v0.6.0. The contract is "the syscall succeeds and the struct is
        // readable", and that is what is checked.
        let me = std::process::id();
        let io = rusage(me).expect("own rusage is readable");
        // Values are cumulative and only ever grow; negative is impossible in the
        // type, so the only meaningful check left is that nothing panicked while
        // reading them, which reaching this line proves.
        println!("rusage: read={} written={}", io.disk_read, io.disk_written);
    }

    #[test]
    fn self_procargs2_mentions_the_test_binary() {
        let me = std::process::id();
        let raw = procargs2(me).expect("own KERN_PROCARGS2 is readable");
        let cmd = crate::darwin::parse::parse_procargs2(&raw).expect("must parse");
        assert!(!cmd.is_empty());
    }

    #[test]
    fn total_memory_is_plausible() {
        let m = total_memory();
        assert!(m > 256 * 1024 * 1024, "got {m}");
    }

    #[test]
    fn cpu_ticks_include_aggregate() {
        let ticks = cpu_ticks();
        assert!(ticks.len() >= 2, "aggregate plus at least one core");
        assert!(ticks[0].total() > 0);
        let cores = cpu_count();
        assert_eq!(ticks.len(), cores + 1, "one aggregate plus every core");
    }

    #[test]
    fn vm_stats_are_non_zero() {
        let vm = vm_stats();
        assert!(vm.free > 0 || vm.active > 0);
    }

    #[test]
    fn loadavg_is_sane() {
        let l = loadavg();
        assert!(l[0] >= 0.0);
    }

    #[test]
    fn uptime_is_positive() {
        assert!(uptime_secs() > 0);
    }

    #[test]
    fn swap_usage_is_plausible() {
        let swap = swap_usage();
        // A machine with swap disabled (or one that has never swapped) reports
        // all zeros, so `total > 0` is not a valid invariant.
        if swap.total == 0 {
            assert_eq!(swap.used, 0, "no swap total means nothing can be in use");
            assert_eq!(swap.free, 0);
        } else {
            assert!(swap.used <= swap.total, "used exceeds total");
            assert_eq!(swap.total, swap.used + swap.free);
        }
    }

    #[test]
    fn nonexistent_pid_yields_none() {
        assert!(
            task_all_info(4_194_303).is_none(),
            "pid 4_194_303 cannot exist"
        );
    }
}
