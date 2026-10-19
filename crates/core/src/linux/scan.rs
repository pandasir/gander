//! Reading `/proc` and `/sys`.
//!
//! All hot-path reads go through `thread_local!` scratch buffers so a scan
//! allocates nothing per process (`docs/DESIGN.md` §9.4).

use std::cell::RefCell;
use std::fs;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use crate::row::Row;
use crate::table::{Cache, Stable};
use crate::Pid;

thread_local! {
    static BUF: RefCell<Vec<u8>> = RefCell::new(Vec::with_capacity(4096));
    static PATH: RefCell<Vec<u8>> = RefCell::new(Vec::with_capacity(64));
}

/// Read a whole file into the thread-local buffer, returning its length.
///
/// `ENOENT` is the common case (the process exited between `readdir` and now),
/// so it is reported as `None` rather than as an error.
fn read_into_buf(path: &[u8]) -> Option<usize> {
    let mut file = fs::File::open(std::ffi::OsStr::from_bytes(path)).ok()?;
    BUF.with(|b| {
        let mut b = b.borrow_mut();
        b.clear();
        use std::io::Read as _;
        file.read_to_end(&mut b).ok()
    })
}

/// Run `f` with the contents of `path`, using the shared scratch buffer.
fn with_file<R>(path: &[u8], f: impl FnOnce(&[u8]) -> R) -> Option<R> {
    let n = read_into_buf(path)?;
    BUF.with(|b| {
        let b = b.borrow();
        Some(f(&b[..n]))
    })
}

/// Build `"<root>/<pid>/<suffix>"` into the thread-local path buffer and run `f`
/// with the resulting path.
fn proc_path(root: &Path, pid: Pid, suffix: &str, f: impl FnOnce(&[u8])) {
    PATH.with(|p| {
        let mut p = p.borrow_mut();
        p.clear();
        p.extend_from_slice(root.as_os_str().as_bytes());
        p.push(b'/');
        write_pid(&mut p, pid);
        if !suffix.is_empty() {
            p.push(b'/');
            p.extend_from_slice(suffix.as_bytes());
        }
        f(&p);
    })
}

fn write_pid(out: &mut Vec<u8>, mut pid: Pid) {
    let mut tmp = [0u8; 10];
    let mut i = tmp.len();
    if pid == 0 {
        out.push(b'0');
        return;
    }
    while pid > 0 {
        i -= 1;
        tmp[i] = b'0' + (pid % 10) as u8;
        pid /= 10;
    }
    out.extend_from_slice(&tmp[i..]);
}

/// List process IDs by scanning `/proc` for numeric directory entries.
pub fn list_pids(root: &Path) -> io::Result<Vec<Pid>> {
    let mut out = Vec::with_capacity(512);
    for entry in fs::read_dir(root)? {
        let Ok(entry) = entry else { continue };
        let name = entry.file_name();
        let bytes = name.as_bytes();
        if bytes.is_empty() || !bytes.iter().all(u8::is_ascii_digit) {
            continue;
        }
        if let Ok(pid) = name.to_string_lossy().parse::<Pid>() {
            out.push(pid);
        }
    }
    Ok(out)
}

/// List the thread ids inside a process (`/proc/<pid>/task`).
///
/// A process is its own thread group leader, so the leader's tid equals its pid
/// and appears in this list too; callers filter or keep it deliberately.
pub fn list_tids(root: &Path, pid: Pid) -> io::Result<Vec<Pid>> {
    let mut out = Vec::with_capacity(8);
    let dir = root.join(pid.to_string()).join("task");
    for entry in fs::read_dir(dir)? {
        let Ok(entry) = entry else { continue };
        let name = entry.file_name();
        let bytes = name.as_bytes();
        if bytes.is_empty() || !bytes.iter().all(u8::is_ascii_digit) {
            continue;
        }
        if let Ok(tid) = name.to_string_lossy().parse::<Pid>() {
            out.push(tid);
        }
    }
    Ok(out)
}

/// Read `stat` for one thread of `pid`.
///
/// `/proc/<pid>/task/<tid>/stat` is the same format as a process's `stat`, with
/// the thread's own CPU counters. The identity files (`status`, `cmdline`) are
/// per-process, so a thread row reuses its leader's.
pub fn read_thread_stat(
    root: &Path,
    pid: Pid,
    tid: Pid,
) -> Option<(crate::row::CommBuf, super::parse::StatFields)> {
    let path = root
        .join(pid.to_string())
        .join("task")
        .join(tid.to_string())
        .join("stat");
    let bytes = fs::read(path).ok()?;
    let (_pid, comm, fields) = super::parse::parse_stat(&bytes)?;
    Some((comm, fields))
}

