use gander_core::metrics::{Availability, Metric, MetricAvailability};
use gander_core::row::{Field, Row};
use gander_core::table::{sort_order, Dir, Sort};

#[test]
fn unavailable_values_follow_real_zero_in_both_sort_directions() {
    for field in [
        Field::PercentCpu,
        Field::PercentNormCpu,
        Field::PercentMem,
        Field::ResMem,
        Field::VirtMem,
        Field::Time,
        Field::Priority,
        Field::Nlwp,
    ] {
        let mut unknown = Row::new(1);
        unknown.availability = MetricAvailability::all(Availability::PermissionDenied);
        let zero = Row::default_for_test(2);
        let mut busy = Row::default_for_test(3);
        busy.percent_cpu = 90.0;
        busy.percent_mem = 5.0;
        busy.res = 100;
        busy.virt = 100;
        busy.time_ticks = 100;
        busy.priority = 30;
        busy.nlwp = 4;
        let rows = [unknown, zero, busy];
        for dir in [Dir::Asc, Dir::Desc] {
            let mut order = [0, 1, 2];
            sort_order(&rows, &mut order, Sort { field, dir });
            assert_eq!(order[2], 0, "{field:?} {dir:?}");
        }
    }
}

#[cfg(unix)]
#[test]
fn fixture_cpu_is_unavailable_until_an_interval_exists_but_rss_is_measured() {
    use gander_core::PlatformSampler;
    let root = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/proc"));
    let mut sampler = gander_core::linux::system::Sampler::new(root);
    let (_, rows) = sampler.sample();
    for row in rows {
        assert_eq!(row.availability.get(Metric::Cpu), Availability::WarmingUp);
        assert_eq!(row.availability.get(Metric::Rss), Availability::Available);
    }
    let (_, rows) = sampler.sample();
    assert!(rows
        .iter()
        .all(|r| r.availability.get(Metric::Cpu) == Availability::WarmingUp));
}

#[cfg(target_os = "macos")]
#[test]
fn darwin_native_priority_and_availability_match_system_reads() {
    use gander_core::PlatformSampler;
    let pid = std::process::id();
    let native = gander_core::darwin::scan::task_info_checked(pid).unwrap();
    let mut sampler = gander_core::Sampler::default();
    let (_, rows) = sampler.sample();
    let row = rows.iter().find(|r| r.pid == pid).unwrap();
    assert_eq!(row.priority, native.priority.clamp(-128, 127) as i8);
    assert_ne!(row.priority, row.nice);
    assert_eq!(
        row.availability.get(Metric::Priority),
        Availability::Available
    );
    assert_eq!(row.availability.get(Metric::Cpu), Availability::WarmingUp);
    std::thread::sleep(std::time::Duration::from_millis(30));
    let (_, rows) = sampler.sample();
    assert_eq!(
        rows.iter()
            .find(|r| r.pid == pid)
            .unwrap()
            .availability
            .get(Metric::Cpu),
        Availability::Available
    );
}
