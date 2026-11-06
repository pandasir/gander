//! Batch mode: print samples as text or JSON, then exit.
//!
//! This is the differentiator over htop: because `gander-core` has no UI
//! dependency, the same data the TUI renders can be emitted for monitoring.
//! `docs/DESIGN.md` §"Deliberate deviations from htop".

use gander_config::Config;
use gander_core::row::{Field, Row};
use gander_core::table::{Dir, Filter, Sort};
use gander_core::{format, PLATFORM_SUPPORTED};

use crate::args::Batch;

pub fn run(config: &Config, batch: &Batch) -> std::process::ExitCode {
    if !PLATFORM_SUPPORTED {
        eprintln!("gander: batch mode requires a live backend (Linux, macOS or FreeBSD)");
        return std::process::ExitCode::from(1);
    }

    let iterations = batch.iterations.unwrap_or(1).max(1);

    // An unknown `--sort` field is an error, not a fallback.
    //
    // This used to be `.and_then(Field::from_name).unwrap_or(Field::PercentCpu)`,
    // and the fallback never actually happened: `Sort`'s field is consumed by
    // `compare_by`, whose `_` arm compares pids, so a misspelt field produced a
    // pid-sorted list that looked deliberate. `--sort BOGUS` and `--sort M_PSS` on
    // macOS gave byte-identical output, which meant a user could not tell a typo
    // from a column their platform does not have. Saying so is the only useful
    // answer: the caller asked for an order and did not get it.
    let sort_field = match batch.sort.as_deref() {
        None => Field::PercentCpu,
        Some(name) => match Field::from_name(name) {
            Some(field) => field,
            None => {
                // A Linux-only column named on another platform is worth
                // distinguishing from a typo: the name is real, the platform is
                // the problem.
                let known_elsewhere = is_linux_only_field(name);
                if known_elsewhere {
                    eprintln!(
                        "gander: --sort {name} is a Linux-only column and this build has no \
                         data source for it"
                    );
                } else {
                    eprintln!("gander: unknown --sort field: {name}");
                    eprintln!("gander: run `gander --help` for the list");
                }
                return std::process::ExitCode::from(2);
            }
        },
    };
    let sort = Sort {
        field: sort_field,
        dir: if sort_field.default_sort_desc() {
            Dir::Desc
        } else {
            Dir::Asc
        },
    };
    let filter = batch.filter.as_deref().and_then(Filter::parse);

    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    // One sampler for the whole run: per-process CPU% needs two consecutive
    // samples, so a fresh sampler per iteration would always report 0.
    let mut sampler = gander_core::Sampler::default();
    // Set before the first sample: the sampler only enumerates threads, reads
    // `smaps_rollup` or walks `fdinfo` when it is told to, and an option applied
    // after `sample()` would cost one wasted pass.
    //
    // Batch mode has no columns to consult, so the expensive reads are driven by
    // what the output actually contains: `--threads` selects thread rows, and the
    // sort field selects the rest. Asking for `--sort M_PSS` is exactly a request
    // to pay for `smaps_rollup`.
    gander_core::PlatformSampler::set_scan_options(
        &mut sampler,
        gander_core::ScanOptions::none()
            .with_threads(batch.threads)
            .with_smaps(sort.field.needs_smaps_rollup())
            .with_gpu(sort.field.needs_gpu_fdinfo())
            .with_delayacct(sort.field.needs_delayacct()),
    );

    let result = measured_samples(
        iterations,
        config.delay(),
        || sample_once(&mut sampler, sort, filter.as_ref()),
        std::thread::sleep,
        |rows| {
            if batch.json {
                write_json(&mut out, &rows, batch.limit, config)?;
            } else {
                write_text(&mut out, &rows, batch.limit, sample_clk_tck())?;
            }
            std::io::Write::flush(&mut out)
        },
    );
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => {
            std::process::ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("gander: {error}");
            std::process::ExitCode::from(1)
        }
    }
}

fn measured_samples<T>(
    iterations: usize,
    delay: std::time::Duration,
    mut sample: impl FnMut() -> std::io::Result<T>,
    mut wait: impl FnMut(std::time::Duration),
    mut emit: impl FnMut(T) -> std::io::Result<()>,
) -> std::io::Result<()> {
    let _baseline = sample()?;
    for _ in 0..iterations.max(1) {
        wait(delay);
        emit(sample()?)?;
    }
    Ok(())
}

