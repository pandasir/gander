//! Integration tests against a fixture `/proc` tree.
//!
//! These run on any unix: the sampler is pointed at `tests/fixtures/proc` instead
//! of the live `/proc`, so the whole read path is exercised without touching the
//! host (`docs/DESIGN.md` §10).

#![cfg(unix)]

use std::path::Path;

use gander_core::linux::system::Sampler;
use gander_core::row::RowFlags;
use gander_core::table::{self, Dir, Filter, ProcTable, Sort, Visibility};
use gander_core::PlatformSampler;

fn fixture_root() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/proc"))
}

#[test]
fn fixture_directory_is_present() {
    assert!(fixture_root().exists(), "fixtures must be committed");
}

#[test]
fn lists_exactly_the_fixture_pids() {
    let mut pids = gander_core::linux::scan::list_pids(fixture_root()).unwrap();
    pids.sort_unstable();
    assert_eq!(pids, vec![1, 42, 1337, 2048]);
}

#[test]
fn first_sample_reads_every_process() {
    let mut sampler = Sampler::new(fixture_root());
    let (snapshot, rows) = sampler.sample();

    assert_eq!(rows.len(), 4, "one row per fixture pid, got {rows:?}");
    assert_eq!(snapshot.cpu.len(), 3, "aggregate plus two cores");
    assert_eq!(snapshot.load, [0.52, 0.58, 0.59]);
    assert_eq!(snapshot.uptime, 3725);
    assert_eq!(snapshot.swap.used, 500_000 * 1024);
    // GNU `free` accounting: total - free - cached - sreclaimable - buffers
    //   = 16e6 - 1e6 - 2e6 - 5e5 - 1e5 = 12.4e6 kB
    assert_eq!(snapshot.mem.used, 12_400_000 * 1024);
    assert_eq!(snapshot.mem.available, 8_000_000 * 1024);
}

#[test]
fn tricky_comm_is_preserved() {
    let mut sampler = Sampler::new(fixture_root());
    let (_snap, rows) = sampler.sample();
    let weird = rows.iter().find(|r| r.pid == 42).expect("pid 42");
    assert_eq!(weird.comm.as_str(), "my ) we(ird comm");
    assert_eq!(weird.uid, 1000);
    assert_eq!(weird.nlwp, 4);
}

#[test]
fn cmdline_and_cgroup_are_loaded() {
    let mut sampler = Sampler::new(fixture_root());
    let (_snap, rows) = sampler.sample();

    let init = rows.iter().find(|r| r.pid == 1).expect("pid 1");
    assert_eq!(init.display_command(), "/sbin/init splash");
    // The *path*, not the raw `0::/init.scope` line: the hierarchy id and the
    // controller list are noise in a column.
    assert_eq!(init.cgroup.as_deref(), Some("/init.scope"));
    assert!(!init.is_kernel_thread());

    let weird = rows.iter().find(|r| r.pid == 42).expect("pid 42");
    assert_eq!(weird.display_command(), "/usr/bin/weird --with=a b");
    assert_eq!(
        weird.cgroup.as_deref(),
        Some("/user.slice/user-1000.slice/session-1.scope")
    );

    // The fixture has one containerised process: pid 1337 sits under
    // `/docker/abc123` on two v1 hierarchies. It must be identified, not just
    // flagged — every containerised process reporting the bare word "docker"
    // made the column carry no information.
    //
    // Container detection is cgroup-based and therefore Linux-only; this whole
    // file runs on any unix, so the assertions are gated rather than dropped.
    #[cfg(target_os = "linux")]
    {
        let containerised = rows.iter().find(|r| r.pid == 1337).expect("pid 1337");
        assert_eq!(
            containerised.cgroup.as_deref(),
            Some("/docker/abc123"),
            "v1-only host: the first entry is the best answer"
        );
        assert_eq!(containerised.container.as_deref(), Some("abc123"));
        assert_eq!(containerised.container_runtime.as_deref(), Some("docker"));

        // A host process must not be classified as containerised.
        assert_eq!(init.container, None);
        assert_eq!(init.container_runtime, None);
        assert_eq!(weird.container, None);
    }
}

#[test]
fn kernel_thread_is_classified() {
    let mut sampler = Sampler::new(fixture_root());
    let (_snap, rows) = sampler.sample();
    let kworker = rows.iter().find(|r| r.pid == 2048).expect("pid 2048");
    assert!(
        kworker.flags.contains(RowFlags::KERNEL_THREAD),
        "a thread with no cmdline and no exe is a kernel thread"
    );
}

