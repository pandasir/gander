//! Reading `sysctl(KERN_PROC_ALL)` directly.
//!
//! `libc` does not expose Darwin's `kinfo_proc`, and `proc_pidinfo` refuses to
//! describe processes owned by other users, so `PROC_PIDTASKALLINFO` alone can
//! only see about 60% of the process table. `sysctl(KERN_PROC_ALL)` is not
//! restricted the same way, which is why htop uses it
//! (`darwin/DarwinProcessTable.c: ProcessTable_getKInfoProcs`).
//!
//! Defining the full struct is an ABI hazard, so this module does not: it reads
//! the fixed-size records and extracts the handful of fields we need by offset.
//! The offsets were derived on arm64 macOS by locating `p_comm` (`"launchd"`),
//! then cross-checking `p_pid`, `p_ppid`, `p_uid` and `p_starttime` against
//! `ps(1)` output; [`validate`] re-runs those checks at startup so a layout
//! change is caught loudly instead of producing silently wrong numbers.

use libc::{c_int, c_void};

/// `struct kinfo_proc` stride on 64-bit Darwin.
pub const STRIDE: usize = 648;

// Offsets inside one record, taken from the SDK's own `offsetof` output
// (`sizeof(struct kinfo_proc) == 648`, identical on the 15 and 26 SDKs) and
// cross-checked against ps(1) at runtime by `validate`:
//   p_un.__p_starttime.tv_sec  at 0
//   p_flag                     at 32  (P_* bits, not a state)
//   p_stat                     at 36  (S* state)
//   p_pid                      at 40
//   p_nice                     at 242
//   p_comm                     at 243
//   kp_eproc.e_pcred.p_ruid    at 392 (real uid, what `USER` means)
//   kp_eproc.e_ucred.cr_uid    at 420 (effective uid; differs for setuid execs)
//   kp_eproc.e_ppid            at 560
const OFF_STARTTIME_SEC: usize = 0;
const OFF_PID: usize = 40;
const OFF_PPID: usize = 560;
const OFF_NICE: usize = 242;
/// `p_stat`. Reading offset 32 instead yields `p_flag`, whose low byte is
/// always `P_INMEM`, so every process looked stopped.
const OFF_STAT: usize = 36;
const OFF_COMM: usize = 243;
const COMM_LEN: usize = 17;
/// `_pcred.p_ruid`: the real uid, matching Linux's `Uid:` first field and
/// `ps`'s `UID` column. `e_ucred.cr_uid` (420) is the effective uid and only
/// differs for setuid binaries.
const OFF_UID: usize = 392;

/// One process as reported by `KERN_PROC_ALL`.
#[derive(Copy, Clone, Debug, Default)]
pub struct RawProc {
    pub pid: u32,
    pub ppid: u32,
    pub uid: u32,
    pub nice: i32,
    pub p_stat: i8,
    pub start_sec: u64,
    pub comm: [u8; COMM_LEN],
}

impl RawProc {
    /// Command name as a lossy string.
    pub fn comm_str(&self) -> String {
        let len = self.comm.iter().position(|b| *b == 0).unwrap_or(COMM_LEN);
        String::from_utf8_lossy(&self.comm[..len]).into_owned()
    }
}

fn read_i32(b: &[u8], off: usize) -> i32 {
    i32::from_ne_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]])
}

fn read_u32(b: &[u8], off: usize) -> u32 {
    u32::from_ne_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]])
}

fn read_u64(b: &[u8], off: usize) -> u64 {
    u64::from_ne_bytes([
        b[off],
        b[off + 1],
        b[off + 2],
        b[off + 3],
        b[off + 4],
        b[off + 5],
        b[off + 6],
        b[off + 7],
    ])
}

fn decode(rec: &[u8]) -> Option<RawProc> {
    let pid = read_i32(rec, OFF_PID);
    if pid <= 0 {
        return None;
    }
    let mut comm = [0u8; COMM_LEN];
    comm.copy_from_slice(&rec[OFF_COMM..OFF_COMM + COMM_LEN]);

    Some(RawProc {
        pid: pid as u32,
        ppid: read_i32(rec, OFF_PPID).max(0) as u32,
        uid: read_u32(rec, OFF_UID),
        nice: rec[OFF_NICE] as i8 as i32,
        p_stat: rec[OFF_STAT] as i8,
        start_sec: read_u64(rec, OFF_STARTTIME_SEC),
        comm,
    })
}

