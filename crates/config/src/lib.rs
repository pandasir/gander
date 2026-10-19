//! Configuration model, TOML persistence, and `htoprc` import.

use std::path::{Path, PathBuf};

use gander_core::row::Field;
use gander_core::table::{Dir, Sort};
use serde::{Deserialize, Serialize};

pub mod htoprc;

/// The user's persisted settings.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Seconds between samples. htop's unit is tenths; we use seconds.
    pub delay_ms: u64,
    /// Columns per screen tab; each tab is an independent layout.
    pub tabs: Vec<TabConfig>,
    /// Index of the active tab.
    pub active_tab: usize,
    /// Meters in the header's left column, top to bottom.
    pub meters_left: Vec<MeterConfig>,
    /// Meters in the header's right column, top to bottom.
    pub meters_right: Vec<MeterConfig>,
    pub tree_mode: bool,
    pub show_kernel_threads: bool,
    /// Whether to list one row per thread inside each process (`H`).
    pub show_userland_threads: bool,
    /// Screen layout: `panels` (system column, table, inspector) or `htop`
    /// (horizontal meters, full-width table). See `docs/LAYOUT.md`.
    pub layout: String,
    pub highlight_base_name: bool,
    pub highlight_deleted_exe: bool,
    pub color_scheme: String,
    /// Maximum number of cores rendered in the `panels` system column's
    /// per-core grid. `0` means "no cap": render every core. Defaults to `64`,
    /// enough for the grid no longer to dominate the column on machines we
    /// tested on; raise it if you run gander on a box with 128 logical CPUs.
    pub system_cores_max: usize,
}

/// One header meter, with its own display mode.
///
/// The mode is per meter, not global: htop's setup screen changes only the meter
/// under the cursor, and having one `I` key flip the whole header was the wrong
/// shape. The name is the same one `meters_impl::meter_by_name` accepts.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MeterConfig {
    pub name: String,
    /// `bar`, `text` or `graph`.
    pub mode: String,
}

impl MeterConfig {
    pub fn new(name: &str, mode: &str) -> Self {
        Self {
            name: name.to_string(),
            mode: mode.to_string(),
        }
    }

