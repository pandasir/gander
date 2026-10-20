//! Live FreeBSD checks.
//!
//! Same shape as `linux_live.rs`: every assertion compares gander's figure
//! against the same source read independently, *inside one test*, so a changing
//! system cannot explain a discrepancy and a fixed expectation cannot go stale.
//!
//! The suite is skipped entirely on other platforms.

#![cfg(target_os = "freebsd")]

use std::process::Command;

use gander_core::row::{Field, Row};
use gander_core::table::{Dir, ProcTable, Sort, Visibility};
use gander_core::PlatformSampler;

/// One `sysctl` value as text, read with `sysctl(8)` so the comparison is against
/// a different program's answer rather than against our own parser.
fn sysctl(name: &str) -> Option<String> {
    let out = Command::new("sysctl").arg("-n").arg(name).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!text.is_empty()).then_some(text)
}

fn sysctl_u64(name: &str) -> Option<u64> {
    sysctl(name)?.parse().ok()
}

#[test]
fn the_sampler_satisfies_the_shared_trait() {
    assert!(<gander_core::Sampler as PlatformSampler>::backend_name().contains("freebsd"));
    let mut s = gander_core::Sampler::default();
    let (_snap, rows) = PlatformSampler::sample(&mut s);
    assert!(!rows.is_empty(), "a live machine always has processes");
    assert_eq!(
        PlatformSampler::cache_len(&s),
        s.cache_len(),
        "the trait and the inherent method must agree"
    );
}

#[test]
fn total_memory_matches_the_managed_page_count() {
    let mut s = gander_core::Sampler::default();
    let (snap, _rows) = s.sample();

    // `v_page_count * pagesize` is what the VM subsystem manages. `hw.physmem` is
    // larger by the kernel's early allocations, which no page queue ever holds,
    // so using it as the denominator leaves the bar permanently short.
    let pages = sysctl_u64("vm.stats.vm.v_page_count").expect("page count");
    let page_size = sysctl_u64("hw.pagesize").expect("page size");
    assert_eq!(
        snap.mem.total,
        pages * page_size,
        "total must be the managed page count, not hw.physmem"
    );
}

#[test]
fn the_page_queues_are_the_memory_classes() {
    let mut s = gander_core::Sampler::default();
    let (snap, _rows) = s.sample();
    let m = &snap.mem;
    let page = sysctl_u64("hw.pagesize").expect("page size");

    // Each counter is a *separate* `sysctl` call here and a separate one in the
    // sampler, so the two are milliseconds apart on a live VM whose page daemon
    // is moving pages. The tolerance is one screen's worth of pages, not slack for
    // a wrong source: reading `v_free_count` from the wrong key would differ by
    // orders of magnitude.
    let counter = |name: &str| sysctl_u64(name).expect(name) * page;
    let tolerance = 16 * 1024 * 1024;
    let close = |got: u64, want: u64, what: &str| {
        assert!(
            got.abs_diff(want) <= tolerance,
            "{what}: gander {got} vs sysctl {want}"
        );
    };
    close(m.free, counter("vm.stats.vm.v_free_count"), "free");
    close(m.active, counter("vm.stats.vm.v_active_count"), "active");
    close(m.inactive, counter("vm.stats.vm.v_inactive_count"), "inactive");
    close(m.wired, counter("vm.stats.vm.v_wire_count"), "wired");
    close(m.laundry, counter("vm.stats.vm.v_laundry_count"), "laundry");

    // `used` is the queues that cannot be reclaimed without work: wired pages,
    // active pages, and pages queued for writeback. This is an identity over the
    // sampler's own one-shot view, so it is exact.
    assert_eq!(
        m.used,
        m.wired + m.active + m.laundry,
        "used must be exactly the non-reclaimable queues"
    );
    // Cache is the reclaimable half.
    assert_eq!(m.cached, m.inactive + m.file_cache, "cache");
    assert!(m.available >= m.free, "available includes the cache");
    assert!(m.available <= m.total);
    assert!(m.used <= m.total);
}