/// Fetch the whole process table.
///
/// Two things make this awkward. First, the kernel's size hint races with
/// process creation, so the first attempt can always come back short. Second,
/// recent macOS returns `ENOMEM` instead of a partial result when the buffer is
/// too small, so a hard-coded starting capacity fails outright on a busy
/// machine (it used to start at 512 records, which is below a two-hundredth of
/// a desktop's table). Both cases are handled by growing the buffer and
/// retrying.
pub fn list_processes() -> std::io::Result<Vec<RawProc>> {
    let mut mib = [
        libc::CTL_KERN,
        libc::KERN_PROC,
        libc::KERN_PROC_ALL,
        0 as c_int,
    ];

    // The size query itself needs no buffer, and its answer is a byte count.
    let mut hint = 0usize;
    // SAFETY: a null `oldp` makes the call a pure size query; `hint` is writable.
    let rc = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            mib.len() as u32,
            std::ptr::null_mut(),
            &mut hint,
            std::ptr::null_mut(),
            0,
        )
    };
    // A failed size query is not fatal: fall back to the old starting point.
    let mut capacity = if rc == 0 && hint > 0 {
        hint.div_ceil(STRIDE) + 64
    } else {
        1024
    };

    for _ in 0..6 {
        let mut buf = vec![0u8; capacity * STRIDE];
        let mut len = buf.len();

        // SAFETY: `buf` has `capacity * STRIDE` writable bytes and `len` carries
        // that length; the kernel writes at most `len` bytes back.
        let rc = unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                mib.len() as u32,
                buf.as_mut_ptr().cast::<c_void>(),
                &mut len,
                std::ptr::null_mut(),
                0,
            )
        };
        if rc != 0 {
            let err = std::io::Error::last_os_error();
            // ENOMEM means "not enough room", not "out of memory": grow and
            // retry. Anything else is a real failure.
            if err.raw_os_error() == Some(libc::ENOMEM) {
                capacity *= 2;
                continue;
            }
            return Err(err);
        }

        let records = len / STRIDE;
        // A full buffer means the table grew mid-call; retry with more room.
        if records >= capacity {
            capacity *= 2;
            continue;
        }

        buf.truncate(records * STRIDE);
        let mut out = Vec::with_capacity(records);
        for rec in buf.chunks_exact(STRIDE) {
            if let Some(p) = decode(rec) {
                out.push(p);
            }
        }
        return Ok(out);
    }

    Err(std::io::Error::other(
        "KERN_PROC_ALL kept returning a full buffer",
    ))
}

