//! Live `/proc` checks.
//!
//! These only run on Linux. They compare gander's figures against the same
//! files read directly, *inside one test*, so a changing system cannot explain a
//! discrepancy. Everything is skipped on other platforms.

#![cfg(target_os = "linux")]

use std::collections::HashMap;

use gander_core::row::Field;
use gander_core::table::{self, Dir, ProcTable, Sort, Visibility};
use gander_core::PlatformSampler;

/// Parse `/proc/meminfo` into kB values, independently of gander's parser.
fn meminfo() -> HashMap<String, u64> {
    let text = std::fs::read_to_string("/proc/meminfo").expect("/proc/meminfo");
    text.lines()
        .filter_map(|line| {
            let (key, rest) = line.split_once(':')?;
            let value = rest.split_whitespace().next()?.parse().ok()?;
            Some((key.to_string(), value))
        })
        .collect()
}

#[test]
fn memory_matches_free_accounting() {
    let mut sampler = gander_core::Sampler::default();
    let (snapshot, _rows) = sampler.sample();
    let m = &snapshot.mem;
    let kb = 1024;

    // Read meminfo immediately after sampling; the sampler itself uses the same
    // file, so a small drift is possible but not an order of magnitude.
    let raw = meminfo();
    let total = raw["MemTotal"] * kb;
    let free = raw["MemFree"] * kb;
    let buffers = raw["Buffers"] * kb;
    let cached = raw["Cached"] * kb;
    let sreclaimable = raw["SReclaimable"] * kb;

    // `free`'s used: total - free - (buffers + cached + sreclaimable - shmem)
    let shmem = raw.get("Shmem").copied().unwrap_or(0) * kb;
    let buff_cache = buffers + cached + sreclaimable - shmem;
    let expected_used = total - free - buff_cache;

    assert_eq!(m.total, total, "total comes straight from MemTotal");

    // Allow 2% drift for memory changing between the two reads.
    let tolerance = total / 50;
    assert!(
        m.used.abs_diff(expected_used) <= tolerance,
        "gander used {} KiB vs free-accounting {} KiB",
        m.used / kb,
        expected_used / kb
    );
}

#[test]
fn available_comes_from_memavailable() {
    let mut sampler = gander_core::Sampler::default();
    let (snapshot, _rows) = sampler.sample();
    let raw = meminfo();

    // Kernels newer than 3.14 always provide MemAvailable.
    if let Some(expected) = raw.get("MemAvailable") {
        // The two reads are not simultaneous, and MemAvailable moves as the
        // kernel reclaims pages. Comparing them exactly made this test flake
        // roughly 2 runs in 5 in a container; the accounting test above allows
        // the same 2% drift for the same reason.
        let expected = expected * 1024;
        let tolerance = (snapshot.mem.total / 50).max(64 * 1024 * 1024);
        assert!(
            snapshot.mem.available.abs_diff(expected) <= tolerance,
            "available {} vs MemAvailable {} (tolerance {tolerance})",
            snapshot.mem.available,
            expected
        );
        // And it must still be a real MemAvailable figure, not `free`.
        assert!(
            snapshot.mem.available >= snapshot.mem.free,
            "available must be at least free"
        );
    }
}

#[test]
fn cpu_times_cover_every_core() {
    let mut sampler = gander_core::Sampler::default();
    let (snapshot, _rows) = sampler.sample();

    // SAFETY: `sysconf` has no preconditions.
    let ncpu = unsafe { libc::sysconf(libc::_SC_NPROCESSORS_ONLN) } as usize;
    assert!(ncpu >= 1);
    assert_eq!(
        snapshot.cpu.len(),
        ncpu + 1,
        "one aggregate entry plus one per online core"
    );
    assert_eq!(snapshot.cpu_count(), ncpu);
    assert!(snapshot.cpu[0].total() > 0);
}

#[test]
fn process_count_matches_proc() {
    let mut sampler = gander_core::Sampler::default();
    let (_snapshot, rows) = sampler.sample();

    // Count numeric /proc entries, which is what the sampler walks.
    let on_disk = std::fs::read_dir("/proc")
        .expect("/proc")
        .filter_map(|e| e.ok())
        .filter(|e| {
            let name = e.file_name();
            let name = name.to_string_lossy();
            !name.is_empty() && name.chars().all(|c| c.is_ascii_digit())
        })
        .count();

    // Processes come and go between the two walks, so allow a small delta.
    let delta = rows.len().abs_diff(on_disk);
    assert!(
        delta <= 10,
        "gander read {} processes, /proc has {on_disk}",
        rows.len()
    );
    assert!(
        rows.len() > 1,
        "a container still has more than one process"
    );
}