#[test]
fn zombie_has_empty_cmdline_and_falls_back_to_comm() {
    let mut sampler = Sampler::new(fixture_root());
    let (_snap, rows) = sampler.sample();
    let zombie = rows.iter().find(|r| r.pid == 1337).expect("pid 1337");
    assert_eq!(zombie.state, b'Z');
    assert_eq!(zombie.display_command(), "zombie");
    assert_eq!(zombie.res, 0);
}

#[test]
fn memory_accounting_from_fixtures() {
    let mut sampler = Sampler::new(fixture_root());
    let (_snap, rows) = sampler.sample();
    let init = rows.iter().find(|r| r.pid == 1).expect("pid 1");
    // 1024 rss pages times the page size reported by the platform.
    let page = gander_core::CurrentPlatform::page_size();
    assert_eq!(init.res, 1024 * page);
    assert_eq!(init.virt, 4_096_000);
}

#[test]
fn first_sample_reports_zero_cpu() {
    let mut sampler = Sampler::new(fixture_root());
    let (_snap, rows) = sampler.sample();
    for row in &rows {
        assert_eq!(
            row.percent_cpu, 0.0,
            "no delta is available on the first tick"
        );
    }
}

#[test]
fn identity_is_cached_between_ticks() {
    let mut sampler = Sampler::new(fixture_root());
    sampler.sample();
    let after_first = sampler.cache_len();
    assert_eq!(after_first, 4);

    let (_snap, rows) = sampler.sample();
    assert_eq!(sampler.cache_len(), 4, "cache size must not grow");
    // The data still has to be correct on the second tick.
    let init = rows.iter().find(|r| r.pid == 1).unwrap();
    assert_eq!(init.display_command(), "/sbin/init splash");
}

#[test]
fn sorting_and_filtering_round_trip() {
    let mut sampler = Sampler::new(fixture_root());
    let (_snap, rows) = sampler.sample();

    let mut table = ProcTable {
        rows,
        ..Default::default()
    };
    table.sort = Sort {
        field: gander_core::row::Field::Pid,
        dir: Dir::Asc,
    };
    table.reorder(&Visibility {
        show_kernel_threads: true,
        ..Default::default()
    });

    let pids: Vec<u32> = table
        .view
        .order
        .iter()
        .map(|&i| table.rows[i].pid)
        .collect();
    assert_eq!(pids, vec![1, 42, 1337, 2048]);

    table.sort.dir = Dir::Desc;
    table.reorder(&Visibility {
        show_kernel_threads: true,
        ..Default::default()
    });
    let pids: Vec<u32> = table
        .view
        .order
        .iter()
        .map(|&i| table.rows[i].pid)
        .collect();
    assert_eq!(pids, vec![2048, 1337, 42, 1]);

    let filter = Filter::parse("weird").unwrap();
    table.reorder(&Visibility {
        text: Some(filter),
        show_kernel_threads: true,
        ..Default::default()
    });
    assert_eq!(table.visible(), 1);
    assert_eq!(table.row_at(0).unwrap().pid, 42);
}

#[test]
fn tree_mode_orders_parent_before_child() {
    let mut sampler = Sampler::new(fixture_root());
    let (_snap, rows) = sampler.sample();

    let mut table = ProcTable {
        rows,
        tree_mode: true,
        ..Default::default()
    };
    table.reorder(&Visibility {
        show_kernel_threads: true,
        ..Default::default()
    });

    let order: Vec<u32> = table
        .view
        .order
        .iter()
        .map(|&i| table.rows[i].pid)
        .collect();
    assert_eq!(order.len(), 4);
    // pid 1 is the parent of 42, and 42 of 1337.
    let pos = |pid: u32| order.iter().position(|p| *p == pid).unwrap();
    assert!(pos(1) < pos(42));
    assert!(pos(42) < pos(1337));
}

#[test]
fn delta_math_is_applied_on_the_second_tick() {
    // Two samples of the same fixture: counters do not advance, so CPU% stays 0.
    // This guards against a division that would otherwise produce NaN.
    let mut sampler = Sampler::new(fixture_root());
    sampler.sample();
    let (snapshot, rows) = sampler.sample();
    for row in &rows {
        assert!(row.percent_cpu.is_finite(), "cpu% must never be NaN/inf");
    }
    for delta in &snapshot.cpu_delta {
        assert_eq!(delta.total(), 0, "fixture counters are static");
    }
}

