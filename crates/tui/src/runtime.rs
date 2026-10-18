//! Terminal setup, the event loop, and the sampling thread.
//!
//! The sampler runs off-thread and hands snapshots over an `arc-swap` slot, so
//! rendering never blocks on a `/proc` scan. That is the one place gander can be
//! qualitatively better than htop (`docs/DESIGN.md` §6).
//!
//! The event loop is gated on having a live backend (`PLATFORM_SUPPORTED`); key
//! translation is not, so its tests run everywhere.

use ratatui::crossterm::event::{self, KeyCode, KeyEvent, KeyModifiers};

#[allow(unused_imports)]
use crate::app::{App, Key};

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod platform {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use arc_swap::ArcSwap;
    use gander_core::row::Row;
    use gander_core::system::SystemSnapshot;
    use gander_core::PlatformSampler as _;
    use ratatui::backend::CrosstermBackend;
    use ratatui::crossterm::event::{self, Event};
    use ratatui::crossterm::execute;
    use ratatui::crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
    use ratatui::Terminal;

    use crate::app::{App, Reaction};

    /// One completed sample handed from the sampler to the UI.
    pub struct Sample {
        pub snapshot: SystemSnapshot,
        pub rows: Vec<Row>,
    }

    /// Shared slot written by the sampler thread and read by the renderer.
    pub type Slot = Arc<ArcSwap<Option<Sample>>>;

    /// Input poll interval: keeps the UI responsive between samples.
    /// A 30 Hz cap is well below the terminal's refresh rate and far below the
    /// cost of a `/proc` scan.
    const TICK: Duration = Duration::from_millis(33);

    /// Shared "list threads too" flag.
    ///
    /// The sampler thread runs the scan, but `H` is handled on the UI thread, so
    /// the setting has to be visible to both. It is read once per sample, which
    /// is the coarsest cadence that still feels immediate.
    type ThreadsFlag = Arc<AtomicBool>;

    /// Run the application until the user quits.
    ///
    /// Returns the app's final configuration so the caller can persist it.
    /// It used to return `()`, and `main` then saved the *config it started
    /// with* — so every in-app change (meter modes, columns, tabs, theme,
    /// delay) was silently thrown away on exit. That is also why the config
    /// appeared to "not remember" settings between runs.
    pub fn run(mut app: App) -> std::io::Result<gander_config::Config> {
        let mut sampler = gander_core::Sampler::default();
        let slot: Slot = Arc::new(ArcSwap::from_pointee(None));

        // Prime the first sample synchronously so the very first frame has data.
        let (snapshot, rows) = sampler.sample();
        app.apply_sample(snapshot, rows);

        let threads = Arc::new(AtomicBool::new(app.table.show_userland_threads));
        let (handle, stop) = spawn_sampler(slot.clone(), app.config.delay(), threads.clone());

        let mut terminal = init_terminal()?;
        let result = event_loop(&mut terminal, &mut app, &slot, &threads);

        stop.store(true, Ordering::Relaxed);
        let _ = handle.join();
        // Restore the terminal before returning: a lost config is annoying, a
        // terminal left in raw mode is unusable.
        restore_terminal(&mut terminal)?;

        // A failed event loop still hands back the config: the user got as far as
        // editing it, and throwing that away on an unrelated error is hostile.
        result.map(|()| app.config)
    }

    fn spawn_sampler(
        slot: Slot,
        delay: Duration,
        threads: ThreadsFlag,
    ) -> (std::thread::JoinHandle<()>, Arc<AtomicBool>) {
        let stop = Arc::new(AtomicBool::new(false));
        let stop_flag = stop.clone();

        let handle = std::thread::Builder::new()
            .name("gander-sampler".into())
            .spawn(move || {
                let mut sampler = gander_core::Sampler::default();
                while !stop_flag.load(Ordering::Relaxed) {
                    let started = Instant::now();
                    gander_core::PlatformSampler::set_show_threads(
                        &mut sampler,
                        threads.load(Ordering::Relaxed),
                    );
                    let (snapshot, rows) = sampler.sample();
                    slot.store(Arc::new(Some(Sample { snapshot, rows })));

                    // Sleep the remainder in small steps so a quit is honoured
                    // promptly even with a long sample delay.
                    let step = Duration::from_millis(20).min(delay);
                    while started.elapsed() < delay && !stop_flag.load(Ordering::Relaxed) {
                        std::thread::sleep(step);
                    }
                }
            })
            .expect("spawn sampler thread");

        (handle, stop)
    }

    fn event_loop(
        terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
        app: &mut App,
        slot: &Slot,
        threads: &ThreadsFlag,
    ) -> std::io::Result<()> {
        let mut last_draw = Instant::now() - TICK;

        loop {
            // Adopt the newest sample, if there is one. `swap` clears the slot so
            // each sample is applied exactly once. The sample is *moved* out, not
            // cloned: `Row` owns three `Box<str>`s, so cloning the vector copied
            // thousands of strings per second for nothing
            // (`docs/DESIGN.md` §9.4).
            let taken = slot.swap(Arc::new(None));
            if let Ok(Some(sample)) = Arc::try_unwrap(taken) {
                app.apply_sample(sample.snapshot, sample.rows);
            }

            if last_draw.elapsed() >= TICK {
                terminal.draw(|frame| app.draw(frame))?;
                last_draw = Instant::now();
            }

            if event::poll(TICK)? {
                match event::read()? {
                    Event::Key(key) => {
                        if let Some(k) = super::translate(key) {
                            if app.handle_key(k) == Reaction::Quit || app.quit {
                                return Ok(());
                            }
                            // `H` may have flipped it; the sampler reads this
                            // before its next scan.
                            threads.store(app.table.show_userland_threads, Ordering::Relaxed);
                        }
                    }
                    Event::Resize(_, _) => {
                        terminal.draw(|frame| app.draw(frame))?;
                        last_draw = Instant::now();
                    }
                    _ => {}
                }
            }
        }
    }

    fn init_terminal() -> std::io::Result<Terminal<CrosstermBackend<std::io::Stdout>>> {
        terminal::enable_raw_mode()?;
        let mut stdout = std::io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        Terminal::new(backend)
    }

    fn restore_terminal(
        terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    ) -> std::io::Result<()> {
        terminal::disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
        terminal.show_cursor()
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub use platform::run;

/// Non-Linux stub: there is no backend yet, so the TUI cannot start.
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub fn run(_app: App) -> std::io::Result<gander_config::Config> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "gander has no backend for this platform yet; see docs/PORTABILITY.md",
    ))
}