/// Whether `name` is a column that exists on some other platform.
///
/// `Field::from_name` only sees this build's `FIELD_META`, so a Linux-only column
/// asked for on macOS comes back as an unknown name. The two cases deserve
/// different messages: "you spelt it wrong" and "this build cannot answer that"
/// are different problems with different fixes.
///
/// The list is written out rather than derived because the fields it names do not
/// exist in this build's `Field` enum — there is nothing to iterate over.
#[cfg(not(target_os = "linux"))]
fn is_linux_only_field(name: &str) -> bool {
    matches!(
        name,
        "M_PSS"
            | "M_EPSS"
            | "M_PRIVATE"
            | "M_SWAP"
            | "GPU%"
            | "GPU_TIME"
            | "CPUD%"
            | "IOD%"
            | "SWPD%"
            | "CGROUP"
            | "CONTAINER"
    )
}

/// On Linux every column in the list exists, so this is always false.
///
/// Kept as a function rather than a `cfg` at each call site so the error path
/// reads the same on both platforms.
#[cfg(target_os = "linux")]
fn is_linux_only_field(_name: &str) -> bool {
    false
}

/// Clock ticks per second, for the TIME column.
#[cfg(any(target_os = "linux", target_os = "macos", target_os = "freebsd"))]
fn sample_clk_tck() -> u64 {
    gander_core::CurrentPlatform::clk_tck()
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "freebsd"))]
fn sample_once(
    sampler: &mut gander_core::Sampler,
    sort: Sort,
    filter: Option<&Filter>,
) -> std::io::Result<Vec<Row>> {
    use gander_core::PlatformSampler as _;
    let (_snapshot, rows) = sampler.sample();
    let mut order: Vec<usize> = if let Some(f) = filter {
        (0..rows.len()).filter(|&i| f.matches(&rows[i])).collect()
    } else {
        (0..rows.len()).collect()
    };
    gander_core::table::sort_order(&rows, &mut order, sort);

    // Move rows out in sorted order without reordering the source vector.
    let mut slots: Vec<Option<Row>> = rows.into_iter().map(Some).collect();
    Ok(order.into_iter().filter_map(|i| slots[i].take()).collect())
}

/// Placeholder for targets without a backend; batch mode is rejected earlier.
#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "freebsd")))]
fn sample_once(
    _sampler: &mut gander_core::Sampler,
    _sort: Sort,
    _filter: Option<&Filter>,
) -> std::io::Result<Vec<Row>> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "no backend for this platform",
    ))
}

fn write_text(
    out: &mut impl std::io::Write,
    rows: &[Row],
    limit: Option<usize>,
    clk_tck: u64,
) -> std::io::Result<()> {
    let take = limit.unwrap_or(rows.len()).min(rows.len());
    writeln!(
        out,
        "{:>7} {:>6} {:>5} {:>5} {:>8} {:>8} : COMMAND",
        "PID", "CPU%", "MEM%", "NI", "RES", "TIME"
    )?;
    for row in rows.iter().take(take) {
        // A thread row is marked so the output stays greppable; on macOS a thread
        // shares its leader's pid, so without a marker the two lines are
        // indistinguishable.
        let command = if row.is_thread {
            format!("{{{}}} {}", row.comm.as_str(), row.display_command())
        } else {
            row.display_command().to_string()
        };
        writeln!(
            out,
            "{:>7} {:>6} {:>5} {:>5} {:>8} {:>8} : {}",
            row.pid,
            if row.availability.field(Field::PercentCpu).is_available() {
                format::percent_human(row.percent_cpu).to_string()
            } else {
                "-".into()
            },
            if row.availability.field(Field::PercentMem).is_available() {
                format::percent_human(row.percent_mem).to_string()
            } else {
                "-".into()
            },
            row.nice,
            if row.availability.field(Field::ResMem).is_available() {
                format::bytes_human(row.res).to_string()
            } else {
                "-".into()
            },
            if row.availability.field(Field::Time).is_available() {
                format::cpu_time_human(row.time_ticks, clk_tck).to_string()
            } else {
                "-".into()
            },
            command
        )?;
    }
    Ok(())
}

