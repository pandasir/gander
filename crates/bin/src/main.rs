//! `gander` — a cross-platform interactive process viewer.
//!
//! Batch mode (`-n`, `--json`) exists because `gander-core` has no UI
//! dependency: the same data the TUI shows can be emitted for monitoring
//! (`docs/DESIGN.md` §5).

use gander_config as config_crate;

mod args;
mod batch;

fn main() -> std::process::ExitCode {
    let args = match args::Args::parse(std::env::args().skip(1)) {
        Ok(Some(args)) => args,
        Ok(None) => return std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("gander: {e}");
            eprintln!("Try 'gander --help' for more information.");
            return std::process::ExitCode::from(2);
        }
    };

    // Import an htoprc before anything else, so the converted settings are what
    // the run actually uses.
    let mut config = if let Some(path) = &args.import_htoprc {
        match std::fs::read_to_string(path) {
            Ok(text) => {
                let imported = config_crate::htoprc::parse(&text);
                if args.verbose {
                    eprintln!("gander: imported htoprc from {}", path.display());
                }
                imported
            }
            Err(e) => {
                eprintln!("gander: cannot read {}: {e}", path.display());
                return std::process::ExitCode::from(1);
            }
        }
    } else {
        let path = config_crate::config_path();
        match config_crate::load(&path) {
            Ok(config) => config,
            Err(e) => {
                eprintln!("gander: cannot load config {}: {e}", path.display());
                return std::process::ExitCode::from(1);
            }
        }
    };

    if let Some(delay) = args.delay_ms {
        config.delay_ms = delay;
    }

    if args.import_htoprc.is_some() && args.save {
        if let Err(e) = config_crate::save(&config, &config_crate::config_path()) {
            eprintln!("gander: cannot save config: {e}");
            return std::process::ExitCode::from(1);
        }
    }

    if args.batch.json || args.batch.iterations.is_some() {
        return batch::run(&config, &args.batch);
    }

    if !gander_core::PLATFORM_SUPPORTED {
        // Naming the platforms that *do* work is the whole value of this message;
        // it said "Linux only" for two milestones after macOS had shipped.
        eprintln!(
            "gander: this platform is not supported yet. \
             gander runs on Linux, macOS and FreeBSD."
        );
        return std::process::ExitCode::from(1);
    }

    let app = gander_tui::App::new(config.clone());
    let result = gander_tui::runtime::run(app);

    match result {
        Ok(final_config) => {
            // Save on clean exit only; a signal must lose changes. What gets
            // saved is the config the app *ended* with, not the one it started
            // with: in-app edits (columns, tabs, meter modes, theme, delay)
            // all live only in `App` until now.
            if let Err(e) = config_crate::save(&final_config, &config_crate::config_path()) {
                eprintln!("gander: cannot save config: {e}");
                return std::process::ExitCode::from(1);
            }
            std::process::ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("gander: {e}");
            std::process::ExitCode::from(1)
        }
    }
}