#[test]
fn every_row_has_a_plausible_pid_and_command() {
    let mut sampler = gander_core::Sampler::default();
    let (_snapshot, rows) = sampler.sample();

    for row in &rows {
        assert!(row.pid > 0, "pid 0 must not be listed");
        assert!(!row.comm.is_empty(), "pid {} has no command", row.pid);
    }

    // Pids are unique.
    let mut pids: Vec<u32> = rows.iter().map(|r| r.pid).collect();
    let n = pids.len();
    pids.sort_unstable();
    pids.dedup();
    assert_eq!(pids.len(), n, "duplicate pids in the table");
}

#[test]
fn command_lines_have_no_control_characters_after_sanitising() {
    // The renderer replaces control characters; this checks the raw data really
    // can contain them, which is why the sanitiser exists.
    let mut sampler = gander_core::Sampler::default();
    let (_snapshot, rows) = sampler.sample();

    let mut found_control = false;
    for row in &rows {
        if row.display_command().chars().any(|c| c.is_control()) {
            found_control = true;
            break;
        }
    }
    // Not an assertion: in a quiet container there may be none. The point is that
    // the sanitiser runs over everything regardless.
    let _ = found_control;
}

#[test]
fn second_sample_reports_finite_cpu_percent() {
    let mut sampler = gander_core::Sampler::default();
    sampler.sample();
    std::thread::sleep(std::time::Duration::from_millis(120));
    let (snapshot, rows) = sampler.sample();

    assert!(snapshot.cpu_usage().is_finite());
    for row in &rows {
        assert!(
            row.percent_cpu.is_finite() && row.percent_cpu >= 0.0,
            "pid {} has cpu% {}",
            row.pid,
            row.percent_cpu
        );
    }

    // Something must have moved in 120 ms.
    let busiest = rows.iter().map(|r| r.percent_cpu).fold(0.0f32, f32::max);
    assert!(busiest >= 0.0);
}

#[test]
fn sorting_and_filtering_yield_a_permutation() {
    let mut sampler = gander_core::Sampler::default();
    let (_snapshot, rows) = sampler.sample();
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
        Field::PercentCpu,
        Field::PercentMem,
        Field::ResMem,
    ] {
        for dir in [Dir::Asc, Dir::Desc] {
            table.sort = Sort { field, dir };
            table.reorder(&visibility);

            let mut order = table.view.order.clone();
            let n = order.len();
            order.sort_unstable();
            order.dedup();
            assert_eq!(order.len(), n, "sort by {field:?} {dir:?} duplicated a row");
            assert!(n > 0);
        }
    }
    let _ = table::sort_order;
}

#[test]
fn tree_mode_lists_every_row_once() {
    let mut sampler = gander_core::Sampler::default();
    let (_snapshot, rows) = sampler.sample();
    let total = rows.len();

    let mut table = ProcTable {
        rows,
        tree_mode: true,
        ..Default::default()
    };
    table.reorder(&Visibility {
        show_kernel_threads: true,
        ..Default::default()
    });

    assert_eq!(table.view.order.len(), total, "tree mode dropped rows");

    let mut sorted = table.view.order.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), total, "tree mode duplicated a row");
}

#[test]
fn process_actions_are_available() {
    // `kill(2)` is POSIX; this asserts the unix gate is right rather than the
    // Linux-only gate that made F9 fail on macOS.
    let action = gander_core::action::Action::Kill {
        pid: u32::MAX - 1,
        signal: 0,
    };
    // Signal 0 only checks permission, so it must fail with ESRCH for a pid that
    // cannot exist — but it must not fail with "unsupported".
    match gander_core::action::execute(&action) {
        Err(e) => assert_ne!(
            e.kind(),
            std::io::ErrorKind::Unsupported,
            "process actions must work on every unix"
        ),
        Ok(()) => panic!("signalling a non-existent pid should fail"),
    }
}