/// Cheap self-check that the offsets still describe this kernel's layout.
///
/// Returns a description of the first failed check, so the caller can log it and
/// fall back to `proc_pidinfo` rather than trusting garbage.
pub fn validate() -> Result<(), String> {
    let procs = list_processes().map_err(|e| format!("sysctl failed: {e}"))?;

    // pid 1 is always launchd on macOS.
    match procs.iter().find(|p| p.pid == 1) {
        Some(launchd) if launchd.comm_str() == "launchd" => {}
        Some(other) => {
            return Err(format!(
                "pid 1 reports comm {:?}, expected \"launchd\" (layout changed?)",
                other.comm_str()
            ))
        }
        None => return Err("pid 1 missing from KERN_PROC_ALL".to_string()),
    }

    // Our own record must agree with getpid/getppid/getuid.
    let me = std::process::id();
    match procs.iter().find(|p| p.pid == me) {
        Some(this) => {
            // SAFETY: these getters have no preconditions.
            let (ppid, uid) = unsafe { (libc::getppid() as u32, libc::getuid()) };
            if this.ppid != ppid {
                return Err(format!(
                    "own ppid is {} in sysctl, {ppid} from getppid",
                    this.ppid
                ));
            }
            if this.uid != uid {
                return Err(format!(
                    "own uid is {} in sysctl, {uid} from getuid",
                    this.uid
                ));
            }
        }
        None => return Err("own pid missing from KERN_PROC_ALL".to_string()),
    }

    // Start times must look like Unix seconds, not ticks or garbage.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    for p in procs.iter().take(64) {
        if p.start_sec > now + 86_400 || (p.start_sec != 0 && p.start_sec < 500_000_000) {
            return Err(format!(
                "pid {} has implausible start time {}",
                p.pid, p.start_sec
            ));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_offsets_are_consistent() {
        // Field offsets must not overlap and must fit inside a record.
        let mut spans = [
            (OFF_STARTTIME_SEC, 8),
            (OFF_PID, 4),
            (OFF_PPID, 4),
            (OFF_NICE, 1),
            (OFF_STAT, 1),
            (OFF_COMM, COMM_LEN),
            (OFF_UID, 4),
        ];
        spans.sort_unstable();
        for w in spans.windows(2) {
            let (a, alen) = w[0];
            let (b, _) = w[1];
            assert!(a + alen <= b, "fields overlap at {a} and {b}");
        }
        for (off, len) in spans {
            assert!(off + len <= STRIDE, "field at {off} runs past the record");
        }
    }

    #[test]
    fn validate_passes_on_this_kernel() {
        validate().expect("kinfo_proc layout must match this macOS version");
    }

    #[test]
    fn lists_every_process_including_root_owned() {
        let procs = list_processes().expect("KERN_PROC_ALL must work");
        assert!(procs.len() > 100, "got {} processes", procs.len());

        // launchd is root-owned; `proc_pidinfo` denies it to a normal user, but
        // sysctl does not. This is the whole reason for this module.
        let launchd = procs.iter().find(|p| p.pid == 1).expect("pid 1");
        assert_eq!(launchd.comm_str(), "launchd");
        assert_eq!(launchd.uid, 0);

        // PIDs must be unique.
        let mut pids: Vec<u32> = procs.iter().map(|p| p.pid).collect();
        let n = pids.len();
        pids.sort_unstable();
        pids.dedup();
        assert_eq!(pids.len(), n, "duplicate pids in KERN_PROC_ALL");
    }

    #[test]
    fn sees_more_processes_than_proc_pidinfo() {
        // Regression guard for the bug where 186 of 456 processes were dropped.
        let via_sysctl = list_processes().map(|v| v.len()).unwrap_or(0);
        let via_libproc = crate::darwin::scan::list_pids()
            .map(|v| {
                v.into_iter()
                    .filter(|p| crate::darwin::scan::task_all_info(*p).is_some())
                    .count()
            })
            .unwrap_or(0);

        assert!(
            via_sysctl >= via_libproc,
            "sysctl saw {via_sysctl}, libproc {via_libproc}"
        );
        if unsafe { libc::getuid() } != 0 {
            assert!(
                via_sysctl > via_libproc + 50,
                "sysctl ({via_sysctl}) should see far more than libproc ({via_libproc}) \
                 for a non-root user"
            );
        }
    }

    /// The uid must come from `KERN_PROC_ALL` and must not be clobbered later.
    ///
    /// A previous version overwrote it with `0` while fetching the identity, so
    /// every process in the table showed up as `root`.
    #[test]
    fn uid_is_the_real_owner() {
        let procs = list_processes().unwrap();
        let me = std::process::id();
        let this = procs.iter().find(|p| p.pid == me).expect("own pid");
        // SAFETY: `getuid` has no preconditions.
        let real = unsafe { libc::getuid() };
        assert_eq!(this.uid, real);

        // And across the table there must be more than one distinct uid on any
        // machine running as a normal user.
        if real != 0 {
            let distinct: std::collections::HashSet<u32> = procs.iter().map(|p| p.uid).collect();
            assert!(
                distinct.len() > 1,
                "every process reported uid {real}; the owner is being lost"
            );
        }
    }

    #[test]
    fn own_ppid_matches_getppid() {
        let procs = list_processes().unwrap();
        let me = std::process::id();
        let this = procs.iter().find(|p| p.pid == me).expect("own pid");
        // SAFETY: no preconditions.
        let real = unsafe { libc::getppid() } as u32;
        assert_eq!(this.ppid, real);
    }

    #[test]
    fn state_histogram_matches_bsd_meaning() {
        // `p_stat` is not reliable for the sleeping/runnable distinction on this
        // kernel: `KERN_PROC_ALL` and `KERN_PROC_PID` both report `SRUN` (2) for
        // a blocked `read(2)` while `ps -o stat=` calls it `S`, and both report
        // `SSTOP` (4) for a stopped process, so only the *stopped* and *zombie*
        // values can be trusted from this field. `darwin::system` therefore
        // derives runnability from `proc_taskinfo.pti_numrunning` instead.
        //
        // What this test can still pin down: the field is being read from the
        // right offset, so it contains BSD state values rather than `p_flag`
        // bytes (which are 0x04 for essentially every process).
        let procs = list_processes().unwrap();
        let hist: std::collections::BTreeMap<i8, usize> =
            procs.iter().fold(Default::default(), |mut h, p| {
                *h.entry(p.p_stat).or_insert(0) += 1;
                h
            });

        assert!(!hist.is_empty(), "no processes were decoded");
        for stat in hist.keys() {
            assert!(
                (1..=5).contains(stat),
                "p_stat {stat} is not a BSD S* constant; histogram {hist:?}"
            );
        }

        // Sleeping and runnable processes both come back as SRUN on this kernel,
        // so the bulk of the table must be valid BSD states rather than all
        // sitting on one bogus byte.
        let total: usize = hist.values().sum();
        let plausible: usize = hist
            .iter()
            .filter(|(stat, _)| (1..=5).contains(*stat))
            .map(|(_, n)| *n)
            .sum();
        assert_eq!(plausible, total);
    }

    /// A stopped process must be reported as such: that is the one state this
    /// field does distinguish, and it is what the `T` badge depends on.
    #[test]
    fn a_stopped_process_reports_sstop() {
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .expect("spawn sleep");
        let pid = child.id();

        // SIGSTOP via the same path the TUI uses.
        // SAFETY: `kill` takes integers only.
        let rc = unsafe { libc::kill(pid as libc::pid_t, libc::SIGSTOP) };
        assert_eq!(rc, 0, "SIGSTOP failed");
        std::thread::sleep(std::time::Duration::from_millis(120));

        let procs = list_processes().expect("KERN_PROC_ALL");
        let this = procs.iter().find(|p| p.pid == pid);
        let stat = this.map(|p| p.p_stat);

        let _ = child.kill();
        let _ = child.wait();

        assert_eq!(
            stat,
            Some(4),
            "a stopped process must read SSTOP(4), not {} (offset wrong?)",
            stat.map(|s| s.to_string()).unwrap_or_else(|| "missing".into())
        );
    }
}