#[test]
fn swap_matches_swapinfo() {
    let mut s = gander_core::Sampler::default();
    let (snap, _rows) = s.sample();

    let out = match Command::new("swapinfo").arg("-k").output() {
        Ok(o) if o.status.success() => o,
        // No swap configured is a legitimate machine; nothing to compare.
        _ => return,
    };
    let text = String::from_utf8_lossy(&out.stdout);
    // `swapinfo -k` prints a header, one line per device, then a `Total` line.
    let total_line = match text.lines().find(|l| l.starts_with("Total")) {
        Some(l) => l,
        None => return,
    };
    let fields: Vec<&str> = total_line.split_whitespace().collect();
    // Total, Used, Avail, Capacity. Values are in KiB.
    let expected_total: u64 = fields[1].parse::<u64>().unwrap_or(0) * 1024;
    let expected_used: u64 = fields[2].parse::<u64>().unwrap_or(0) * 1024;

    // `swapinfo` rounds to KiB, so allow one KiB per device plus a page.
    let tolerance = 64 * 1024;
    assert!(
        snap.swap.total.abs_diff(expected_total) <= tolerance,
        "swap total {} vs swapinfo {}",
        snap.swap.total,
        expected_total
    );
    assert!(
        snap.swap.used.abs_diff(expected_used) <= tolerance,
        "swap used {} vs swapinfo {}",
        snap.swap.used,
        expected_used
    );
}

#[test]
fn per_core_cpu_counters_match_kern_cp_times() {
    let mut s = gander_core::Sampler::default();
    let (snap, _rows) = s.sample();

    let ncpu = sysctl_u64("hw.ncpu").expect("hw.ncpu") as usize;
    assert_eq!(
        snap.cpu.len(),
        ncpu + 1,
        "one aggregate plus one per core"
    );
    assert_eq!(snap.cpu_count(), ncpu);
    assert!(snap.cpu[0].total() > 0, "the aggregate has been ticking");

    // The aggregate must not be less than any single core: it is their sum.
    let agg = snap.cpu[0].total();
    for (i, c) in snap.cpu.iter().enumerate().skip(1) {
        assert!(
            c.total() <= agg,
            "core {i} has more ticks ({}) than the aggregate ({agg})",
            c.total()
        );
    }
}

#[test]
fn cpu_percent_is_finite_and_bounded_after_two_samples() {
    let mut s = gander_core::Sampler::default();
    s.sample();
    std::thread::sleep(std::time::Duration::from_millis(250));
    let (snap, rows) = s.sample();

    assert!(snap.cpu_usage().is_finite());
    assert!(
        (0.0..=1.0).contains(&snap.cpu_usage()),
        "aggregate usage {} is not a share",
        snap.cpu_usage()
    );

    let cap = snap.cpu_count() as f32 * 100.0 + 1.0;
    for row in &rows {
        assert!(row.percent_cpu.is_finite(), "pid {} has NaN cpu%", row.pid);
        assert!(row.percent_cpu >= 0.0, "pid {} has negative cpu%", row.pid);
        assert!(
            row.percent_cpu <= cap,
            "pid {} reports {}% with {} cores",
            row.pid,
            row.percent_cpu,
            snap.cpu_count()
        );
    }
}

#[test]
fn every_row_has_a_pid_and_a_command() {
    let mut s = gander_core::Sampler::default();
    let (_snap, rows) = s.sample();

    for row in &rows {
        assert!(row.pid > 0, "pid 0 is the kernel container, not a process");
        assert!(
            !row.comm.is_empty(),
            "pid {} has an empty command name",
            row.pid
        );
    }

    let mut pids: Vec<u32> = rows.iter().filter(|r| !r.is_thread).map(|r| r.pid).collect();
    let n = pids.len();
    pids.sort_unstable();
    pids.dedup();
    assert_eq!(pids.len(), n, "duplicate pids in the table");
}