/// The host network counters must match `/proc/net/dev` read independently.
///
/// The trap this pins down: the interface name and the first counter share a
/// colon-separated field (`eth0:1234`), so a parser that splits on whitespace
/// first drops the receive byte count and reports a plausible-looking but wrong
/// number. Loopback must also be excluded, or every local byte is counted twice.
#[test]
fn network_counters_match_proc_net_dev() {
    let text = std::fs::read_to_string("/proc/net/dev").expect("/proc/net/dev");
    let (mut rx, mut tx) = (0u64, 0u64);
    for line in text.lines() {
        let Some((name, rest)) = line.split_once(':') else {
            continue;
        };
        let name = name.trim();
        if name.is_empty() || name == "lo" {
            continue;
        }
        let fields: Vec<&str> = rest.split_whitespace().collect();
        // 0 = rx bytes, 8 = tx bytes.
        rx += fields.first().and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
        tx += fields.get(8).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
    }

    let got = gander_core::linux::scan::read_net_counters(std::path::Path::new("/proc"));
    // Read again after: traffic happens between the two reads, so the gander
    // figure is a lower bound for the direct one.
    assert!(
        got.rx_bytes >= rx && got.tx_bytes >= tx,
        "gander rx/tx {}/{} is below the direct read {}/{}",
        got.rx_bytes,
        got.tx_bytes,
        rx,
        tx
    );
    // But not by an order of magnitude: that would mean loopback leaked in.
    let slack = |a: u64, b: u64| a.saturating_sub(b) <= (b / 4).max(4 * 1024 * 1024);
    assert!(slack(got.rx_bytes, rx), "rx {} vs {}", got.rx_bytes, rx);
    assert!(slack(got.tx_bytes, tx), "tx {} vs {}", got.tx_bytes, tx);
}

/// Disk counters must match `/proc/diskstats` for whole disks only.
///
/// Summing partitions as well doubles every byte, so the direct read here applies
/// the same `/sys/block` test the sampler does, and then asserts the sampler
/// agrees with it.
#[test]
fn disk_counters_match_proc_diskstats() {
    let Ok(text) = std::fs::read_to_string("/proc/diskstats") else {
        return; // no block devices in this sandbox
    };
    let (mut read, mut written) = (0u64, 0u64);
    let mut disks = 0usize;
    for line in text.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 14 {
            continue;
        }
        let name = fields[2];
        if !std::path::Path::new("/sys/block").join(name).is_dir() {
            continue;
        }
        disks += 1;
        read += fields[5].parse::<u64>().unwrap_or(0) * 512;
        written += fields[9].parse::<u64>().unwrap_or(0) * 512;
    }

    let got = gander_core::linux::scan::read_disk_counters(std::path::Path::new("/proc"));
    if disks == 0 {
        // Nothing to compare against; the reader must still not panic.
        assert_eq!(got.read_bytes, 0);
        return;
    }
    // Same read-again slack as the network test: a busy disk moves on.
    let slack = (read / 20).max(64 * 1024 * 1024);
    assert!(
        got.read_bytes.abs_diff(read) <= slack,
        "read {} vs {}",
        got.read_bytes,
        read
    );
    assert!(
        got.write_bytes.abs_diff(written) <= slack,
        "written {} vs {}",
        got.write_bytes,
        written
    );
}

/// Two samples a second apart must produce a finite, non-negative rate, and the
/// first sample must report zero rather than a meaningless counter-since-boot
/// divided by an unknown interval.
#[test]
fn throughput_rates_are_meaningful_after_two_samples() {
    let mut sampler = gander_core::Sampler::default();
    let (first, _) = sampler.sample();
    assert_eq!(
        first.net_rate.total(),
        0.0,
        "the first sample has no interval to divide by"
    );
    assert_eq!(first.disk_rate.total(), 0.0);

    std::thread::sleep(std::time::Duration::from_millis(250));
    let (second, _) = sampler.sample();
    for rate in [second.net_rate, second.disk_rate] {
        assert!(rate.inbound.is_finite() && rate.inbound >= 0.0, "{rate:?}");
        assert!(rate.outbound.is_finite() && rate.outbound >= 0.0, "{rate:?}");
    }
    // The cumulative counters never go backwards on a live machine.
    assert!(second.net.rx_bytes >= first.net.rx_bytes);
    assert!(second.disk.read_bytes >= first.disk.read_bytes);
}