/// Read the `stat` file of one process.
pub fn read_stat(root: &Path, pid: Pid) -> Option<(crate::row::CommBuf, super::parse::StatFields)> {
    let mut slot = None;
    proc_path(root, pid, "stat", |p| {
        slot = with_file(p, super::parse::parse_stat).flatten();
    });
    let (_pid, comm, fields) = slot?;
    Some((comm, fields))
}

/// Join a NUL-separated `cmdline` into a displayable single line.
fn join_argv(b: &[u8]) -> String {
    let mut s = String::with_capacity(b.len());
    let mut first = true;
    for part in b.split(|c| *c == 0) {
        if part.is_empty() {
            continue;
        }
        if !first {
            s.push(' ');
        }
        s.push_str(&String::from_utf8_lossy(part));
        first = false;
    }
    s
}

/// Apply `stat` fields to a row.
pub fn apply_stat(out: &mut Row, fields: &super::parse::StatFields, page_size: u64) {
    super::parse::fill_row(out, fields, page_size);
}

/// Fetch (and cache) the immutable identity of one process.
pub fn read_identity(root: &Path, pid: Pid, starttime: u64, cache: &mut Cache) -> Stable {
    if let Some(hit) = cache.get(pid, starttime) {
        return hit.clone();
    }

    let mut stable = Stable::default();

    proc_path(root, pid, "status", |p| {
        if let Some(uid) = with_file(p, super::parse::parse_status_uid).flatten() {
            stable.uid = uid;
        }
    });

    let mut cmdline = None;
    proc_path(root, pid, "cmdline", |p| {
        cmdline = with_file(p, join_argv).filter(|s| !s.is_empty());
    });
    stable.cmdline = cmdline.map(String::into_boxed_str);

    let mut exe = None;
    let mut exe_deleted = false;
    proc_path(root, pid, "exe", |p| {
        if let Ok(target) = fs::read_link(std::ffi::OsStr::from_bytes(p)) {
            let text = target.to_string_lossy();
            // The kernel appends " (deleted)" to the link target once the file
            // backing a running process is unlinked or replaced. This is the only
            // available signal: an mtime comparison needs the path to still
            // exist, which is precisely what deleting it removes.
            exe_deleted = text.ends_with(" (deleted)");
            let path = text.strip_suffix(" (deleted)").unwrap_or(&text);
            exe = Some(path.to_string().into_boxed_str());
        }
    });
    stable.exe = exe;
    stable.exe_deleted = exe_deleted;

    let mut cgroup = None;
    #[cfg(target_os = "linux")]
    let mut container = None;
    #[cfg(target_os = "linux")]
    let mut container_runtime = None;
    proc_path(root, pid, "cgroup", |p| {
        if let Some(entries) = with_file(p, super::fsops::parse_cgroup) {
            // The path, not the raw line: `12:cpuset:/docker/abc` carries the
            // hierarchy id and controller list, which no column wants.
            cgroup = super::fsops::preferred_path(&entries)
                .map(|s| s.to_string().into_boxed_str());
            #[cfg(target_os = "linux")]
            if let Some(c) = super::fsops::container_from_cgroup(&entries) {
                container = Some(c.id.into_boxed_str());
                container_runtime = Some(c.runtime.to_string().into_boxed_str());
            }
        }
    });
    stable.cgroup = cgroup;

    // A container with its own cgroup namespace reports `/` in
    // `/proc/<pid>/cgroup`, so the path above says nothing. The PID namespace
    // still distinguishes a *child* container, and without this the difference
    // between "not containerised" and "containerised but unidentified" is lost.
    //
    // Only consulted when the path did not already identify a container: the path
    // is the *informative* signal (it names the runtime and the id), the namespace
    // is the *membership* signal, and the path wins when both exist.
    //
    // The id is left as `?` rather than filled with something we made up. A
    // namespace inode is not a container id, and printing one would look like an
    // identifier while telling the reader nothing they can act on.
    #[cfg(target_os = "linux")]
    if container.is_none()
        && matches!(
            super::fsops::in_different_pid_namespace(root, pid),
            Some(true)
        )
    {
        container = Some("?".to_string().into_boxed_str());
        container_runtime = Some("container".to_string().into_boxed_str());
    }

    #[cfg(target_os = "linux")]
    {
        stable.container = container;
        stable.container_runtime = container_runtime;
    }

    stable.is_kernel_thread = stable.cmdline.is_none() && stable.exe.is_none();

    cache.insert(pid, starttime, stable.clone());
    stable
}

