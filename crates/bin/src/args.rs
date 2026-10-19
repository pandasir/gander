//! Command-line parsing.
//!
//! Hand-rolled rather than clap: the surface is small and stable, and this keeps
//! the dependency graph tight.

use std::path::PathBuf;

#[derive(Debug, Default)]
pub struct Batch {
    pub json: bool,
    pub iterations: Option<usize>,
    pub filter: Option<String>,
    pub sort: Option<String>,
    pub limit: Option<usize>,
    /// Emit one row per thread in addition to one per process.
    pub threads: bool,
}

#[derive(Debug, Default)]
pub struct Args {
    /// Import an `htoprc` instead of reading the native config.
    pub import_htoprc: Option<PathBuf>,
    /// Write the imported config back to the native location.
    pub save: bool,
    /// Override the sample delay, in milliseconds.
    pub delay_ms: Option<u64>,
    pub verbose: bool,
    pub batch: Batch,
}

impl Args {
    /// Parse argv. `Ok(None)` means help or version was printed.
    pub fn parse<I: Iterator<Item = String>>(args: I) -> Result<Option<Args>, String> {
        let mut out = Args::default();
        let mut args_vec: Vec<String> = Vec::new();
        for a in args {
            args_vec.push(a);
        }

        let mut i = 0;
        while i < args_vec.len() {
            let arg = args_vec[i].clone();
            i += 1;

            let mut take_value = |name: &str| -> Result<String, String> {
                // `inspect` rather than `map` because the closure is here for the
                // side effect (consuming the value) and returns its input
                // unchanged. clippy's manual_inspect lint is right about the
                // intent, and the borrow of `i` is the same either way.
                args_vec
                    .get(i)
                    .cloned()
                    .inspect(|_| i += 1)
                    .ok_or_else(|| format!("option {name} requires a value"))
            };

            match arg.as_str() {
                "-h" | "--help" => {
                    print_help();
                    return Ok(None);
                }
                "-V" | "--version" => {
                    println!("gander {}", env!("CARGO_PKG_VERSION"));
                    return Ok(None);
                }
                "-v" | "--verbose" => out.verbose = true,
                "-d" | "--delay" => {
                    let v = take_value("--delay")?;
                    let ms: u64 = v.parse().map_err(|_| format!("invalid delay: {v}"))?;
                    out.delay_ms = Some(ms);
                }
                "-i" | "--import-htoprc" => {
                    let v = take_value("--import-htoprc")?;
                    out.import_htoprc = Some(PathBuf::from(v));
                }
                "--save" => out.save = true,
                "-n" | "--iterations" => {
                    let v = take_value("--iterations")?;
                    let n: usize = v.parse().map_err(|_| format!("invalid count: {v}"))?;
                    out.batch.iterations = Some(n);
                }
                "--json" => {
                    out.batch.json = true;
                    if out.batch.iterations.is_none() {
                        out.batch.iterations = Some(1);
                    }
                }
                "--threads" => out.batch.threads = true,
                "-F" | "--filter" => {
                    let v = take_value("--filter")?;
                    out.batch.filter = Some(v);
                }
                "--sort" => {
                    let v = take_value("--sort")?;
                    out.batch.sort = Some(v);
                }
                "--limit" => {
                    let v = take_value("--limit")?;
                    out.batch.limit = Some(v.parse().map_err(|_| format!("invalid limit: {v}"))?);
                }
                other if other.starts_with('-') && other != "-" => {
                    return Err(format!("unknown option: {other}"));
                }
                _ => return Err(format!("unexpected argument: {arg}")),
            }
        }

        Ok(Some(out))
    }
}

fn print_help() {
    println!(
        "\
gander {version} — interactive process viewer

USAGE:
    gander [OPTIONS]

OPTIONS:
    -d, --delay <MS>         Sample delay in milliseconds (default 1500)
    -i, --import-htoprc <P>  Read settings from an existing htoprc
        --save               Write the imported settings to the native config
    -n, --iterations <N>     Batch mode: print N samples and exit
        --json               Batch mode: emit JSON
    -F, --filter <TERMS>     Filter command lines (fixed strings, '|'-separated)
        --sort <FIELD>       Sort column for batch output, e.g. PERCENT_CPU
        --limit <N>          Print at most N processes
        --threads            Also emit one row per thread
    -v, --verbose            Extra diagnostics on stderr
    -h, --help               Show this help
    -V, --version            Show the version

KEYS (in the TUI):
    F1 h ?   help        F2     setup        F3 /   search
    F4 \\     filter      F5 t   tree         F6 < > sort
    F7 F8    nice -/+    F9 k   kill         F10 q  quit
    L        layout      H      threads      K      kernel threads
    O        containers  I      meter mode   + -    speed
    Space    tag         c      tag children U      untag all
    Tab      next tab    Shift-Tab  prev tab  u      user filter
    Up Down  move        PgUp PgDn  page
    Home End g G  first / last row     Left Right  scroll columns
    a        affinity    i      I/O priority
    e        env         w      command line l      open files
    x        locks       Z      security ctx s      trace (explains itself)
                                               b      backtrace (ditto)

    Note: htop's I inverts the sort order; here it cycles the meter mode.
    Use F6 / < > / . to change the sort. See docs/KEYS.md.

CONFIG:
    $GANDER_CONFIG, otherwise $XDG_CONFIG_HOME/gander/config.toml
    Saved on clean exit only.",
        version = env!("CARGO_PKG_VERSION")
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &[&str]) -> Result<Option<Args>, String> {
        Args::parse(s.iter().map(|s| s.to_string()))
    }

    #[test]
    fn defaults() {
        let a = parse(&[]).unwrap().unwrap();
        assert_eq!(a.delay_ms, None);
        assert!(!a.batch.json);
    }

    #[test]
    fn delay_and_filter() {
        let a = parse(&["-d", "250", "-F", "rust|node"]).unwrap().unwrap();
        assert_eq!(a.delay_ms, Some(250));
        assert_eq!(a.batch.filter.as_deref(), Some("rust|node"));
    }

    #[test]
    fn json_implies_one_iteration() {
        let a = parse(&["--json"]).unwrap().unwrap();
        assert!(a.batch.json);
        assert_eq!(a.batch.iterations, Some(1));
    }

    #[test]
    fn unknown_option_is_an_error() {
        assert!(parse(&["--nope"]).is_err());
    }

    #[test]
    fn missing_value_is_an_error() {
        assert!(parse(&["--delay"]).is_err());
    }

    #[test]
    fn help_exits_early() {
        assert!(parse(&["--help"]).unwrap().is_none());
    }
}