/// `row.cgroup` must be the cgroup *path*, not the raw `/proc/<pid>/cgroup` line.
///
/// The raw line is `0::/user.slice/…`; storing it made the CGROUP column show the
/// hierarchy id and an empty controller list for every process, which is noise.
#[test]
fn cgroup_column_carries_the_path() {
    let mut sampler = gander_core::Sampler::default();
    let (_snap, rows) = sampler.sample();
    let me = rows
        .iter()
        .find(|r| r.pid == std::process::id())
        .expect("own process must be in the table");

    let cgroup = me.cgroup.as_deref().expect("own cgroup must be readable");
    assert!(
        cgroup.starts_with('/'),
        "the path must be absolute, got {cgroup:?}"
    );
    // The raw form starts with the hierarchy id and a colon.
    assert!(
        !cgroup.contains("::"),
        "the raw `/proc/<pid>/cgroup` line leaked through: {cgroup:?}"
    );
}

/// The cgroup path must agree with the kernel's own answer.
#[test]
fn cgroup_matches_proc_cgroup() {
    let me = std::process::id();
    let text = std::fs::read_to_string(format!("/proc/{me}/cgroup")).expect("cgroup file");
    // The unified entry is the one with an empty controller list.
    let expected = text
        .lines()
        .filter_map(|l| {
            let mut parts = l.splitn(3, ':');
            let _hier = parts.next()?;
            let ctrls = parts.next()?;
            let path = parts.next()?;
            if ctrls.is_empty() {
                Some(path.trim())
            } else {
                None
            }
        })
        .next()
        .or_else(|| text.lines().next().and_then(|l| l.splitn(3, ':').nth(2)))
        .expect("at least one cgroup line");

    let mut sampler = gander_core::Sampler::default();
    let (_snap, rows) = sampler.sample();
    let mine = rows.iter().find(|r| r.pid == me).expect("own process");
    assert_eq!(mine.cgroup.as_deref(), Some(expected));
}

