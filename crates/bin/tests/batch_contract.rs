#![cfg(unix)]

use std::process::{Child, Command};

struct TestProcess(Child);
impl Drop for TestProcess {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn os_cpu_seconds(pid: u32) -> Option<f64> {
    #[cfg(target_os = "linux")]
    {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        let fields = stat[stat.rfind(')')? + 1..]
            .split_whitespace()
            .collect::<Vec<_>>();
        let ticks = fields[11].parse::<u64>().ok()? + fields[12].parse::<u64>().ok()?;
        Some(ticks as f64 / gander_core::CurrentPlatform::clk_tck() as f64)
    }
    #[cfg(target_os = "macos")]
    {
        let task = gander_core::darwin::scan::task_info_checked(pid).ok()?;
        Some(
            gander_core::darwin::units::mach_ticks_to_nanos(task.user_ns + task.system_ns) as f64
                / 1e9,
        )
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = pid;
        None
    }
}

#[test]
fn first_json_output_measures_a_preexisting_busy_process() {
    let token = format!("gander-batch-probe-{}", std::process::id());
    let process = TestProcess(
        Command::new("sh")
            .args(["-c", "while :; do :; done", &token])
            .spawn()
            .unwrap(),
    );
    let idle = TestProcess(
        Command::new("sh")
            .args(["-c", "read unused", &token])
            .stdin(std::process::Stdio::piped())
            .spawn()
            .unwrap(),
    );
    std::thread::sleep(std::time::Duration::from_millis(80));
    let before = os_cpu_seconds(process.0.id());
    let start = std::time::Instant::now();
    let output = Command::new(env!("CARGO_BIN_EXE_gander"))
        .args(["--json", "-d", "500", "--filter", &token])
        .env(
            "GANDER_CONFIG",
            std::env::temp_dir().join(format!("{token}.missing.toml")),
        )
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let rows: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        rows[0]["pid"],
        process.0.id(),
        "CPU ordering puts the busy task before idle rows"
    );
    let row = rows
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["pid"] == process.0.id())
        .unwrap();
    assert!(row["cpu_percent"].as_f64().unwrap() > 1.0, "{row}");
    assert_eq!(row["availability"]["cpu_percent"], "available");
    if let (Some(before), Some(after)) = (before, os_cpu_seconds(process.0.id())) {
        let expected = (after - before) / start.elapsed().as_secs_f64() * 100.0;
        let actual = row["cpu_percent"].as_f64().unwrap();
        println!(
            "busy CPU: JSON {actual:.2}%, OS interval {expected:.2}%; idle CPU: measured zero"
        );
        assert!(
            (actual - expected).abs() < (expected * 0.25).max(15.0),
            "exported {actual}, OS interval {expected}"
        );
    }
    let idle_row = rows
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["pid"] == idle.0.id())
        .unwrap();
    assert_eq!(idle_row["availability"]["cpu_percent"], "available");
    assert_eq!(idle_row["cpu_percent"].as_f64(), Some(0.0));
}

#[test]
fn newly_observed_process_starts_unavailable_then_becomes_measured() {
    use std::process::Stdio;
    let token = format!("gander-new-process-probe-{}", std::process::id());
    let mut monitor = TestProcess(
        Command::new(env!("CARGO_BIN_EXE_gander"))
            .args(["--json", "-n", "3", "-d", "300", "--filter", &token])
            .env(
                "GANDER_CONFIG",
                std::env::temp_dir().join(format!("{token}.missing.toml")),
            )
            .stdout(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let output = monitor.0.stdout.take().unwrap();
    let mut snapshots =
        serde_json::Deserializer::from_reader(output).into_iter::<Vec<serde_json::Value>>();
    snapshots.next().unwrap().unwrap();
    let worker = TestProcess(
        Command::new("sh")
            .args(["-c", "while :; do :; done", &token])
            .spawn()
            .unwrap(),
    );
    let first = snapshots.next().unwrap().unwrap();
    let row = first
        .iter()
        .find(|row| row["pid"] == worker.0.id())
        .unwrap();
    assert!(row["cpu_percent"].is_null());
    assert_eq!(row["availability"]["cpu_percent"], "warming_up");
    let second = snapshots.next().unwrap().unwrap();
    let row = second
        .iter()
        .find(|row| row["pid"] == worker.0.id())
        .unwrap();
    assert_eq!(row["availability"]["cpu_percent"], "available");
    assert!(row["cpu_percent"].as_f64().unwrap() > 1.0);
    assert!(monitor.0.wait().unwrap().success());
}

#[test]
fn repeated_json_outputs_keep_the_existing_array_stream() {
    let output = Command::new(env!("CARGO_BIN_EXE_gander"))
        .args(["--json", "-n", "2", "-d", "100", "--limit", "1"])
        .env(
            "GANDER_CONFIG",
            std::env::temp_dir().join(format!(
                "gander-json-stream-{}.missing.toml",
                std::process::id()
            )),
        )
        .output()
        .unwrap();
    assert!(output.status.success());
    let snapshots = serde_json::Deserializer::from_slice(&output.stdout)
        .into_iter::<serde_json::Value>()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(snapshots.len(), 2);
    assert!(snapshots
        .iter()
        .all(|sample| sample.as_array().unwrap().len() <= 1));
}
