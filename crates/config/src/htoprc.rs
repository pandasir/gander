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
            | "LeftCPUs2"
            | "RightCPUs2"
            | "LeftCPUs4"
            | "RightCPUs4"
            | "LeftCPUs8"
            | "RightCPUs8"
            | "AllCPUs"
            | "AllCPUs2"
            | "AllCPUs4"
            | "AllCPUs8"
            | "CPU"
            | "Memory"
            | "Swap"
            | "Tasks"
            | "LoadAverage"
            | "Load"
            | "Uptime"
            | "Clock"
            | "Date"
            | "Hostname"
            | "NetworkIO"
            | "Network"
            | "DiskIO"
            | "Disk"
            // M3: PSI pressure, compressed RAM, huge pages. Kept in this list so
            // an htoprc that names them imports instead of being silently
            // dropped, and so a config written on a Linux box still loads on a
            // Mac (where each meter reports `n/a here`).
            | "PSI"
            | "Pressure"
            | "Zram"
            | "Zswap"
            | "HugePages"
            | "Hugepages"
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
        // htop's PSS block and delay columns, from `linux/ProcessField.h` on
        // htop `main`:
        //
        //   PERCENT_CPU_DELAY = 116, PERCENT_IO_DELAY = 117,
        //   PERCENT_SWAP_DELAY = 118,
        //   M_PSS = 119, M_SWAP = 120, M_PSSWP = 121,
        //   M_PRIV = 131, GPU_TIME = 132, GPU_PERCENT = 133, M_EPSS = 135
        //
        // The `match` below is on `raw + 1` (htop's own id), so the arms use the
        // values above verbatim; the file stores each one minus one.
        //
        // The name mapping is by *meaning*, not by spelling: htop has both
        // `M_EPSS` (135) and `M_PSSWP` (121), and they are the same figure —
        // effective PSS, i.e. PSS plus the swapped-out equivalent. gander keeps
        // one name for it, `M_EPSS`, and both htop ids import to it. `M_PRIV` is
        // our `M_PRIVATE`.
        //
        // Gated on Linux because the fields are: importing an htoprc on macOS must
        // drop them rather than name a `Field` this build does not have.
        #[cfg(target_os = "linux")]
        116 => Field::CpuDelayPercent,
        #[cfg(target_os = "linux")]
        117 => Field::IoDelayPercent,
        #[cfg(target_os = "linux")]
        118 => Field::SwapDelayPercent,
        #[cfg(target_os = "linux")]
        119 => Field::Pss,
        #[cfg(target_os = "linux")]
        120 => Field::Swap,
        #[cfg(target_os = "linux")]
        121 => Field::Epss,
        #[cfg(target_os = "linux")]
        131 => Field::Private,
        #[cfg(target_os = "linux")]
        132 => Field::GpuTime,
        #[cfg(target_os = "linux")]
        133 => Field::GpuPercent,
        #[cfg(target_os = "linux")]
        135 => Field::Epss,
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

    /// htop's PSS block imports to our PSS block, and the mapping is by meaning
    /// rather than by name: htop calls `PSS + SwapPss` "M_PSSWP", we call it
    /// `M_EPSS`.
    #[cfg(target_os = "linux")]
    #[test]
    fn imports_the_pss_columns() {
        // The file stores `id - 1`: M_PSS is 119 in `ProcessField.h` and 118 in
        // the file.
        assert_eq!(htop_field_name("118"), Some("M_PSS"));
        assert_eq!(htop_field_name("119"), Some("M_SWAP"));
        assert_eq!(htop_field_name("120"), Some("M_EPSS"), "htop's M_PSSWP");
        assert_eq!(htop_field_name("134"), Some("M_EPSS"), "htop's M_EPSS");
        assert_eq!(htop_field_name("130"), Some("M_PRIVATE"), "htop's M_PRIV");
        assert_eq!(htop_field_name("131"), Some("GPU_TIME"));
        assert_eq!(htop_field_name("132"), Some("GPU%"));
        // Delay accounting: htop ids 116/117/118, so the file stores 115/116/117.
        assert_eq!(htop_field_name("115"), Some("CPUD%"));
        assert_eq!(htop_field_name("116"), Some("IOD%"));
        assert_eq!(htop_field_name("117"), Some("SWPD%"));

        let c = parse("fields=0 118 119 120 130\n");
        let fields = c.active_tab().fields();
        assert!(fields.contains(&Field::Pss), "{fields:?}");
        assert!(fields.contains(&Field::Swap), "{fields:?}");
        assert!(fields.contains(&Field::Epss), "{fields:?}");
        assert!(fields.contains(&Field::Private), "{fields:?}");
    }

    /// Delay accounting is a separate htop subsystem (`HAVE_DELAYACCT`) but the
    /// columns import like any other.
    #[cfg(target_os = "linux")]
    #[test]
    fn imports_the_delay_columns() {
        let c = parse("fields=0 115 116 117\n");
        let fields = c.active_tab().fields();
        assert!(fields.contains(&Field::CpuDelayPercent), "{fields:?}");
        assert!(fields.contains(&Field::IoDelayPercent), "{fields:?}");
        assert!(fields.contains(&Field::SwapDelayPercent), "{fields:?}");
    }

    /// An htoprc written on macOS must not name a `Field` the macOS build does
    /// not have. The importer drops them rather than failing the whole file.
    #[cfg(not(target_os = "linux"))]
    #[test]
    fn pss_ids_are_dropped_where_the_fields_do_not_exist() {
        assert_eq!(htop_field_name("91"), None);
        // 48 stores htop field id 49. The point of the test is that the Linux-only
        // ids are dropped while a common one survives.
        let c = parse("fields=0 91 48\n");
        let fields = c.active_tab().fields();
        assert!(fields.contains(&Field::Pid));
        assert!(fields.contains(&Field::User), "{fields:?}");
    }

    /// The M3 meters must survive an import, or a header edited on a Linux box is
    /// silently reset the next time the file is read.
    #[test]
    fn imports_the_m3_meters() {
        let c = parse(
            "column_meters_0=PSI Zram Zswap HugePages\n\
             column_meter_modes_0=1 1 1 2\n",
        );
        let names: Vec<&str> = c.meters_left.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, vec!["PSI", "Zram", "Zswap", "HugePages"]);
        assert_eq!(c.meters_left[3].mode_name(), "text");
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

    /// htop writes the suffixed names once it has picked a grid shape, so an
    /// htoprc from a large machine must import as that shape rather than as one
    /// core per line.
    #[test]
    fn suffixed_cpu_meters_are_kept() {
        let text = "\
column_meters_0=LeftCPUs8 Memory Swap
column_meter_modes_0=1 1 1
column_meters_1=RightCPUs8 Tasks LoadAverage Uptime
column_meter_modes_1=1 2 2 2
";
        let c = parse(text);
        assert_eq!(c.meters_left[0].name, "LeftCPUs8");
        assert_eq!(c.meters_right[0].name, "RightCPUs8");
        // Every grid shape htop can write has to survive the import.
        for name in [
            "LeftCPUs",
            "LeftCPUs2",
            "LeftCPUs4",
            "LeftCPUs8",
            "RightCPUs",
            "RightCPUs2",
            "RightCPUs4",
            "RightCPUs8",
            "AllCPUs",
            "AllCPUs8",
        ] {
            assert!(meter_name_known(name), "{name} must import");
        }
        // And a name htop never writes must not.
        assert!(!meter_name_known("LeftCPUs3"));
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

#[cfg(test)]
mod no_silent_column_loss_tests {
    use super::*;

    /// Every field id the importer maps must produce a name that survives
    /// `TabConfig::fields()`.
    ///
    /// This is the user-visible half of the missing-metadata bug: `htop_field_name`
    /// returned `"UNKNOWN"` for nine fields, `parse` put that string in the column
    /// list, and `TabConfig::fields()` dropped it via `filter_map`. The import
    /// "worked" — it just silently produced a different layout than the htoprc
    /// described, with no error and no way to see that anything was missing.
    #[test]
    fn every_mapped_id_resolves_to_a_selectable_column() {
        for raw in 0..160u32 {
            let id = raw.to_string();
            let Some(name) = htop_field_name(&id) else {
                continue;
            };
            assert_ne!(name, "UNKNOWN", "field id {raw} maps to UNKNOWN");
            let field = Field::from_name(name)
                .unwrap_or_else(|| panic!("field id {raw} maps to unselectable {name}"));
            assert_eq!(field.name(), name);
            assert!(field.meta().is_some(), "{name} has no metadata");
        }
    }

    /// The specific ids that used to be lost, checked one by one so a regression
    /// names the field that broke.
    #[test]
    fn session_pgrp_and_proc_columns_import() {
        // htop ids: SESSION 6, PROC_COMM 124, PROC_EXE 125, CWD 126; the file
        // stores each minus one.
        for (stored, want) in [
            ("5", "SESSION"),
            ("123", "PROC_COMM"),
            ("124", "PROC_EXE"),
            ("125", "CWD"),
        ] {
            assert_eq!(htop_field_name(stored), Some(want), "id {stored}");
        }
    }

    /// A column list containing them must come through with all of them, in order.
    #[test]
    fn a_full_column_list_imports_without_dropping_any() {
        // PID, SESSION, PROC_COMM, PROC_EXE, CWD, Command
        let c = parse("fields=0 5 123 124 125 1\n");
        let names: Vec<String> = c.active_tab().columns.clone();
        for want in ["PID", "SESSION", "PROC_COMM", "PROC_EXE", "CWD", "COMM"] {
            assert!(
                names.iter().any(|n| n == want),
                "{want} missing from {names:?}"
            );
        }
        assert_eq!(
            names.iter().filter(|n| *n == "UNKNOWN").count(),
            0,
            "no column may import as UNKNOWN: {names:?}"
        );
    }

    /// `sort_key` on a previously metadata-less field must sort by it, not fall
    /// back to PERCENT_CPU.
    #[test]
    fn sort_key_on_session_sorts_by_session() {
        let c = parse("sort_key=5\n");
        assert_eq!(c.active_tab().sort().field, Field::Session);
    }
}