#[test]
fn resident_size_matches_ps() {
    let me = std::process::id();
    let mut s = gander_core::Sampler::default();
    let (_snap, rows) = s.sample();
    let mine = rows
        .iter()
        .find(|r| r.pid == me)
        .expect("own process must be listed");

    // `ps -o rss=` reports KiB. Comparing against another program is the whole
    // point: `ki_rssize` is in pages, and forgetting the page size is a 4x error
    // that looks plausible.
    let out = Command::new("ps")
        .args(["-o", "rss=", "-p", &me.to_string()])
        .output()
        .expect("ps");
    let text = String::from_utf8_lossy(&out.stdout);
    let kib: u64 = text.trim().parse().expect("ps rss");
    let expected = kib * 1024;

    // The two reads are not simultaneous; the test binary may allocate between
    // them. 25% plus a page is generous enough to be stable and tight enough to
    // catch a pages-vs-bytes mix-up (which would be off by 4096x).
    let tolerance = (expected / 4).max(4096);
    assert!(
        mine.res.abs_diff(expected) <= tolerance,
        "resident {} vs ps {} (tolerance {tolerance})",
        mine.res,
        expected
    );
}

#[test]
fn elapsed_is_seconds_since_start_not_since_boot() {
    let me = std::process::id();
    let mut s = gander_core::Sampler::default();
    let (snap, rows) = s.sample();
    let mine = rows.iter().find(|r| r.pid == me).expect("own process");

    // `ps -o etimes=` is the same quantity.
    let out = Command::new("ps")
        .args(["-o", "etimes=", "-p", &me.to_string()])
        .output()
        .expect("ps");
    let text = String::from_utf8_lossy(&out.stdout);
    let expected: u64 = text.trim().parse().expect("ps etimes");

    assert!(
        mine.elapsed.abs_diff(expected) <= 2,
        "elapsed {} vs ps {} (uptime {})",
        mine.elapsed,
        expected,
        snap.uptime
    );
    assert!(
        mine.elapsed <= snap.uptime,
        "a process cannot be older than the machine"
    );
}

#[test]
fn process_count_matches_ps() {
    let mut s = gander_core::Sampler::default();
    let (_snap, rows) = s.sample();
    let ours = rows.iter().filter(|r| !r.is_thread).count();

    let out = Command::new("ps").arg("-ax").output().expect("ps");
    let text = String::from_utf8_lossy(&out.stdout);
    // Header line plus one per process.
    let theirs = text.lines().count().saturating_sub(1);

    // Processes come and go between the two reads.
    let delta = ours.abs_diff(theirs);
    assert!(
        delta <= 5,
        "gander listed {ours} processes, ps listed {theirs}"
    );
}