fn write_json(
    out: &mut impl std::io::Write,
    rows: &[Row],
    limit: Option<usize>,
    _config: &Config,
) -> std::io::Result<()> {
    let take = limit.unwrap_or(rows.len()).min(rows.len());
    writeln!(out, "[")?;
    for (i, row) in rows.iter().take(take).enumerate() {
        let comma = if i + 1 == take { "" } else { "," };
        write!(out, "  ")?;
        serde_json::to_writer(&mut *out, &crate::export::Process::from(row)).map_err(|error| {
            std::io::Error::new(
                error
                    .io_error_kind()
                    .unwrap_or(std::io::ErrorKind::InvalidData),
                error,
            )
        })?;
        writeln!(out, "{comma}")?;
    }
    writeln!(out, "]")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use gander_core::row::Row;

    fn row(pid: u32, comm: &str, cpu: f32) -> Row {
        let mut r = Row::default_for_test(pid);
        r.comm.try_push_str(comm).unwrap();
        r.percent_cpu = cpu;
        r
    }

    /// A Linux-only column asked for on another platform must be named as such,
    /// not reported as a typo.
    #[cfg(not(target_os = "linux"))]
    #[test]
    fn linux_only_fields_are_recognised_as_such() {
        for name in [
            "M_PSS",
            "M_EPSS",
            "M_PRIVATE",
            "M_SWAP",
            "GPU%",
            "GPU_TIME",
            "CPUD%",
            "IOD%",
            "SWPD%",
        ] {
            assert!(is_linux_only_field(name), "{name} should be Linux-only");
            assert_eq!(
                Field::from_name(name),
                None,
                "{name} must not resolve on this platform"
            );
        }
        assert!(!is_linux_only_field("BOGUS"));
        assert!(!is_linux_only_field("PERCENT_MEM"));
    }

    /// On Linux every name in that list is a real column, so the "Linux-only"
    /// branch is unreachable there — which is the point: the same error path
    /// reads the same way on both platforms.
    #[cfg(target_os = "linux")]
    #[test]
    fn no_field_is_linux_only_on_linux() {
        for name in [
            "M_PSS",
            "M_EPSS",
            "M_PRIVATE",
            "M_SWAP",
            "GPU%",
            "GPU_TIME",
            "CPUD%",
            "IOD%",
            "SWPD%",
        ] {
            assert!(!is_linux_only_field(name));
            assert!(
                Field::from_name(name).is_some(),
                "{name} must resolve on Linux"
            );
        }
    }

    #[test]
    fn json_escaping() {
        for input in ["plain", "a\"b", "a\\b", "a\nb", "a\u{1}b"] {
            let encoded = serde_json::to_string(input).unwrap();
            assert_eq!(serde_json::from_str::<String>(&encoded).unwrap(), input);
        }
    }

    #[test]
    fn text_output_respects_limit() {
        let rows = vec![row(1, "a", 1.0), row(2, "b", 2.0), row(3, "c", 3.0)];
        let mut buf = Vec::new();
        write_text(&mut buf, &rows, Some(2), 100).unwrap();
        let text = String::from_utf8(buf).unwrap();
        assert_eq!(text.lines().count(), 3, "header plus two rows");
    }

    #[test]
    fn json_output_is_well_formed() {
        let rows = vec![row(1, "init", 0.5), row(2, "bash", 1.5)];
        let mut buf = Vec::new();
        write_json(&mut buf, &rows, None, &Config::default()).unwrap();
        let text = String::from_utf8(buf).unwrap();
        assert!(text.starts_with('['));
        assert!(text.trim_end().ends_with(']'));
        assert_eq!(text.matches("{\"pid\"").count(), 2);
        let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed.as_array().unwrap().len(), 2);
    }

    #[test]
    fn warmup_does_not_count_as_output_and_every_output_follows_a_wait() {
        use std::cell::{Cell, RefCell};
        for iterations in [0, 1, 2] {
            let scanned = Cell::new(0usize);
            let emitted = RefCell::new(Vec::new());
            let events = RefCell::new(Vec::new());
            measured_samples(
                iterations,
                Config::default().delay(),
                || {
                    scanned.set(scanned.get() + 1);
                    events.borrow_mut().push("scan");
                    Ok(scanned.get())
                },
                |_| events.borrow_mut().push("wait"),
                |value| {
                    events.borrow_mut().push("emit");
                    emitted.borrow_mut().push(value);
                    Ok(())
                },
            )
            .unwrap();
            assert_eq!(scanned.get(), iterations.max(1) + 1);
            assert_eq!(emitted.borrow().len(), iterations.max(1));
            assert_eq!(emitted.borrow()[0], 2);
            assert_eq!(&events.borrow()[..4], ["scan", "wait", "scan", "emit"]);
        }
    }

    #[test]
    fn broken_pipe_stops_sampling_without_an_extra_output() {
        let mut scanned = 0;
        let error = measured_samples(
            5,
            std::time::Duration::ZERO,
            || {
                scanned += 1;
                Ok(scanned)
            },
            |_| {},
            |_| Err(std::io::ErrorKind::BrokenPipe.into()),
        )
        .unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe);
        assert_eq!(scanned, 2);
    }

    #[test]
    fn empty_output_is_empty_array() {
        let mut buf = Vec::new();
        write_json(&mut buf, &[], None, &Config::default()).unwrap();
        assert_eq!(String::from_utf8(buf).unwrap().trim(), "[\n]");
    }
}
