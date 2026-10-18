//! Import an existing `htoprc`.
//!
//! `htoprc` is `key=value` lines with a few pseudo-sections. Supporting it is
//! cheap and is a real adoption lever (`docs/DESIGN.md` §8): a user can point
//! gander at their existing htop config instead of rebuilding it by hand.

use gander_core::row::Field;

use crate::{Config, SortConfig, TabConfig};

/// Parse an `htoprc` document into a `Config`.
///
/// Unknown keys are ignored on purpose: htop's file contains many settings with
/// no gander equivalent yet, and failing on them would be hostile.
pub fn parse(text: &str) -> Config {
    let mut config = Config::default();
    config.tabs.clear();

    let mut columns: Vec<String> = Vec::new();
    let mut sort_key: Option<String> = None;
    let mut sort_direction: i32 = 1;
    let mut delay_tenths: Option<u64> = None;
    let mut header_layout: Option<String> = None;
    let mut tree_view = false;
    let mut hide_kernel_threads = false;

    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if let Some(rest) = line.strip_prefix("fields=") {
            // htoprc: "fields=0 48 17 18 38 39 40 2" — numeric field ids.
            columns = rest
                .split_whitespace()
                .filter_map(htop_field_name)
                .map(str::to_string)
                .collect();
            continue;
        }
        if let Some(rest) = line.strip_prefix("sort_key=") {
            sort_key = htop_field_name(rest.trim()).map(str::to_string);
            continue;
        }
        if let Some(rest) = line.strip_prefix("sort_direction=") {
            sort_direction = rest.trim().parse().unwrap_or(1);
            continue;
        }
        if let Some(rest) = line.strip_prefix("delay=") {
            delay_tenths = rest.trim().parse().ok();
            continue;
        }
        if let Some(rest) = line.strip_prefix("header_layout=") {
            header_layout = Some(rest.trim().to_string());
            continue;
        }
        if let Some(rest) = line.strip_prefix("tree_view=") {
            tree_view = rest.trim() == "1";
            continue;
        }
        if let Some(rest) = line.strip_prefix("hide_kernel_threads=") {
            hide_kernel_threads = rest.trim() == "1";
            continue;
        }
        if let Some(rest) = line.strip_prefix("color_scheme=") {
            config.color_scheme = color_name(rest.trim()).to_string();
        }
    }

    if let Some(tenths) = delay_tenths {
        config.delay_ms = tenths.saturating_mul(100);
    }

    // htoprc stores the *visible* columns in `fields`. htop always appends the
    // Command column when it is not part of the stored list.
    if columns.is_empty() {
        columns = vec!["PERCENT_CPU".into(), "PERCENT_MEM".into(), "COMM".into()];
    }
    if !columns.iter().any(|c| c == "COMM") {
        columns.push("COMM".into());
    }

    let field = sort_key.unwrap_or_else(|| "PERCENT_CPU".to_string());

    config.tabs = vec![TabConfig {
        name: header_layout.unwrap_or_else(|| "Main".into()),
        columns,
        sort: SortConfig {
            field,
            desc: sort_direction < 0,
        },
    }];
    config.active_tab = 0;
    config.tree_mode = tree_view;
    config.show_kernel_threads = !hide_kernel_threads;
    config
}

/// Map an htop numeric field ID to a gander field name.
///
/// htop stores `field - 1` in the file and adds 1 back on load (`Settings.c`,
/// `writeFields`/`readFields`), so every ID here is offset by one.
fn htop_field_name(id: &str) -> Option<&'static str> {
    let raw: u32 = id.trim().parse().ok()?;
    let field = match raw.checked_add(1)? {
        1 => Field::Pid,
        2 => Field::Comm,
        3 => Field::State,
        4 => Field::Ppid,
        5 => Field::Pgrp,
        6 => Field::Session,
        46 => Field::User, // ST_UID
        49 => Field::User,
        18 => Field::Priority,
        19 => Field::Nice,
        47 => Field::PercentCpu,
        48 => Field::PercentMem,
        50 => Field::Time,
        39 => Field::VirtMem,
        40 => Field::ResMem,
        38 => Field::Processor,
        10 => Field::MinFlt,
        12 => Field::MajFlt,
        51 => Field::Nlwp,
        53 => Field::PercentNormCpu,
        54 => Field::Elapsed,
        124 => Field::ProcComm,
        125 => Field::ProcExe,
        126 => Field::Cwd,
        _ => return None,
    };
    Some(field.name())
}