/// Every process's state letter must be the one `ps` prints.
///
/// This compares the whole table rather than one row, because a single row is
/// inherently a race: the sampler reads the table *while* this test process is
/// blocked inside the `sysctl` call, so the kernel reports `S` for it, and by the
/// time `ps` looks it is `R` again. Asserting on that one row would be a flaky
/// test — and asserting the *converse* (`S`) would encode a scheduling accident
/// as a requirement.
///
/// Across ~30 processes a systematic decode error (`SIDL` vs `SSLEEP`, or the
/// arithmetic `b'I' + stat - 1` mapping) shows up immediately.
#[test]
fn state_letters_are_the_ones_ps_prints() {
    let mut s = gander_core::Sampler::default();
    let (snap, rows) = s.sample();
    let _ = snap;

    // Two columns, one header line. `ps`'s `pid=` spelling (suppress the header)
    // is mangled by this shell, so the header is skipped by position instead.
    let out = Command::new("ps").args(["-axo", "pid,stat"]).output().expect("ps");
    let text = String::from_utf8_lossy(&out.stdout);
    let mut theirs = std::collections::HashMap::new();
    for line in text.lines().skip(1) {
        let mut f = line.split_whitespace();
        let (Some(pid), Some(stat)) = (f.next(), f.next()) else {
            continue;
        };
        if let (Ok(pid), Some(letter)) = (pid.parse::<u32>(), stat.chars().next()) {
            theirs.insert(pid, letter);
        }
    }
    assert!(!theirs.is_empty(), "ps listed nothing");

    let me = std::process::id();
    let mut compared = 0;
    let mut mismatched = Vec::new();
    for row in rows.iter().filter(|r| !r.is_thread) {
        if row.pid == me {
            continue; // transitioning between the two reads, by construction
        }
        let Some(&want) = theirs.get(&row.pid) else {
            continue; // exited between the two reads
        };
        compared += 1;
        let got = row.state as char;
        if got != want {
            mismatched.push((row.pid, row.comm.to_string(), got, want));
        }
    }

    assert!(
        compared >= 5,
        "only {compared} processes could be compared; ps and gander disagree on          which processes exist"
    );
    assert!(
        mismatched.is_empty(),
        "state letters disagree: {mismatched:?}"
    );
    // A sanity check that the mapping produces real letters, not `?`. `D` and
    // `L` are included: FreeBSD prints both and Linux has neither.
    let known = rows
        .iter()
        .filter(|r| matches!(r.state, b'R' | b'S' | b'I' | b'D' | b'T' | b'Z' | b'W' | b'L'))
        .count();
    assert!(
        known * 10 >= rows.len() * 9,
        "over 10% of rows have an undecodable state"
    );
}

#[test]
fn the_identity_strings_are_the_ones_ps_reports() {
    let me = std::process::id();
    let mut s = gander_core::Sampler::default();
    let (snap, rows) = s.sample();
    let mine = rows.iter().find(|r| r.pid == me).expect("own process");

    assert!(mine.cmdline.is_some(), "own cmdline is readable");
    assert!(mine.exe.is_some(), "own exe is readable");
    assert!(mine.cwd.is_some(), "own cwd is readable");

    // `ps -o args=` prints the same argv join, but appends ` (p_comm)` when the
    // title is too long for the column — `MAXCOMLEN` truncation is `ps`'s own
    // decoration, not part of argv.
    let out = Command::new("ps")
        .args(["-o", "args=", "-p", &me.to_string()])
        .output()
        .expect("ps");
    let args = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let cmdline = mine.cmdline.as_deref().unwrap_or("");
    assert_eq!(
        cmdline,
        args.split(" (").next().unwrap_or(&args),
        "cmdline must match ps's args column"
    );
    assert!(
        args.starts_with(cmdline),
        "ps's args {args:?} must extend gander's {cmdline:?}"
    );

    // The cwd must be the directory this test is running in.
    let expected_cwd = std::env::current_dir().expect("cwd").to_string_lossy().to_string();
    assert_eq!(
        mine.cwd.as_deref(),
        Some(expected_cwd.as_str()),
        "cwd must be the process's own working directory"
    );

    // The exe path must exist, and start with a slash.
    let exe = mine.exe.as_deref().expect("exe");
    assert!(exe.starts_with('/'), "exe {exe:?} is not absolute");
    assert!(std::path::Path::new(exe).exists(), "exe {exe:?} is not a file");

    // And the row must have the right owner.
    // SAFETY: `getuid` has no preconditions.
    assert_eq!(mine.uid, unsafe { libc::getuid() });
    let _ = snap;
}

#[test]
fn threads_are_listed_only_when_asked_for() {
    let mut s = gander_core::Sampler::default();
    let (_snap, rows) = s.sample();
    assert!(
        rows.iter().all(|r| !r.is_thread),
        "threads cost a syscall per process; they are off by default"
    );

    s.set_scan_options(gander_core::ScanOptions::none().with_threads(true));
    let (_snap, rows) = s.sample();
    let threads: Vec<&Row> = rows.iter().filter(|r| r.is_thread).collect();
    // A VM with kernel threads always has some.
    assert!(!threads.is_empty(), "this machine has multi-threaded processes");

    let mut seen = std::collections::HashSet::new();
    for t in &threads {
        let id = t.thread_id.expect("a thread row carries its own identity");
        assert!(seen.insert(id), "duplicate thread identity {id}");
        assert_ne!(id, t.pid as u64, "the leader must not be a thread row");
        assert_eq!(t.tgid, t.pid, "a thread's group is its process");
        assert!(t.nlwp == 1, "a thread row is one thread");
        // The process row must also exist, or the tree has an orphan.
        assert!(
            rows.iter().any(|r| !r.is_thread && r.pid == t.pid),
            "thread {id} has no process row"
        );
    }
}

