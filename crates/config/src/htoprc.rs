//! Import an existing `htoprc`.
//!
//! `htoprc` is `key=value` lines with a few pseudo-sections. Supporting it is
//! cheap and is a real adoption lever (`docs/DESIGN.md` §8): a user can point
//! gander at their existing htop config instead of rebuilding it by hand.

use gander_core::row::Field;

use crate::{Config, MeterConfig, SortConfig, TabConfig};

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
    let mut hide_userland_threads = false;
    let mut hide_running_in_container = false;
    // `column_meters_N` and `column_meter_modes_N` are parallel lists, one entry
    // per meter, per header column.
    let mut meters: [Vec<String>; 2] = [Vec::new(), Vec::new()];
    let mut modes: [Vec<String>; 2] = [Vec::new(), Vec::new()];
    // Per-tab layouts live in `screen:NAME=...` sections followed by dotted keys.
    let mut tabs: Vec<TabConfig> = Vec::new();

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
        if let Some(rest) = line.strip_prefix("hide_userland_threads=") {
            hide_userland_threads = rest.trim() == "1";
            continue;
        }
        if let Some(rest) = line.strip_prefix("hide_running_in_container=") {
            hide_running_in_container = rest.trim() == "1";
            continue;
        }
        if let Some(rest) = line.strip_prefix("color_scheme=") {
            config.color_scheme = color_name(rest.trim()).to_string();
            continue;
        }
        for (column, key) in [(0usize, "column_meters_0="), (1, "column_meters_1=")] {
            if let Some(rest) = line.strip_prefix(key) {
                meters[column] = rest.split_whitespace().map(str::to_string).collect();
            }
            let key = if column == 0 {
                "column_meter_modes_0="
            } else {
                "column_meter_modes_1="
            };
            if let Some(rest) = line.strip_prefix(key) {
                modes[column] = rest.split_whitespace().map(str::to_string).collect();
            }
        }
        // `screen:Main=PID USER ...` opens a tab; the `.sort_key=` lines after it
        // belong to that tab until the next `screen:` header.
        if let Some(rest) = line.strip_prefix("screen:") {
            if let Some((name, fields)) = rest.split_once('=') {
                let cols = fields
                    .split_whitespace()
                    .filter_map(htop_field_name)
                    .map(str::to_string)
                    .collect();
                tabs.push(TabConfig {
                    name: name.trim().to_string(),
                    columns: cols,
                    sort: SortConfig {
                        field: "PERCENT_CPU".into(),
                        desc: true,
                    },
                });
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix(".sort_key=") {
            if let Some(tab) = tabs.last_mut() {
                // The value is either an htop numeric id or one of htop's
                // human-readable names (`PERCENT_MEM`, `RES`, `Command`, ...).
                let value = rest.trim();
                if let Some(field) = htop_field_name(value) {
                    tab.sort.field = field.to_string();
                } else if let Some(field) = sort_key_name(value) {
                    tab.sort.field = field.name().to_string();
                }
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix(".sort_direction=") {
            if let Some(tab) = tabs.last_mut() {
                tab.sort.desc = rest.trim().parse::<i32>().unwrap_or(1) < 0;
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix(".tree_view=") {
            let _ = rest;
            continue;
        }
        if line.starts_with('.') {
            // Any other per-tab key is one we do not model yet.
            continue;
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

    // `screen:` sections take precedence over the flat `fields=` list: they are
    // what htop actually renders when screen tabs are enabled.
    if tabs.is_empty() {
        config.tabs = vec![TabConfig {
            name: header_layout.unwrap_or_else(|| "Main".into()),
            columns,
            sort: SortConfig {
                field,
                desc: sort_direction < 0,
            },
        }];
    } else {
        for tab in &mut tabs {
            if !tab.columns.iter().any(|c| c == "COMM") {
                tab.columns.push("COMM".into());
            }
        }
        config.tabs = tabs;
    }
    config.active_tab = 0;
    config.tree_mode = tree_view;
    config.show_kernel_threads = !hide_kernel_threads;
    config.show_userland_threads = !hide_userland_threads;
    // htop stores the hide_ form; the config stores the show_ form.
    config.show_containerised = !hide_running_in_container;

    // Header meters, when the file has them. The mode lists are parallel to the
    // name lists and may be shorter; a missing mode stays whatever the default
    // was for that meter.
    let convert = |names: &[String], mods: &[String]| -> Vec<MeterConfig> {
        names
            .iter()
            .enumerate()
            .filter(|(_, n)| meter_name_known(n))
            .map(|(i, n)| {
                let mode = mods.get(i).map(String::as_str).unwrap_or("1");
                MeterConfig::new(n, mode)
            })
            .collect()
    };
    let left = convert(&meters[0], &modes[0]);
    let right = convert(&meters[1], &modes[1]);
    if !left.is_empty() || !right.is_empty() {
        let defaults = Config::default();
        config.meters_left = if left.is_empty() {
            defaults.meters_left
        } else {
            left
        };
        config.meters_right = if right.is_empty() {
            defaults.meters_right
        } else {
            right
        };
    }

    config
}

/// Resolve a `sort_key=` value written as a name rather than a numeric id.
///
/// htop's own config uses the display names from `RowField` (`PERCENT_MEM`,
/// `RES`, `TIME`, `Command`), which do not always match gander's field names.
fn sort_key_name(name: &str) -> Option<Field> {
    Some(match name {
        "PID" => Field::Pid,
        "Command" | "COMM" => Field::Comm,
        "STATE" => Field::State,
        "PPID" => Field::Ppid,
        "PGRP" => Field::Pgrp,
        "SESSION" => Field::Session,
        "USER" => Field::User,
        "PRIORITY" => Field::Priority,
        "NICE" => Field::Nice,
        "PERCENT_CPU" => Field::PercentCpu,
        "PERCENT_MEM" => Field::PercentMem,
        "TIME" => Field::Time,
        "M_VIRT" | "VIRT" => Field::VirtMem,
        "M_RESIDENT" | "RES" => Field::ResMem,
        "PROCESSOR" => Field::Processor,
        "MINFLT" => Field::MinFlt,
        "MAJFLT" => Field::MajFlt,
        "NLWP" => Field::Nlwp,
        "PERCENT_NORM_CPU" => Field::PercentNormCpu,
        "ELAPSED" => Field::Elapsed,
        "PROC_COMM" => Field::ProcComm,
        "PROC_EXE" => Field::ProcExe,
        "CWD" => Field::Cwd,
        _ => return Field::from_name(name),
    })
}

/// Meter names this build understands.
///
/// Duplicated from `gander-tui` on purpose: `gander-config` must not depend on
/// the renderer, and a name that is not in this list would silently produce an
/// empty meter that the user cannot see or fix.
fn meter_name_known(name: &str) -> bool {
    matches!(
        name,
        "LeftCPUs"
            | "RightCPUs"
            | "CPU"
            | "AllCPUs"
            | "Memory"
            | "Swap"
            | "Tasks"
            | "LoadAverage"
            | "Load"
            | "Uptime"
            | "Clock"
            | "Date"
            | "Hostname"
            | "Blank"
    )
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
    fn imports_header_meters_and_per_meter_modes() {
        // The real shape from htop 3.3: names in one list, numeric modes in a
        // parallel list (1 = bar, 2 = text, 3 = graph).
        let text = "\
column_meters_0=LeftCPUs Memory Swap
column_meter_modes_0=1 2 1
column_meters_1=RightCPUs Tasks LoadAverage Uptime
column_meter_modes_1=1 2 2 2
";
        let c = parse(text);
        let left: Vec<(&str, &str)> = c
            .meters_left
            .iter()
            .map(|m| (m.name.as_str(), m.mode.as_str()))
            .collect();
        assert_eq!(
            left,
            vec![("LeftCPUs", "1"), ("Memory", "2"), ("Swap", "1")]
        );
        assert_eq!(c.meters_right.len(), 4);
        assert_eq!(c.meters_right[1].name, "Tasks");
        assert_eq!(c.meters_right[1].mode_name(), "text");
        assert_eq!(c.meters_left[1].mode_name(), "text");
        assert_eq!(c.meters_left[0].mode_name(), "bar");
    }

    #[test]
    fn unknown_meter_names_are_dropped() {
        let text = "column_meters_0=LeftCPUs NoSuchMeter Memory\ncolumn_meter_modes_0=1 1 2\n";
        let c = parse(text);
        let names: Vec<&str> = c.meters_left.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, vec!["LeftCPUs", "Memory"]);
        // The modes must follow the *surviving* names, not their original slots.
        assert_eq!(c.meters_left[0].mode_name(), "bar");
        assert_eq!(c.meters_left[1].mode_name(), "text");
    }

    #[test]
    fn named_screen_sections_become_tabs() {
        // This is what htop writes when screen tabs are enabled.
        let text = "\
screen:Main=PID USER PERCENT_CPU Command
.sort_key=PERCENT_MEM
.sort_direction=-1
screen:Memory=PID PERCENT_MEM RES Command
.sort_key=RES
.sort_direction=-1
";
        let c = parse(text);
        assert_eq!(c.tabs.len(), 2, "one tab per screen: section");
        assert_eq!(c.tabs[0].name, "Main");
        assert_eq!(c.tabs[1].name, "Memory");
        // `.sort_key` belongs to the tab above it.
        assert_eq!(c.tabs[0].sort().field, Field::PercentMem);
        assert!(c.tabs[0].sort.desc);
        // `RES` is htop's name for the resident-size column.
        assert_eq!(c.tabs[1].sort().field, Field::ResMem);
        // Command is appended when the section omits it.
        for tab in &c.tabs {
            assert!(tab.columns.iter().any(|n| n == "COMM"), "{}", tab.name);
        }
    }

    #[test]
    fn a_flat_file_still_yields_one_tab() {
        // No `screen:` sections: the old single-tab path must keep working.
        let c = parse(SAMPLE);
        assert_eq!(c.tabs.len(), 1);
        assert_eq!(c.active_tab, 0);
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