fn color_name(id: &str) -> &'static str {
    match id {
        "0" => "default",
        "1" => "monochrome",
        "2" => "black-on-white",
        "3" => "light-terminal",
        "4" => "midnight",
        "5" => "broken-gray",
        "6" => "dark-midnight",
        _ => "default",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
# Beware! This file is rewritten by htop when settings are changed in the interface.
htop_version=3.3.0
config_reader_min_version=3
fields=0 48 17 18 38 39 40 2
hide_kernel_threads=1
hide_userland_threads=0
shadow_other_users=0
highlight_base_name=1
tree_view=1
sort_key=46
sort_direction=-1
delay=20
color_scheme=4
header_layout=two_50_50
";

    #[test]
    fn ids_match_htop_rowfield() {
        // Sanity check against htop's RowField.h: PERCENT_CPU is 47, USER is 49.
        assert_eq!(htop_field_name("46"), Some("PERCENT_CPU"));
        assert_eq!(htop_field_name("48"), Some("USER"));
        assert_eq!(htop_field_name("0"), Some("PID"));
        assert_eq!(htop_field_name("45"), Some("USER"), "ST_UID maps to USER");
    }

    #[test]
    fn imports_columns_and_sort() {
        // "fields=0 48 17 18 38 39 40 2" stores `id - 1`, so this is
        // PID USER PRIORITY NICE VIRT RES (SHR) STATE -- htop's classic default.
        let c = parse(SAMPLE);
        let tab = c.active_tab();
        let fields = tab.fields();
        assert_eq!(fields.first(), Some(&Field::Pid));
        assert!(fields.contains(&Field::User));
        assert!(fields.contains(&Field::Priority));
        assert!(fields.contains(&Field::Nice));
        assert!(fields.contains(&Field::VirtMem));
        assert!(fields.contains(&Field::ResMem));
        assert_eq!(fields.last(), Some(&Field::Comm), "COMM is appended");
        // sort_key=46 stores 47 == PERCENT_CPU.
        assert_eq!(tab.sort().field, Field::PercentCpu);
        assert_eq!(tab.sort().dir, gander_core::table::Dir::Desc);
    }

    #[test]
    fn one_based_ids_are_respected() {
        // The stored value is `id - 1`; a naive parser would be off by one and
        // turn PID into PERCENT_MEM.
        let c = parse("fields=0\n");
        assert_eq!(c.active_tab().fields().first(), Some(&Field::Pid));
        let c = parse("fields=46\n");
        assert_eq!(c.active_tab().fields().first(), Some(&Field::PercentCpu));
    }

    #[test]
    fn imports_delay_and_toggles() {
        let c = parse(SAMPLE);
        assert_eq!(c.delay_ms, 2000);
        assert!(c.tree_mode);
        assert!(!c.show_kernel_threads, "hide_kernel_threads=1 must invert");
        assert_eq!(c.color_scheme, "midnight");
    }

    #[test]
    fn appends_command_column_when_missing() {
        let c = parse("fields=0 48 17\n");
        let names: Vec<String> = c.active_tab().columns.clone();
        assert!(names.iter().any(|n| n == "COMM"));
    }

    #[test]
    fn empty_input_is_defaults() {
        let c = parse("");
        assert!(!c.active_tab().fields().is_empty());
    }

    #[test]
    fn unknown_field_ids_are_skipped() {
        // 999 is not a known htop field id; the others must survive.
        let c = parse("fields=0 999 48\n");
        let fields = c.active_tab().fields();
        assert!(fields.contains(&Field::Pid));
        assert!(fields.contains(&Field::User));
        assert_eq!(fields.len(), 3, "PID, USER and the appended COMM");
    }
}
