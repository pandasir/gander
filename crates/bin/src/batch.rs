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
        eprintln!("gander: batch mode requires the Linux backend");
        return std::process::ExitCode::from(1);
    }

    let iterations = batch.iterations.unwrap_or(1).max(1);
    let sort_field = batch
        .sort
        .as_deref()
        .and_then(Field::from_name)
        .unwrap_or(Field::PercentCpu);
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

    for iteration in 0..iterations {
        let rows = match sample_once(sort, filter.as_ref()) {
            Ok(rows) => rows,
            Err(e) => {
                eprintln!("gander: {e}");
                return std::process::ExitCode::from(1);
            }
        };

        let result = if batch.json {
            write_json(&mut out, &rows, batch.limit, config)
        } else {
            write_text(&mut out, &rows, batch.limit)
        };

        if let Err(e) = result {
            if e.kind() == std::io::ErrorKind::BrokenPipe {
                return std::process::ExitCode::SUCCESS;
            }
            eprintln!("gander: {e}");
            return std::process::ExitCode::from(1);
        }

        if iteration + 1 < iterations {
            std::thread::sleep(config.delay());
        }
    }

    std::process::ExitCode::SUCCESS
}

#[cfg(target_os = "linux")]
fn sample_once(sort: Sort, filter: Option<&Filter>) -> std::io::Result<Vec<Row>> {
    let mut sampler = gander_core::linux::system::Sampler::default();
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

/// Placeholder for non-Linux builds; batch mode is rejected earlier.
#[cfg(not(target_os = "linux"))]
fn sample_once(_sort: Sort, _filter: Option<&Filter>) -> std::io::Result<Vec<Row>> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "no backend for this platform",
    ))
}

fn write_text(
    out: &mut impl std::io::Write,
    rows: &[Row],
    limit: Option<usize>,
) -> std::io::Result<()> {
    let take = limit.unwrap_or(rows.len()).min(rows.len());
    writeln!(
        out,
        "{:>7} {:>6} {:>5} {:>5} {:>8} {:>8} : COMMAND",
        "PID", "CPU%", "MEM%", "NI", "RES", "TIME"
    )?;
    for row in rows.iter().take(take) {
        writeln!(
            out,
            "{:>7} {:>6} {:>5} {:>5} {:>8} {:>8} : {}",
            row.pid,
            format::percent_human(row.percent_cpu),
            format::percent_human(row.percent_mem),
            row.nice,
            format::bytes_human(row.res),
            format::duration_human(row.elapsed),
            row.display_command()
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
        writeln!(
            out,
            "  {{\"pid\":{},\"ppid\":{},\"uid\":{},\"comm\":{},\"state\":\"{}\",\
             \"cpu_percent\":{:.2},\"mem_percent\":{:.2},\"rss\":{},\"vsize\":{},\
             \"nice\":{},\"priority\":{},\"threads\":{},\"elapsed\":{},\"cmdline\":{}}}{}",
            row.pid,
            row.ppid,
            row.uid,
            json_string(row.comm.as_str()),
            row.state as char,
            row.percent_cpu,
            row.percent_mem,
            row.res,
            row.virt,
            row.nice,
            row.priority,
            row.nlwp,
            row.elapsed,
            json_string(row.display_command()),
            comma
        )?;
    }
    writeln!(out, "]")?;
    Ok(())
}

/// Minimal JSON string escaping: quotes, backslashes and control characters.
fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                use std::fmt::Write as _;
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
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

    #[test]
    fn json_escaping() {
        assert_eq!(json_string("plain"), "\"plain\"");
        assert_eq!(json_string("a\"b"), "\"a\\\"b\"");
        assert_eq!(json_string("a\\b"), "\"a\\\\b\"");
        assert_eq!(json_string("a\nb"), "\"a\\nb\"");
        assert_eq!(json_string("a\u{1}b"), "\"a\\u0001b\"");
    }

    #[test]
    fn text_output_respects_limit() {
        let rows = vec![row(1, "a", 1.0), row(2, "b", 2.0), row(3, "c", 3.0)];
        let mut buf = Vec::new();
        write_text(&mut buf, &rows, Some(2)).unwrap();
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
        assert_eq!(text.matches("},").count(), 1, "comma only between entries");
    }

    #[test]
    fn empty_output_is_empty_array() {
        let mut buf = Vec::new();
        write_json(&mut buf, &[], None, &Config::default()).unwrap();
        assert_eq!(String::from_utf8(buf).unwrap().trim(), "[\n]");
    }
}