#[test]
fn sorting_yields_a_permutation_and_is_stable() {
    let mut s = gander_core::Sampler::default();
    let (_snap, rows) = s.sample();
    let total = rows.len();
    let mut table = ProcTable {
        rows,
        ..Default::default()
    };
    let visibility = Visibility {
        show_kernel_threads: true,
        ..Default::default()
    };

    for field in [
        Field::Pid,
        Field::Comm,
        Field::PercentCpu,
        Field::ResMem,
        Field::Time,
        Field::Elapsed,
        Field::User,
        Field::Priority,
        Field::Nice,
        Field::State,
        Field::Session,
        Field::Tty,
        Field::Tpgid,
        Field::StartTime,
        Field::ProcExe,
        Field::Cwd,
    ] {
        for dir in [Dir::Asc, Dir::Desc] {
            table.sort = Sort { field, dir };
            table.reorder(&visibility);
            let order = table.view.order.clone();
            assert_eq!(order.len(), total, "{field:?} {dir:?} lost rows");
            let mut seen = vec![false; total];
            for &i in &order {
                assert!(!seen[i], "{field:?} {dir:?} repeated a row");
                seen[i] = true;
            }
            assert!(
                seen.iter().all(|s| *s),
                "{field:?} {dir:?} skipped a row"
            );
        }
    }

    // Ties keep the pid tie-break in both directions: a descending sort of a
    // column where every value is equal must still read in ascending pid order,
    // not backwards. That is the bug `reverse()` used to cause.
    let same = table.rows.iter().all(|r| r.percent_cpu == table.rows[0].percent_cpu);
    if same {
        table.sort = Sort {
            field: Field::PercentCpu,
            dir: Dir::Desc,
        };
        table.reorder(&visibility);
        let desc: Vec<u32> = table.view.order.iter().map(|&i| table.rows[i].pid).collect();
        table.sort = Sort {
            field: Field::PercentCpu,
            dir: Dir::Asc,
        };
        table.reorder(&visibility);
        let asc: Vec<u32> = table.view.order.iter().map(|&i| table.rows[i].pid).collect();
        assert_eq!(desc, asc, "ties must not reverse with direction");
    }
}

#[test]
fn filtering_keeps_the_matching_rows() {
    let mut s = gander_core::Sampler::default();
    let (_snap, rows) = s.sample();
    // The test binary's name is the integration-test target, and its command line
    // contains the path it was started from, so filtering on a substring proves
    // the predicate ran rather than that one particular row matched.
    let filter = gander_core::table::Filter::parse("gander").expect("filter");
    let matching = rows.iter().filter(|r| filter.matches(r)).count();
    let direct = rows
        .iter()
        .filter(|r| {
            r.display_command().contains("gander") || r.comm.as_str().contains("gander")
        })
        .count();
    assert_eq!(
        matching, direct,
        "the filter must agree with a direct search"
    );
    assert!(matching > 0, "this test binary's own row must match");
}

#[test]
fn the_hostname_meter_has_something_to_show() {
    // The `Hostname` meter is fed from `gethostname(2)` in the TUI rather than
    // from the snapshot, so what matters here is that the sysctl the backend uses
    // agrees with it.
    let want = sysctl("kern.hostname").expect("kern.hostname");
    let mut buf = [0u8; 256];
    // SAFETY: `buf` is 256 bytes and its length is passed.
    let rc = unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) };
    assert_eq!(rc, 0, "gethostname");
    let have = String::from_utf8_lossy(&buf)
        .trim_end_matches('\0')
        .to_string();
    assert_eq!(have, want, "kern.hostname and gethostname must agree");
}