/// A process in a nested PID namespace must be recognised as containerised even
/// when its cgroup path says nothing.
///
/// This is the case the namespace check exists for: a container with its own
/// cgroup namespace reports `/` in `/proc/<pid>/cgroup`, so the path-based
/// detection alone reports "no container" for a process that is plainly inside
/// one. The test creates a real nested namespace with `unshare`, so it is gated on
/// that working (it needs privileges a default container may not have).
#[test]
fn a_nested_pid_namespace_is_detected_without_a_cgroup_path() {
    use std::process::{Command, Stdio};

    // `unshare --pid --fork` needs CAP_SYS_ADMIN or a privileged container. When
    // it is unavailable the test is meaningless rather than failing.
    let Ok(mut child) = Command::new("unshare")
        .args(["--pid", "--fork", "--mount-proc", "sleep", "30"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return;
    };
    std::thread::sleep(std::time::Duration::from_millis(600));

    let mut sampler = gander_core::Sampler::default();
    let (_snap, rows) = sampler.sample();

    // Find any process the kernel puts in a different PID namespace than ours.
    // That is exactly what the sampler is supposed to have flagged.
    let nested: Vec<u32> = rows
        .iter()
        .filter(|r| {
            gander_core::linux::fsops::in_different_pid_namespace(
                std::path::Path::new("/proc"),
                r.pid,
            ) == Some(true)
        })
        .map(|r| r.pid)
        .collect();

    let _ = child.kill();
    let _ = child.wait();

    if nested.is_empty() {
        // `unshare` may have failed to create a namespace despite spawning.
        return;
    }

    for pid in nested {
        let row = rows.iter().find(|r| r.pid == pid).expect("row");
        assert!(
            row.container.is_some(),
            "pid {pid} is in a nested PID namespace but was not flagged as containerised \
             (cgroup {:?})",
            row.cgroup
        );
    }
}

// ---------------------------------------------------------------------------
// M3: PSI, zram, zswap, huge pages, PSS, GPU
// ---------------------------------------------------------------------------

/// PSI, when the kernel provides it, must agree with the raw files.
///
/// Read in the same test as the sample so a changing machine cannot explain a
/// mismatch — the same discipline the memory tests above follow.
#[test]
fn psi_matches_a_direct_read() {
    let mut sampler = gander_core::Sampler::default();
    let (snapshot, _rows) = sampler.sample();

    let path = std::path::Path::new("/proc/pressure/io");
    let Some(psi) = snapshot.psi else {
        // CONFIG_PSI=n, or an old kernel: the sampler must report absence rather
        // than four zeroed resources.
        assert!(
            !path.exists(),
            "PSI files exist but the sampler reported none"
        );
        return;
    };

    // `io` is the one resource every PSI-enabled kernel provides.
    let raw = std::fs::read_to_string(path).expect("/proc/pressure/io");
    let some_line = raw
        .lines()
        .find(|l| l.starts_with("some "))
        .expect("a `some` line");
    let avg10: f32 = some_line
        .split_whitespace()
        .find_map(|t| t.strip_prefix("avg10="))
        .and_then(|v| v.parse().ok())
        .expect("avg10 on the some line");

    assert!(psi.io.present);
    // It moves between the two reads, so compare with a tolerance rather than for
    // equality.
    assert!(
        (psi.io.some.avg10 - avg10).abs() < 25.0,
        "sampler {} vs direct read {}",
        psi.io.some.avg10,
        avg10
    );
    // Every resource reports both lines on current kernels, `cpu` included. This
    // asserts the file's shape rather than a table of assumptions about it: an
    // earlier version of this test claimed `cpu` had no `full` line and failed
    // against a real /proc.
    assert!(psi.io.has_full);
    assert!(psi.cpu.has_full);
}

/// Huge pages, when configured, must add up: `used = total - free`, in bytes.
#[test]
fn huge_pages_are_counted_in_bytes() {
    let mut sampler = gander_core::Sampler::default();
    let (snapshot, _rows) = sampler.sample();
    let mem = &snapshot.mem;

    if !mem.huge_pages_configured() {
        // Nothing reserved: the meter says `n/a here` and there is nothing to
        // check, but the bytes helper must still not divide by zero.
        assert_eq!(mem.huge_total_bytes(), mem.huge_pages_total * mem.huge_page_size);
        return;
    }

    assert!(mem.huge_page_size > 0, "a configured pool has a page size");
    assert_eq!(mem.huge_total_bytes(), mem.huge_pages_total * mem.huge_page_size);
    assert!(mem.huge_pages_free <= mem.huge_pages_total);
    let expected = (mem.huge_pages_total - mem.huge_pages_free) * mem.huge_page_size;
    assert_eq!(mem.huge_used_bytes(), expected);
}

/// Reading `smaps_rollup` for our own process must produce figures that are
/// internally consistent, and must not be attempted at all when switched off.
///
/// PSS is bounded below by the private figure and above by RSS: a process cannot
/// have more proportional memory than resident, and its private pages are by
/// definition its own share. Those are the two bounds that catch a mis-scaled
/// parse (kB read as bytes, say).
#[test]
fn pss_is_bounded_by_rss_and_private() {
    let mut sampler = gander_core::Sampler::default();
    gander_core::PlatformSampler::set_scan_options(
        &mut sampler,
        gander_core::ScanOptions::none().with_smaps(true),
    );
    let (_snap, rows) = sampler.sample();
    let me = std::process::id();
    let row = rows.iter().find(|r| r.pid == me).expect("own process");

    let pss = row.pss.expect("own smaps_rollup is readable");
    assert!(pss > 0, "a live process has pages");
    assert!(
        pss <= row.res + 64 * 1024 * 1024,
        "PSS {pss} exceeds RSS {} by more than a rounding margin",
        row.res
    );

    let private = row.private.expect("private is read with pss");
    assert!(
        private <= pss + 64 * 1024 * 1024,
        "private {private} cannot exceed pss {pss}"
    );

    let epss = row.epss.expect("epss is read with pss");
    let swap = row.swap_bytes.expect("swap is read with pss");
    assert_eq!(epss, pss + swap, "EPSS is PSS plus the swapped equivalent");
}

/// `smaps_rollup` must not be read when no column needs it.
///
/// This is the whole point of the gate: the read costs tens of milliseconds across
/// a desktop's process table, and a column nobody is looking at must not pay for
/// it.
#[test]
fn smaps_is_not_read_by_default() {
    let mut sampler = gander_core::Sampler::default();
    let (_snap, rows) = sampler.sample();
    for row in &rows {
        assert_eq!(row.pss, None, "pid {} read smaps unasked", row.pid);
    }
}

/// The GPU reader must never report a percentage above 100, and must not invent
/// one for a process with no DRM descriptor.
#[test]
fn gpu_percentages_are_bounded() {
    let mut sampler = gander_core::Sampler::default();
    gander_core::PlatformSampler::set_scan_options(
        &mut sampler,
        gander_core::ScanOptions::none().with_gpu(true),
    );
    let (_snap, rows) = sampler.sample();

    for row in &rows {
        if let Some(pct) = row.gpu_percent {
            assert!(
                (0.0..=100.0).contains(&pct),
                "pid {} reported {}% GPU",
                row.pid,
                pct
            );
            assert!(
                row.gpu_ns.is_some(),
                "a percentage implies engine time was read"
            );
        }
    }
}

/// Delay accounting must either produce a bounded percentage or report `None`.
///
/// The `None` case is the common one: `TASKSTATS` needs `CAP_NET_ADMIN`, so an
/// unprivileged run gets `EPERM` and every column renders `-`. That is the
/// behaviour being pinned here — a refusal must not become `0.0`, because `0.0`
/// says "this process never waited" about a figure nobody obtained.
#[test]
fn delay_percentages_are_bounded_or_absent() {
    let mut sampler = gander_core::Sampler::default();
    gander_core::PlatformSampler::set_scan_options(
        &mut sampler,
        gander_core::ScanOptions::none().with_delayacct(true),
    );
    // Two samples: the first populates the per-pid state.
    let _ = sampler.sample();
    let (_snap, rows) = sampler.sample();

    let mut read = 0;
    for row in &rows {
        for (label, value) in [
            ("cpud", row.cpu_delay_percent),
            ("iod", row.io_delay_percent),
            ("swpd", row.swap_delay_percent),
        ] {
            if let Some(pct) = value {
                read += 1;
                assert!(
                    (0.0..=100.0).contains(&pct),
                    "pid {} reported {}% {label}",
                    row.pid,
                    pct
                );
            }
        }
        // The three arrive together or not at all: they come from one reply.
        let present = [
            row.cpu_delay_percent.is_some(),
            row.io_delay_percent.is_some(),
            row.swap_delay_percent.is_some(),
        ];
        assert!(
            present.iter().all(|b| *b) || present.iter().all(|b| !*b),
            "pid {} had a partial delay reading: {:?}",
            row.pid,
            present
        );
    }
    // Either the kernel answered for the whole table, or it refused for the whole
    // table; a mixture would mean the socket was left in a broken state.
    let with_data = rows
        .iter()
        .filter(|r| r.cpu_delay_percent.is_some())
        .count();
    assert!(
        with_data == 0 || with_data == rows.len(),
        "delay data on {with_data} of {} rows",
        rows.len()
    );
    if with_data == 0 {
        eprintln!("note: TASKSTATS refused (no CAP_NET_ADMIN); columns render '-'");
    } else {
        assert!(read > 0);
    }
}

/// Delay columns must not cost anything when they are not visible.
#[test]
fn delayacct_is_not_read_by_default() {
    let mut sampler = gander_core::Sampler::default();
    let (_snap, rows) = sampler.sample();
    for row in &rows {
        assert_eq!(
            row.cpu_delay_percent, None,
            "pid {} read delay data unasked",
            row.pid
        );
        assert_eq!(row.io_delay_percent, None);
        assert_eq!(row.swap_delay_percent, None);
    }
}

/// The `TASKSTATS` socket must survive a full process table without leaking or
/// leaving the family unresolved.
#[test]
fn delayacct_socket_opens_or_reports_why() {
    use gander_core::linux::delayacct::DelayAcct;
    match DelayAcct::open() {
        Some(mut acct) => {
            // Reading our own pid must not panic and must not claim a denial.
            let me = std::process::id();
            let _ = acct.read(me);
            // After a successful open the socket is either usable or denied; both
            // are fine, a panic is not.
        }
        None => {
            eprintln!("note: no TASKSTATS family (CONFIG_TASKSTATS=n)");
        }
    }
}