/// Translate a crossterm key event into the app's key type.
pub fn translate(key: KeyEvent) -> Option<Key> {
    if key.kind == event::KeyEventKind::Release {
        return None;
    }

    // Ctrl-C always quits, even inside a text input.
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        return Some(Key::F(10));
    }

    // Shifted arrows move the header meter cursor, mirroring htop's
    // Shift-Left/Right. They are checked before the plain arrows so a modified
    // arrow never falls through to "scroll the process list".
    if key.modifiers.contains(KeyModifiers::SHIFT) {
        match key.code {
            KeyCode::Left => return Some(Key::HeaderPrev),
            KeyCode::Right => return Some(Key::HeaderNext),
            _ => {}
        }
    }

    Some(match key.code {
        KeyCode::Char(c) => Key::Char(c),
        KeyCode::Up => Key::Up,
        KeyCode::Down => Key::Down,
        KeyCode::Left => Key::Left,
        KeyCode::Right => Key::Right,
        KeyCode::Home => Key::Home,
        KeyCode::End => Key::End,
        KeyCode::PageUp => Key::PgUp,
        KeyCode::PageDown => Key::PgDn,
        KeyCode::Tab => Key::Tab,
        KeyCode::BackTab => Key::BackTab,
        KeyCode::Enter => Key::Enter,
        KeyCode::Esc => Key::Esc,
        KeyCode::Backspace => Key::Backspace,
        KeyCode::Delete => Key::Delete,
        KeyCode::F(n) => Key::F(n),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translates_function_keys() {
        let ev = KeyEvent::new(KeyCode::F(9), KeyModifiers::NONE);
        assert_eq!(translate(ev), Some(Key::F(9)));
    }

    #[test]
    fn ctrl_c_becomes_quit() {
        let ev = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(translate(ev), Some(Key::F(10)));
    }

    #[test]
    fn release_events_are_ignored() {
        let mut ev = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE);
        ev.kind = event::KeyEventKind::Release;
        assert_eq!(translate(ev), None);
    }

    #[test]
    fn shifted_arrows_move_the_header_cursor() {
        let left = KeyEvent::new(KeyCode::Left, KeyModifiers::SHIFT);
        assert_eq!(translate(left), Some(Key::HeaderPrev));
        let right = KeyEvent::new(KeyCode::Right, KeyModifiers::SHIFT);
        assert_eq!(translate(right), Some(Key::HeaderNext));

        // Without the modifier they must stay as table scrolling.
        let plain = KeyEvent::new(KeyCode::Right, KeyModifiers::NONE);
        assert_eq!(translate(plain), Some(Key::Right));
    }

    /// The TUI's exit path has to hand back the config it ended with.
    ///
    /// `run` used to return `()` and `main` saved the config it had *started*
    /// with, so every in-app change was discarded on exit. This pins the
    /// signature: the return type is the mechanism.
    #[test]
    fn run_returns_the_final_config() {
        fn assert_signature<F>(_f: F)
        where
            F: Fn(App) -> std::io::Result<gander_config::Config>,
        {
        }
        assert_signature(platform::run);
    }

    #[test]
    fn plain_letters_pass_through() {
        let ev = KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE);
        assert_eq!(translate(ev), Some(Key::Char('t')));
    }
}
