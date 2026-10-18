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

    /// Run the application until the user quits.
    pub fn run(mut app: App) -> std::io::Result<()> {
        let mut sampler = gander_core::Sampler::default();
        let slot: Slot = Arc::new(ArcSwap::from_pointee(None));

        // Prime the first sample synchronously so the very first frame has data.
        let (snapshot, rows) = sampler.sample();
        app.apply_sample(snapshot, rows);

        let (handle, stop) = spawn_sampler(slot.clone(), app.config.delay());

        let mut terminal = init_terminal()?;
        let result = event_loop(&mut terminal, &mut app, &slot);

        stop.store(true, Ordering::Relaxed);
        let _ = handle.join();
        restore_terminal(&mut terminal)?;

        result
    }

    fn spawn_sampler(
        slot: Slot,
        delay: Duration,
    ) -> (std::thread::JoinHandle<()>, Arc<AtomicBool>) {
        let stop = Arc::new(AtomicBool::new(false));
        let stop_flag = stop.clone();

        let handle = std::thread::Builder::new()
            .name("gander-sampler".into())
            .spawn(move || {
                let mut sampler = gander_core::Sampler::default();
                while !stop_flag.load(Ordering::Relaxed) {
                    let started = Instant::now();
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
pub fn run(_app: App) -> std::io::Result<()> {
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
    fn plain_letters_pass_through() {
        let ev = KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE);
        assert_eq!(translate(ev), Some(Key::Char('t')));
    }
}
