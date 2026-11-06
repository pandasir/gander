use std::collections::BTreeMap;

use gander_core::metrics::Metric;
use gander_core::row::Row;
use serde::Serialize;

#[derive(Serialize)]
pub struct Process<'a> {
    pid: u32,
    ppid: u32,
    uid: u32,
    comm: &'a str,
    state: char,
    cpu_percent: Option<f32>,
    mem_percent: Option<f32>,
    rss: Option<u64>,
    vsize: Option<u64>,
    nice: i8,
    priority: Option<i8>,
    threads: Option<i32>,
    elapsed: u64,
    is_thread: bool,
    identity: u64,
    cmdline: &'a str,
    memory: serde_json::Value,
    availability: BTreeMap<&'static str, &'static str>,
}

impl<'a> From<&'a Row> for Process<'a> {
    fn from(row: &'a Row) -> Self {
        let valid = |metric| row.availability.get(metric).is_available();
        Self {
            pid: row.pid,
            ppid: row.ppid,
            uid: row.uid,
            comm: row.comm.as_str(),
            state: row.state as char,
            cpu_percent: valid(Metric::Cpu).then_some((row.percent_cpu * 100.0).round() / 100.0),
            mem_percent: valid(Metric::Memory).then_some((row.percent_mem * 100.0).round() / 100.0),
            rss: valid(Metric::Rss).then_some(row.res),
            vsize: valid(Metric::Virtual).then_some(row.virt),
            nice: row.nice,
            priority: valid(Metric::Priority).then_some(row.priority),
            threads: valid(Metric::Threads).then_some(row.nlwp),
            elapsed: row.elapsed,
            is_thread: row.is_thread,
            identity: row.identity(),
            cmdline: row.display_command(),
            memory: memory(row),
            availability: [
                ("cpu_percent", Metric::Cpu),
                ("mem_percent", Metric::Memory),
                ("rss", Metric::Rss),
                ("vsize", Metric::Virtual),
                ("priority", Metric::Priority),
                ("threads", Metric::Threads),
            ]
            .into_iter()
            .map(|(key, metric)| (key, row.availability.get(metric).as_str()))
            .collect(),
        }
    }
}

#[cfg(target_os = "linux")]
fn memory(row: &Row) -> serde_json::Value {
    serde_json::json!({
        "pss": row.pss, "epss": row.epss, "private": row.private, "swap": row.swap_bytes,
        "gpu_percent": row.gpu_percent, "gpu_ns": row.gpu_ns,
        "cpu_delay_percent": row.cpu_delay_percent, "io_delay_percent": row.io_delay_percent,
        "swap_delay_percent": row.swap_delay_percent,
    })
}

#[cfg(not(target_os = "linux"))]
fn memory(_row: &Row) -> serde_json::Value {
    serde_json::Value::Null
}

#[cfg(test)]
mod tests {
    use super::*;
    use gander_core::metrics::{Availability, MetricAvailability};

    #[test]
    fn nullable_metrics_and_known_zero_have_distinct_contracts() {
        let mut row = Row::default_for_test(1);
        let value = serde_json::to_value(Process::from(&row)).unwrap();
        for key in ["cpu_percent", "mem_percent", "rss", "vsize"] {
            assert_eq!(value[key].as_f64(), Some(0.0));
            assert_eq!(value["availability"][key], "available");
        }
        row.availability = MetricAvailability::all(Availability::PermissionDenied);
        row.availability.set(Metric::Cpu, Availability::WarmingUp);
        let value = serde_json::to_value(Process::from(&row)).unwrap();
        for key in [
            "cpu_percent",
            "mem_percent",
            "rss",
            "vsize",
            "priority",
            "threads",
        ] {
            assert!(value[key].is_null(), "{key}");
        }
        assert_eq!(value["availability"]["rss"], "permission_denied");
        assert_eq!(value["availability"]["cpu_percent"], "warming_up");
    }

    #[test]
    fn commands_round_trip_through_json_escaping() {
        let mut row = Row::default_for_test(9);
        row.cmdline = Some("a\"b\\c\n\t\u{1}".into());
        let encoded = serde_json::to_string(&Process::from(&row)).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&encoded).unwrap();
        assert_eq!(parsed["cmdline"], row.display_command());
        assert!(parsed.get("memory").is_some());
    }
}