/// Read `/proc/meminfo`.
pub fn read_meminfo(
    root: &Path,
    mem: &mut crate::system::MemInfo,
    swap: &mut crate::system::SwapInfo,
) {
    let mut path = PathBuf::from(root);
    path.push("meminfo");
    with_file(path.as_os_str().as_bytes(), |b| {
        super::parse::parse_meminfo(b, mem, swap)
    });
}

/// Read `/proc/stat`.
pub fn read_proc_stat(root: &Path) -> Vec<crate::system::CpuTimes> {
    let mut path = PathBuf::from(root);
    path.push("stat");
    with_file(path.as_os_str().as_bytes(), super::parse::parse_proc_stat).unwrap_or_default()
}

/// Read `/proc/loadavg`.
pub fn read_loadavg(root: &Path) -> Option<[f64; 3]> {
    let mut path = PathBuf::from(root);
    path.push("loadavg");
    with_file(path.as_os_str().as_bytes(), super::parse::parse_loadavg)
}

/// Read `/proc/uptime`, returning whole seconds.
pub fn read_uptime(root: &Path) -> Option<u64> {
    let mut path = PathBuf::from(root);
    path.push("uptime");
    with_file(path.as_os_str().as_bytes(), |b| {
        let s = String::from_utf8_lossy(b);
        let secs = s.split_whitespace().next()?;
        secs.split('.').next()?.parse::<u64>().ok()
    })
    .flatten()
}

/// Whether `/proc/<pid>` still refers to the same process identified by
/// `starttime`. Used to detect PID reuse between two samples.
pub fn is_same_process(root: &Path, pid: Pid, starttime: u64) -> bool {
    read_stat(root, pid)
        .map(|(_, fields)| fields.starttime == starttime)
        .unwrap_or(false)
}

/// Read host-wide network counters from `/proc/net/dev`.
///
/// Missing on a system with no networking at all; that is reported as zeroes
/// rather than as an error, because the meter should show `0 B/s`, not vanish.
pub fn read_net_counters(root: &Path) -> crate::system::NetCounters {
    let mut path = PathBuf::from(root);
    path.push("net");
    path.push("dev");
    with_file(path.as_os_str().as_bytes(), super::parse::parse_net_dev).unwrap_or_default()
}

/// Read host-wide disk counters from `/proc/diskstats`.
///
/// The authoritative "is this a whole disk" test is the presence of
/// `/sys/block/<name>`; it is consulted per device, because `/proc/diskstats`
/// mixes disks and partitions in one file. When `/sys` is not readable (a
/// fixture tree, a container) the name heuristic is used instead.
pub fn read_disk_counters(root: &Path) -> crate::system::DiskCounters {
    // `root` is `/proc`; sysfs lives beside it at `/sys` for the live tree. A
    // fixture root has neither, and the heuristic covers that case.
    let sys_block = root
        .parent()
        .map(|p| p.join("sys/block"))
        .filter(|p| p.is_dir());

    let mut path = PathBuf::from(root);
    path.push("diskstats");
    with_file(path.as_os_str().as_bytes(), |b| {
        super::parse::parse_diskstats(b, |name| match &sys_block {
            Some(dir) => dir
                .join(std::ffi::OsStr::from_bytes(name))
                .is_dir(),
            None => super::parse::looks_like_whole_disk(name),
        })
    })
    .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pid_writing() {
        for pid in [0u32, 1, 9, 10, 42, 1234, 4_194_304] {
            let mut v = Vec::new();
            write_pid(&mut v, pid);
            assert_eq!(String::from_utf8(v).unwrap(), pid.to_string());
        }
    }

    #[test]
    fn pid_path_shape() {
        let mut seen = String::new();
        proc_path(Path::new("/proc"), 42, "stat", |b| {
            seen = String::from_utf8_lossy(b).into_owned();
        });
        assert_eq!(seen, "/proc/42/stat");

        let mut root_only = String::new();
        proc_path(Path::new("/proc"), 1, "", |b| {
            root_only = String::from_utf8_lossy(b).into_owned();
        });
        assert_eq!(root_only, "/proc/1");
    }

    #[test]
    fn argv_joining() {
        assert_eq!(join_argv(b"/bin/sh\0-c\0echo hi\0"), "/bin/sh -c echo hi");
        assert_eq!(join_argv(b""), "");
        assert_eq!(join_argv(b"\0\0"), "");
    }

    #[test]
    fn fixture_scan() {
        let root = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/proc"));
        if !root.exists() {
            return; // fixtures are added in a later step
        }
        let pids = list_pids(root).expect("readable fixture root");
        assert!(!pids.is_empty());
    }
}
