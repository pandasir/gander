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

/// Choose which cgroup path to display when `/proc/<pid>/cgroup` lists several.
///
/// On a hybrid host (v1 controllers plus a v2 entry) the v2 line has an empty
/// controller list and its path is the one shared by every hierarchy, so it is
/// the honest answer for "which cgroup is this process in". Falling back to the
/// first entry keeps v1-only hosts working.
pub fn preferred_path(entries: &[CgroupEntry]) -> Option<&str> {
    // The unified entry: hierarchy 0 with no controllers.
    if let Some(unified) = entries
        .iter()
        .find(|e| e.hierarchy == 0 && e.controllers.is_empty())
    {
        return Some(&unified.path);
    }
    entries.first().map(|e| e.path.as_str())
}

/// What a cgroup path says about the process's container membership.
///
/// htop decides this from cgroup membership rather than heuristics, and so do we.
/// The distinction that matters for a *column* is not "is it in a container" but
/// *which* one: `container_from_cgroup` returning the bare marker (`"docker"`)
/// made every containerised process on the machine render the same word, which is
/// a boolean dressed up as data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Container {
    /// The runtime that owns it: `docker`, `kubepods`, `lxc`, …
    pub runtime: &'static str,
    /// The container's identifier, as the cgroup path spells it.
    ///
    /// For Docker and Podman this is the 64-hex id (or a prefix of it); for
    /// Kubernetes it is the pod uid. Empty when the path names a runtime but no
    /// identifiable container — a `kubepods.slice` with no pod below it, say.
    pub id: String,
}

impl Container {
    /// The id shortened to the first 12 characters, which is what Docker prints
    /// and enough to tell containers apart on screen.
    ///
    /// Shortening here rather than at render time keeps the column and the
    /// inspector agreeing; it also means the sort key and the displayed text are
    /// the same string.
    pub fn short_id(&self) -> &str {
        let end = self.id.len().min(12);
        // Ids are hex or uuid, so this never splits a character.
        &self.id[..end]
    }
}

/// Best-effort container detection from cgroup membership.
///
/// # What the paths look like
///
/// | runtime | example path |
/// |---|---|
/// | Docker (cgroup v1) | `/docker/1f2e…` |
/// | Docker (v2, systemd) | `/system.slice/docker-1f2e….scope` |
/// | Podman | `/machine.slice/libpod-1f2e….scope` |
/// | Kubernetes | `/kubepods.slice/kubepods-burstable.slice/kubepods-burstable-pod<uid>.slice/cri-containerd-<id>.scope` |
/// | LXC | `/lxc.payload.<name>` |
/// | systemd-nspawn | `/machine.slice/machine-<name>.scope` |
///
/// A host process sits at `/`, `/init.scope`, `/user.slice/…` or `/system.slice/…`
/// and must report `None`: the marker check alone would classify a host process
/// under `system.slice/containerd.service` as a container.
pub fn container_from_cgroup(entries: &[CgroupEntry]) -> Option<Container> {
    for entry in entries {
        if let Some(c) = container_from_path(&entry.path) {
            return Some(c);
        }
    }
    None
}

/// Classify one cgroup path.
///
/// Split out from [`container_from_cgroup`] so it is testable on its own, and so
/// the caller can decide which hierarchy to prefer when there are several.
pub fn container_from_path(path: &str) -> Option<Container> {
    // Order matters: `kubepods` paths also contain `containerd` in the leaf
    // (`cri-containerd-<id>.scope`), and the pod id is the useful identifier, so
    // Kubernetes has to be tested before the plain runtimes.
    if let Some(rest) = path.strip_prefix("/kubepods") {
        return Some(Container {
            runtime: "kubepods",
            id: kubernetes_id(rest),
        });
    }

    // Docker v1 (`/docker/<id>`) and the systemd-scoped form
    // (`/system.slice/docker-<id>.scope`).
    if let Some(rest) = path.strip_prefix("/docker/") {
        return Some(Container {
            runtime: "docker",
            id: trim_scope(rest).to_string(),
        });
    }
    if let Some(pos) = path.find("docker-") {
        // `docker-<id>.scope`, and the v1-style `docker/<id>` after a prefix.
        let rest = &path[pos + "docker-".len()..];
        return Some(Container {
            runtime: "docker",
            id: trim_scope(rest).to_string(),
        });
    }

    if let Some(pos) = path.find("libpod-") {
        let rest = &path[pos + "libpod-".len()..];
        return Some(Container {
            runtime: "podman",
            id: trim_scope(rest).to_string(),
        });
    }
    // `podman-<id>.scope` appears on newer systemd-driven Podman.
    if let Some(pos) = path.find("podman-") {
        let rest = &path[pos + "podman-".len()..];
        return Some(Container {
            runtime: "podman",
            id: trim_scope(rest).to_string(),
        });
    }

    if let Some(pos) = path.find("lxc.payload.") {
        let rest = &path[pos + "lxc.payload.".len()..];
        return Some(Container {
            runtime: "lxc",
            id: rest.trim_end_matches('/').to_string(),
        });
    }

    // systemd-nspawn: `/machine.slice/machine-<name>.scope`. htop treats these as
    // containers (`[SNC:*]`), so we do too.
    if let Some(pos) = path.find("machine-") {
        let rest = &path[pos + "machine-".len()..];
        return Some(Container {
            runtime: "nspawn",
            id: trim_scope(rest).to_string(),
        });
    }

    // A bare `containerd` leaf with no pod above it, e.g.
    // `/system.slice/containerd.service` — that is the *host* daemon, not a
    // container, and classifying it would label every process on the box.
    None
}

