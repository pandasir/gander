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

// Offsets inside one record. See the module docs for how they were derived.
// Verified on arm64 macOS 15 by cross-checking against ps(1):
//   p_starttime.tv_sec  at 0    (plausible Unix seconds)
//   p_stat              at 32   (match `ps -o stat=`)
//   p_pid               at 40   (matches getpid/ps)
//   p_nice              at 242  (matched by renice-ing a child to 10)
//   p_comm              at 243  ("launchd" for pid 1)
//   e_ppid              at 560  (matches getppid)
//   e_ucred.cr_uid      at 392  (matches getuid, and pid 1 is 0)
const OFF_STARTTIME_SEC: usize = 0;
const OFF_PID: usize = 40;
const OFF_PPID: usize = 560;
const OFF_NICE: usize = 242;
const OFF_STAT: usize = 32;
const OFF_COMM: usize = 243;
const COMM_LEN: usize = 17;
/// `e_ucred.cr_uid` inside `eproc`.
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
/// The size query races with process creation, so the fetch retries with growing
/// headroom until it returns fewer records than it was given room for.
pub fn list_processes() -> std::io::Result<Vec<RawProc>> {
    let mut mib = [
        libc::CTL_KERN,
        libc::KERN_PROC,
        libc::KERN_PROC_ALL,
        0 as c_int,
    ];

    let mut capacity = 512usize;
    for attempt in 0..5 {
        capacity += attempt * 128;
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
            return Err(std::io::Error::last_os_error());
        }

        let records = len / STRIDE;
        // A full buffer means the table grew mid-call; retry with more room.
        if records >= capacity {
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
        // On macOS `p_stat` uses the classic BSD numbering *shifted*: this kernel
        // reports 4 for the 458 sleeping processes `ps` also calls S, 2 for
        // running, and 6 for zombies. See `parse::state_letter`.
        let procs = list_processes().unwrap();
        let hist: std::collections::BTreeMap<i8, usize> =
            procs.iter().fold(Default::default(), |mut h, p| {
                *h.entry(p.p_stat).or_insert(0) += 1;
                h
            });

        let sleeping = hist.get(&4).copied().unwrap_or(0);
        let total: usize = hist.values().sum();
        assert!(
            sleeping as f64 / total as f64 > 0.8,
            "expected most processes to be SSLEEP(4), histogram {hist:?}"
        );
    }
}