#[test]
fn network_counters_match_netstat() {
    let mut s = gander_core::Sampler::default();
    let (_snap, rows) = s.sample();
    let (snap, _rows) = s.sample();

    // `netstat -ib` prints one line per address, with the byte counters in
    // columns 7 and 10; `-n` keeps it from resolving names (slow over serial).
    let out = match Command::new("netstat").arg("-ibn").output() {
        Ok(o) if o.status.success() => o,
        _ => return,
    };
    let text = String::from_utf8_lossy(&out.stdout);
    let mut ibytes = 0u64;
    let mut obytes = 0u64;
    let mut counted = std::collections::HashSet::new();
    for line in text.lines().skip(1) {
        let f: Vec<&str> = line.split_whitespace().collect();
        // Name Mtu Network Address Ipkts Ierrs Ibytes Opkts Oerrs Obytes Coll
        if f.len() < 10 || f[0].starts_with("lo") {
            continue;
        }
        // One row per address; the interface's counters repeat, so count once.
        if !counted.insert(f[0]) {
            continue;
        }
        ibytes += f[6].parse::<u64>().unwrap_or(0);
        obytes += f[9].parse::<u64>().unwrap_or(0);
    }
    if ibytes == 0 && obytes == 0 {
        return; // no traffic yet on this VM
    }

    // The two reads are seconds apart and the VM keeps talking to the host, so
    // the comparison is a lower bound plus a generous allowance.
    assert!(
        snap.net.rx_bytes >= ibytes,
        "rx {} is below netstat's {ibytes}",
        snap.net.rx_bytes
    );
    assert!(
        snap.net.tx_bytes >= obytes,
        "tx {} is below netstat's {obytes}",
        snap.net.tx_bytes
    );
    let _ = rows;
}

#[test]
fn disk_counters_are_readable_and_monotonic() {
    let mut s = gander_core::Sampler::default();
    let (a, _rows) = s.sample();
    std::thread::sleep(std::time::Duration::from_millis(120));
    let (b, _rows) = s.sample();

    // A VM with no disks is possible; a VM with disks must report some bytes.
    assert!(
        b.disk.read_bytes >= a.disk.read_bytes,
        "read counter went backwards"
    );
    assert!(
        b.disk.write_bytes >= a.disk.write_bytes,
        "write counter went backwards"
    );
    assert!(b.disk_rate.inbound.is_finite());
    assert!(b.disk_rate.outbound.is_finite());
    assert!(b.disk_rate.inbound >= 0.0);
}

#[test]
fn the_identity_cache_pays_for_the_paths_once() {
    let mut s = gander_core::Sampler::default();
    s.sample();
    let first = s.cache_len();
    assert!(first > 0, "every process got an identity");
    s.sample();
    let second = s.cache_len();
    // A steady machine reuses the whole cache; a churning one adds a few.
    assert!(
        second <= first + first / 4 + 16,
        "cache grew from {first} to {second} in one tick"
    );
}

#[test]
fn a_process_that_exits_leaves_no_stale_row() {
    // Fork a short-lived child and wait for it to disappear.
    let child = Command::new("sleep").arg("0.05").spawn().expect("spawn");
    let pid = child.id();
    let mut s = gander_core::Sampler::default();
    let (_snap, rows) = s.sample();
    let saw_it = rows.iter().any(|r| r.pid == pid);

    let mut child = child;
    let _ = child.wait();
    std::thread::sleep(std::time::Duration::from_millis(150));
    let (_snap, rows) = s.sample();
    assert!(
        !rows.iter().any(|r| r.pid == pid),
        "pid {pid} is gone but still listed"
    );
    // Not an assertion on `saw_it`: the child may have exited before the first
    // sample on a fast machine. What matters is the negative case.
    let _ = saw_it;
}