/// Trim a `.scope` / `.slice` suffix and any trailing slash.
fn trim_scope(s: &str) -> &str {
    let s = s.trim_end_matches('/');
    s.strip_suffix(".scope")
        .or_else(|| s.strip_suffix(".slice"))
        .unwrap_or(s)
}

/// The PID namespace inode of the running process, i.e. gander's own.
///
/// This is the baseline for [`in_different_pid_namespace`], and it is deliberately
/// *our* namespace rather than `/proc/1`'s. htop does the same
/// (`LinuxProcessTable.c` reads `PROCDIR "/self/ns/pid"` into `rootPidNs`).
///
/// Using `/proc/1` looks equivalent and is not: inside a container, pid 1 *is* the
/// container's init, so comparing against it says "same namespace" for every
/// process in that container, and no process is ever reported as containerised.
/// That is exactly backwards — everything visible from inside a container is in
/// it. Anchoring on our own namespace answers the question the column asks:
/// "is this process in a *nested* namespace below the one I am watching from?"
pub fn self_pid_namespace(proc_root: &Path) -> Option<u64> {
    use std::os::unix::fs::MetadataExt as _;
    std::fs::metadata(proc_root.join("self/ns/pid"))
        .ok()
        .map(|m| m.ino())
}

/// Whether `pid` lives in a different PID namespace than the caller.
///
/// # Why this exists on top of the cgroup path
///
/// The cgroup path is the *informative* signal — it names the runtime and the
/// container — but it is not a reliable *membership* test, because a container
/// with its own cgroup namespace sees `/` in `/proc/<pid>/cgroup`. A cgroup-only
/// check therefore reports "no container" for a process whose cgroup path says
/// nothing.
///
/// The PID namespace is the membership test: two processes in the same namespace
/// share an `ns/pid` inode, and a process in a *child* namespace has a different
/// one. Needs no privileges for a process we can already see.
///
/// Returns `None` when the comparison cannot be made (no `/proc/<pid>/ns/pid`,
/// which is the normal case for another user's process on a hardened kernel), so
/// the caller can distinguish "not containerised" from "unknown".
pub fn in_different_pid_namespace(proc_root: &Path, pid: crate::Pid) -> Option<bool> {
    use std::os::unix::fs::MetadataExt as _;
    let mine = std::fs::metadata(proc_root.join(pid.to_string()).join("ns/pid"))
        .ok()?
        .ino();
    let ours = self_pid_namespace(proc_root)?;
    Some(mine != ours)
}