    /// The mode, falling back to `bar` for anything unrecognised.
    ///
    /// Accepts htoprc's numeric spellings (1 = bar, 2 = text, 3 = graph) so an
    /// imported file does not need translating first.
    pub fn mode_name(&self) -> &'static str {
        match self.mode.trim() {
            "1" | "bar" | "Bar" | "BAR" => "bar",
            "2" | "text" | "Text" | "TEXT" => "text",
            "3" | "graph" | "Graph" | "GRAPH" => "graph",
            _ => "bar",
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        let default_fields: Vec<String> = vec![
            "PID".into(),
            "USER".into(),
            "PRIORITY".into(),
            "NICE".into(),
            "PERCENT_CPU".into(),
            "PERCENT_MEM".into(),
            "TIME".into(),
            "COMM".into(),
        ];

        Self {
            delay_ms: 1500,
            tabs: vec![TabConfig {
                name: "Main".into(),
                columns: default_fields,
                sort: SortConfig {
                    field: "PERCENT_CPU".into(),
                    desc: true,
                },
            }],
            active_tab: 0,
            // htop's default header. `LeftCPUs`/`RightCPUs` split themselves
            // once the core count is known, so the default is stable across
            // machines.
            meters_left: vec![
                MeterConfig::new("LeftCPUs", "bar"),
                MeterConfig::new("Memory", "bar"),
                MeterConfig::new("Swap", "bar"),
            ],
            meters_right: vec![
                MeterConfig::new("RightCPUs", "bar"),
                MeterConfig::new("Tasks", "text"),
                MeterConfig::new("LoadAverage", "text"),
                MeterConfig::new("Uptime", "text"),
            ],
            tree_mode: false,
            show_kernel_threads: true,
            show_userland_threads: false,
            // The panels layout is the default because it is the one that is
            // actually different from htop; the htop arrangement is available
            // for anyone who came here wanting htop.
            layout: "panels".into(),
            highlight_base_name: false,
            highlight_deleted_exe: true,
            color_scheme: "gander".into(),
            system_cores_max: 64,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TabConfig {
    pub name: String,
    /// Field names, as produced by `Field::name()`.
    pub columns: Vec<String>,
    pub sort: SortConfig,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SortConfig {
    pub field: String,
    pub desc: bool,
}

impl TabConfig {
    /// Resolve the configured column names, silently dropping unknown ones.
    pub fn fields(&self) -> Vec<Field> {
        self.columns
            .iter()
            .filter_map(|c| Field::from_name(c))
            .collect()
    }

    pub fn sort(&self) -> Sort {
        Sort {
            field: Field::from_name(&self.sort.field).unwrap_or(Field::PercentCpu),
            dir: if self.sort.desc { Dir::Desc } else { Dir::Asc },
        }
    }
}

impl Config {
    /// Apply a sampler delay, clamped to htop's documented 1..=100 tenths.
    pub fn delay(&self) -> std::time::Duration {
        // htop clamps the delay to 1..=100 tenths of a second.
        std::time::Duration::from_millis(self.delay_ms.clamp(100, 10_000))
    }

    pub fn active_tab(&self) -> &TabConfig {
        &self.tabs[self.active_tab.min(self.tabs.len().saturating_sub(1))]
    }
}

/// Resolve the config path: `$GANDER_CONFIG`, else XDG.
pub fn config_path() -> PathBuf {
    if let Some(p) = std::env::var_os("GANDER_CONFIG") {
        return PathBuf::from(p);
    }
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let home = std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_default();
            home.join(".config")
        });
    base.join("gander").join("config.toml")
}

/// Resolve the state path for search/filter history.
pub fn state_dir() -> PathBuf {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let home = std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_default();
            home.join(".local").join("state")
        });
    base.join("gander")
}

/// Load configuration, falling back to defaults on any problem.
///
/// A first run must work with no file present, so this never fails hard.
pub fn load(path: &Path) -> Config {
    match std::fs::read_to_string(path) {
        Ok(text) => toml::from_str(&text).unwrap_or_default(),
        Err(_) => Config::default(),
    }
}

/// Persist configuration. Called on clean exit only — a signal must lose changes,
/// matching htop's documented behaviour (`docs/DESIGN.md` §8).
pub fn save(config: &Config, path: &Path) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let text = toml::to_string_pretty(config)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(path, text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_round_trips() {
        let c = Config::default();
        let text = toml::to_string_pretty(&c).unwrap();
        let back: Config = toml::from_str(&text).unwrap();
        assert_eq!(back.delay_ms, c.delay_ms);
        assert_eq!(back.tabs.len(), 1);
        assert_eq!(back.active_tab().sort().field, Field::PercentCpu);
    }

    #[test]
    fn unknown_columns_are_dropped() {
        let tab = TabConfig {
            name: "t".into(),
            columns: vec!["PID".into(), "NOT_A_FIELD".into(), "USER".into()],
            sort: SortConfig {
                field: "PID".into(),
                desc: false,
            },
        };
        assert_eq!(tab.fields(), vec![Field::Pid, Field::User]);
    }

    #[test]
    fn delay_is_clamped() {
        let low = Config {
            delay_ms: 0,
            ..Default::default()
        };
        assert_eq!(low.delay().as_millis(), 100);
        let high = Config {
            delay_ms: 99_999,
            ..Default::default()
        };
        assert_eq!(high.delay().as_millis(), 10_000);
    }

    #[test]
    fn missing_file_yields_defaults() {
        let c = load(Path::new("/nonexistent/gander/config.toml"));
        assert_eq!(c.delay_ms, 1500);
    }
}