struct CpuFixture(std::path::PathBuf);

impl CpuFixture {
    fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "gander-cpu-fixture-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        let fixture = Self(root);
        std::fs::create_dir(fixture.0.join("42")).unwrap();
        for name in ["meminfo", "uptime", "loadavg"] {
            std::fs::copy(fixture_root().join(name), fixture.0.join(name)).unwrap();
        }
        for name in ["status", "cmdline", "cgroup"] {
            std::fs::copy(
                fixture_root().join("42").join(name),
                fixture.0.join("42").join(name),
            )
            .unwrap();
        }
        fixture
    }

    fn write(&self, cores: usize, user_per_core: u64, idle_per_core: u64, task_ticks: u64) {
        let line =
            |label: &str, user: u64, idle: u64| format!("{label} {user} 0 0 {idle} 0 0 0 0 0 0\n");
        let mut host = line(
            "cpu",
            user_per_core * cores as u64,
            idle_per_core * cores as u64,
        );
        for core in 0..cores {
            host.push_str(&line(&format!("cpu{core}"), user_per_core, idle_per_core));
        }
        std::fs::write(self.0.join("stat"), host).unwrap();

        let original = std::fs::read_to_string(fixture_root().join("42/stat")).unwrap();
        let end_comm = original.rfind(')').unwrap() + 1;
        let mut fields: Vec<_> = original[end_comm..]
            .split_whitespace()
            .map(str::to_owned)
            .collect();
        fields[11] = task_ticks.to_string();
        fields[12] = "0".into();
        std::fs::write(
            self.0.join("42/stat"),
            format!("{} {}\n", &original[..end_comm], fields.join(" ")),
        )
        .unwrap();
    }

    fn cpu(&self, sampler: &mut Sampler) -> f32 {
        let (_, rows) = sampler.sample();
        rows.iter().find(|r| r.pid == 42).unwrap().percent_cpu
    }
}

impl Drop for CpuFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn advancing_fixture_scales_one_busy_core_to_one_hundred_percent() {
    let fixture = CpuFixture::new();
    fixture.write(2, 100, 100, 500);
    let mut sampler = Sampler::new(&fixture.0);
    assert_eq!(fixture.cpu(&mut sampler), 0.0, "initial host sample");
    fixture.write(2, 150, 150, 600);
    assert_eq!(fixture.cpu(&mut sampler), 100.0);
    assert_eq!(fixture.cpu(&mut sampler), 0.0, "unchanged counters");
}

#[test]
fn advancing_fixture_supports_a_two_core_process() {
    let fixture = CpuFixture::new();
    fixture.write(2, 100, 100, 500);
    let mut sampler = Sampler::new(&fixture.0);
    fixture.cpu(&mut sampler);
    fixture.write(2, 200, 100, 700);
    assert_eq!(fixture.cpu(&mut sampler), 200.0);
}

#[test]
fn advancing_fixture_resets_the_interval_when_cpu_topology_changes() {
    let fixture = CpuFixture::new();
    fixture.write(2, 100, 100, 500);
    let mut sampler = Sampler::new(&fixture.0);
    fixture.cpu(&mut sampler);
    fixture.write(3, 150, 150, 600);
    assert_eq!(
        fixture.cpu(&mut sampler),
        0.0,
        "topology change resets the host delta"
    );
    fixture.write(3, 200, 200, 700);
    assert_eq!(
        fixture.cpu(&mut sampler),
        100.0,
        "next interval uses three cores"
    );
}

#[test]
fn proc_table_reorder_is_a_permutation() {
    let mut sampler = Sampler::new(fixture_root());
    let (_snap, rows) = sampler.sample();
    let mut table = ProcTable {
        rows,
        ..Default::default()
    };

    for field in [
        gander_core::row::Field::Pid,
        gander_core::row::Field::PercentCpu,
        gander_core::row::Field::PercentMem,
        gander_core::row::Field::Comm,
        gander_core::row::Field::ResMem,
    ] {
        for dir in [Dir::Asc, Dir::Desc] {
            table.sort = Sort { field, dir };
            table.reorder(&Visibility {
                show_kernel_threads: true,
                ..Default::default()
            });
            let mut sorted = table.view.order.clone();
            sorted.sort_unstable();
            assert_eq!(
                sorted,
                vec![0, 1, 2, 3],
                "sort by {field:?} {dir:?} must be a permutation"
            );
        }
    }
    let _ = table::sort_order;
}