/// Pull the pod uid and, if present, the container id out of a `kubepods` path.
///
/// Kubernetes nests one slice per QoS class and one per pod, then a leaf per
/// container. The pod uid identifies *which* pod, which is what a column should
/// show; the leaf id changes on every restart.
fn kubernetes_id(rest: &str) -> String {
    // `kubepods-burstable-pod<uid>_<uid>.slice` — the uid appears twice, once in
    // the slice name and once as the slice's own suffix.
    if let Some(pos) = rest.find("-pod") {
        let after = &rest[pos + "-pod".len()..];
        let end = after
            .find(['.', '_', '/'])
            .unwrap_or(after.len());
        let uid = &after[..end];
        if !uid.is_empty() {
            return uid.to_string();
        }
    }
    // A bare `/kubepods/<id>` leaf (cgroup v1, CRI-O).
    rest.trim_start_matches('/')
        .split('/')
        .next()
        .map(trim_scope)
        .unwrap_or("")
        .to_string()
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

/// Read and parse `/proc/<pid>/smaps_rollup`.
///
/// # Why the caller has to gate this
///
/// The kernel walks every VMA in the process's address space to build this file.
/// Measured on a desktop with ~700 processes, reading it for all of them costs
/// tens of milliseconds — more than the entire rest of the scan. It is therefore
/// only called when a column that needs it (`PSS`, `EPSS`, `M_PRIVATE`) is
/// actually visible, and never otherwise.
///
/// Returns `None` when the file is missing (a kernel older than 4.14), or when it
/// cannot be read: another user's `smaps_rollup` needs `CAP_SYS_PTRACE`, and
/// `EPERM` there is normal rather than an error to report.
pub fn read_smaps_rollup(proc_root: &Path, pid: crate::Pid) -> Option<SmapsRollup> {
    let path = proc_root.join(pid.to_string()).join("smaps_rollup");
    let bytes = std::fs::read(path).ok()?;
    parse_smaps_rollup(&bytes)
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
        let c = container_from_cgroup(&e).expect("docker");
        assert_eq!(c.runtime, "docker");
        assert_eq!(c.id, "abc123");
    }

    #[test]
    fn no_container_for_plain_host_process() {
        let e = parse_cgroup(b"0::/user.slice/session-2.scope\n");
        assert_eq!(container_from_cgroup(&e), None);
    }

    /// A containerised process must be identifiable, not just flagged.
    ///
    /// `container_from_cgroup` used to return the bare marker, so every
    /// containerised process on the machine rendered the same word and the column
    /// carried no information.
    #[test]
    fn docker_paths_yield_the_container_id() {
        // cgroup v1
        let c = container_from_path("/docker/1f2e3d4c5b6a7988").expect("docker");
        assert_eq!(c.runtime, "docker");
        assert_eq!(c.id, "1f2e3d4c5b6a7988");

        // cgroup v2 via systemd, which is what a current Docker on a systemd host
        // writes. The `.scope` suffix must not leak into the id.
        let c = container_from_path("/system.slice/docker-1f2e3d4c5b6a7988.scope").expect("docker");
        assert_eq!(c.runtime, "docker");
        assert_eq!(c.id, "1f2e3d4c5b6a7988");
    }

    #[test]
    fn podman_paths_are_recognised() {
        let c = container_from_path("/machine.slice/libpod-abc123def456.scope").expect("podman");
        assert_eq!(c.runtime, "podman");
        assert_eq!(c.id, "abc123def456");

        let c = container_from_path("/user.slice/podman-999.scope").expect("podman");
        assert_eq!(c.runtime, "podman");
        assert_eq!(c.id, "999");
    }

    #[test]
    fn lxc_and_nspawn_paths_are_recognised() {
        let c = container_from_path("/lxc.payload.web-1").expect("lxc");
        assert_eq!(c.runtime, "lxc");
        assert_eq!(c.id, "web-1");

        let c = container_from_path("/machine.slice/machine-build.scope").expect("nspawn");
        assert_eq!(c.runtime, "nspawn");
        assert_eq!(c.id, "build");
    }

    /// Kubernetes nests QoS class → pod → container. The pod uid is the useful
    /// identifier; the leaf changes on every restart.
    #[test]
    fn kubernetes_paths_yield_the_pod_uid() {
        let path = "/kubepods.slice/kubepods-burstable.slice/\
                    kubepods-burstable-pod1a2b3c4d-5e6f-7890-abcd-ef1234567890.slice/\
                    cri-containerd-9f8e7d6c5b4a3210.scope";
        let c = container_from_path(path).expect("kubepods");
        assert_eq!(c.runtime, "kubepods");
        assert_eq!(c.id, "1a2b3c4d-5e6f-7890-abcd-ef1234567890");
    }

    #[test]
    fn kubernetes_wins_over_the_containerd_leaf() {
        // The leaf says `cri-containerd-…`, so testing `containerd` first would
        // classify this as plain containerd and lose the pod identity.
        let path = "/kubepods.slice/kubepods-besteffort.slice/\
                    kubepods-besteffort-podabcd.slice/cri-containerd-1234.scope";
        let c = container_from_path(path).expect("a container");
        assert_eq!(c.runtime, "kubepods");
        assert_eq!(c.id, "abcd");
    }

    /// The host's own `containerd.service` is not a container. Classifying it
    /// would label every process on the machine as containerised.
    #[test]
    fn the_host_daemon_is_not_a_container() {
        assert_eq!(container_from_path("/system.slice/containerd.service"), None);
        assert_eq!(container_from_path("/system.slice/docker.service"), None);
        assert_eq!(container_from_path("/"), None);
        assert_eq!(container_from_path("/init.scope"), None);
        assert_eq!(container_from_path("/user.slice/user-1000.slice/session-1.scope"), None);
    }

    #[test]
    fn short_id_is_twelve_characters() {
        let c = Container {
            runtime: "docker",
            id: "1f2e3d4c5b6a79880a1b2c3d4e5f6071".to_string(),
        };
        assert_eq!(c.short_id(), "1f2e3d4c5b6a");

        // A short id (LXC name, a pod uid prefix) is returned whole.
        let c = Container {
            runtime: "lxc",
            id: "web-1".to_string(),
        };
        assert_eq!(c.short_id(), "web-1");

        let c = Container {
            runtime: "kubepods",
            id: String::new(),
        };
        assert_eq!(c.short_id(), "");
    }
}
