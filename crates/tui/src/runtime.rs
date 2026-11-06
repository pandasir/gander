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

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "freebsd"))]
mod platform {
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::mpsc::{self, Receiver, Sender};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use arc_swap::ArcSwap;
    use gander_core::row::Row;
    use gander_core::system::SystemSnapshot;
    use gander_core::PlatformSampler as _;
    use ratatui::backend::CrosstermBackend;
    use ratatui::crossterm::event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, MouseButton, MouseEventKind,
    };
    use ratatui::crossterm::execute;
    use ratatui::crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
    use ratatui::Terminal;

    use crate::app::{App, Reaction};

    /// The sample delay, shared between the UI and the sampler threads.
    ///
    /// `+`/`-` change it at runtime, and the sampler has to see the new value on
    /// its next cycle. Passing a `Duration` by value to `spawn_sampler` meant the
    /// thread kept the value it started with, so changing the speed appeared to
    /// do nothing until a restart — the same class of bug as the thread-view
    /// flag, which is why both are shared now.
    type DelayMs = Arc<AtomicU64>;

    /// Sample delay in milliseconds, clamped to htop's documented range.
    ///
    /// A zero delay would spin the sampler at full speed, so it is floored at
    /// 100 ms even if a config or an arithmetic slip asks for less.
    pub(crate) fn clamp_delay_ms(ms: u64) -> u64 {
        ms.clamp(100, 10_000)
    }

    /// One completed sample handed from the sampler to the UI.
    pub struct Sample {
        pub snapshot: SystemSnapshot,
        pub rows: Vec<Row>,
    }

    /// Shared slot written by the sampler thread and read by the renderer.
    pub type Slot = Arc<ArcSwap<Option<Sample>>>;

    enum RuntimeEvent {
        Input(std::io::Result<Event>),
        SampleReady,
    }

    #[derive(Default)]
    struct Redraw {
        dirty: bool,
    }

    impl Redraw {
        fn invalidate(&mut self) {
            self.dirty = true;
        }
        fn take(&mut self) -> bool {
            std::mem::take(&mut self.dirty)
        }
    }

    /// Shared "which expensive per-process reads are needed" slot.
    ///
    /// `smaps_rollup` makes the kernel walk every VMA of the process and `fdinfo`
    /// costs a directory walk per process; together they dwarf the rest of a scan.
    /// The columns that need them are chosen in the setup screen, on the UI thread,
    /// so the decision has to cross the thread boundary. It is stored in an
    /// `AtomicU64` so threads, smaps, GPU and delay accounting are published as
    /// one value; the sampler can never observe a half-updated combination.
    type OptionsSlot = Arc<AtomicU64>;

    /// Run the application until the user quits.
    ///
    /// Returns the app's final configuration so the caller can persist it.
    /// It used to return `()`, and `main` then saved the *config it started
    /// with* — so every in-app change (meter modes, columns, tabs, theme,
    /// delay) was silently thrown away on exit. That is also why the config
    /// appeared to "not remember" settings between runs.
    pub fn run(mut app: App) -> std::io::Result<gander_config::Config> {
        // Search/filter history is loaded here rather than in `App::new` so the
        // tests that build an `App` directly never read the developer's real
        // `$XDG_STATE_HOME`.
        app = app.with_history(gander_config::history::dir());

        let mut sampler = gander_core::Sampler::default();
        let slot: Slot = Arc::new(ArcSwap::from_pointee(None));

        // Prime the first sample synchronously so the very first frame has data.
        sampler.set_scan_options(app.scan_options());
        let (snapshot, rows) = sampler.sample();
        app.apply_sample(snapshot, rows);

        let options = Arc::new(AtomicU64::new(encode_options(app.scan_options())));
        let delay_ms = Arc::new(AtomicU64::new(clamp_delay_ms(app.config.delay_ms)));
        let mut terminal = init_terminal()?;
        let (sender, receiver) = mpsc::channel();
        let (handle, stop) = spawn_sampler_notified(
            slot.clone(),
            delay_ms.clone(),
            options.clone(),
            Some(sender.clone()),
        );
        let input_stop = stop.clone();
        let input = std::thread::spawn(move || {
            while !input_stop.load(Ordering::Relaxed) {
                match event::poll(Duration::from_millis(50)) {
                    Ok(false) => continue,
                    Ok(true) => {
                        let result = event::read();
                        let failed = result.is_err();
                        if sender.send(RuntimeEvent::Input(result)).is_err() || failed {
                            break;
                        }
                    }
                    Err(error) => {
                        let _ = sender.send(RuntimeEvent::Input(Err(error)));
                        break;
                    }
                }
            }
        });
        let result = event_loop(
            &mut terminal.terminal,
            &mut app,
            &slot,
            &options,
            &delay_ms,
            &receiver,
        );
        stop.store(true, Ordering::Relaxed);
        let _ = handle.join();
        let _ = input.join();
        terminal.restore()?;

        // History is written on every submission, but a final flush covers a
        // prompt that was still open at exit.
        app.save_history();

        // Only a normal exit returns writable configuration.
        result.map(|()| app.config)
    }

    /// Pack `ScanOptions` into one integer, so a sampler never sees a torn update.
    ///
    /// One `AtomicBool` per flag would let the sampler read a mix of old and new
    /// values — a sample taken with `smaps` on but `gpu` off, say, costing a VMA
    /// walk for a column that is no longer displayed. One atomic write cannot tear.
    fn encode_options(opts: gander_core::ScanOptions) -> u64 {
        (opts.threads as u64)
            | ((opts.smaps as u64) << 1)
            | ((opts.gpu as u64) << 2)
            | ((opts.delayacct as u64) << 3)
    }

    fn decode_options(bits: u64) -> gander_core::ScanOptions {
        gander_core::ScanOptions {
            threads: bits & 1 != 0,
            smaps: bits & 2 != 0,
            gpu: bits & 4 != 0,
            delayacct: bits & 8 != 0,
        }
    }

    #[cfg(test)]
    fn spawn_sampler(
        slot: Slot,
        delay_ms: DelayMs,
        options: OptionsSlot,
    ) -> (std::thread::JoinHandle<()>, Arc<AtomicBool>) {
        spawn_sampler_notified(slot, delay_ms, options, None)
    }

    fn spawn_sampler_notified(
        slot: Slot,
        delay_ms: DelayMs,
        options: OptionsSlot,
        sender: Option<Sender<RuntimeEvent>>,
    ) -> (std::thread::JoinHandle<()>, Arc<AtomicBool>) {
        let stop = Arc::new(AtomicBool::new(false));
        let stop_flag = stop.clone();

        let handle = std::thread::Builder::new()
            .name("gander-sampler".into())
            .spawn(move || {
                let mut sampler = gander_core::Sampler::default();
                while !stop_flag.load(Ordering::Relaxed) {
                    let started = Instant::now();
                    gander_core::PlatformSampler::set_scan_options(
                        &mut sampler,
                        decode_options(options.load(Ordering::Relaxed)),
                    );
                    let (snapshot, rows) = sampler.sample();
                    publish_sample(&slot, sender.as_ref(), Sample { snapshot, rows });

                    // The delay is re-read every cycle, so `+`/`-` take effect on
                    // the next one rather than at the next restart.
                    let step = Duration::from_millis(20);
                    while started.elapsed()
                        < Duration::from_millis(delay_ms.load(Ordering::Relaxed))
                        && !stop_flag.load(Ordering::Relaxed)
                    {
                        std::thread::sleep(step);
                    }
                }
            })
            .expect("spawn sampler thread");

        (handle, stop)
    }

    fn publish_sample(slot: &Slot, sender: Option<&Sender<RuntimeEvent>>, sample: Sample) {
        let previous = slot.swap(Arc::new(Some(sample)));
        if previous.is_none() {
            if let Some(sender) = sender {
                let _ = sender.send(RuntimeEvent::SampleReady);
            }
        }
    }

    fn publish_settings(app: &App, options: &OptionsSlot, delay_ms: &DelayMs) {
        options.store(encode_options(app.scan_options()), Ordering::Relaxed);
        delay_ms.store(clamp_delay_ms(app.config.delay_ms), Ordering::Relaxed);
    }

    fn event_loop(
        terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
        app: &mut App,
        slot: &Slot,
        options: &OptionsSlot,
        delay_ms: &DelayMs,
        receiver: &Receiver<RuntimeEvent>,
    ) -> std::io::Result<()> {
        let mut redraw = Redraw::default();
        redraw.invalidate();
        let mut timed = timed_token(app);
        loop {
            if redraw.take() {
                terminal.draw(|frame| app.draw(frame))?;
            }
            let event = if timed.is_some() {
                match receiver.recv_timeout(Duration::from_millis(100)) {
                    Ok(event) => Some(event),
                    Err(mpsc::RecvTimeoutError::Timeout) => None,
                    Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
                }
            } else {
                Some(
                    receiver
                        .recv()
                        .map_err(|_| std::io::Error::other("terminal event channel closed"))?,
                )
            };
            for event in event.into_iter().chain(receiver.try_iter()).take(128) {
                if dispatch_event(event, app, slot, options, delay_ms, &mut redraw)? {
                    return Ok(());
                }
            }
            let next_timed = timed_token(app);
            if next_timed != timed {
                app.refresh_timed_meters();
                redraw.invalidate();
                timed = next_timed;
            }
        }
    }

    fn timed_token(app: &App) -> Option<String> {
        if app.layout != crate::app::LayoutKind::Htop
            || app.screen != crate::app::Screen::ProcessList
        {
            return None;
        }
        let clock = app.meters.iter().any(|meter| meter.name() == "Clock");
        let date = app.meters.iter().any(|meter| meter.name() == "Date");
        if clock {
            Some(crate::meters_impl::local_clock())
        } else if date {
            Some(crate::meters_impl::local_date())
        } else {
            None
        }
    }

    fn dispatch_event(
        event: RuntimeEvent,
        app: &mut App,
        slot: &Slot,
        options: &OptionsSlot,
        delay_ms: &DelayMs,
        redraw: &mut Redraw,
    ) -> std::io::Result<bool> {
        match event {
            RuntimeEvent::SampleReady => {
                let taken = slot.swap(Arc::new(None));
                if let Ok(Some(sample)) = Arc::try_unwrap(taken) {
                    app.apply_sample(sample.snapshot, sample.rows);
                    redraw.invalidate();
                }
            }
            RuntimeEvent::Input(event) => match event? {
                Event::Key(key) => {
                    if key.modifiers.contains(super::KeyModifiers::CONTROL)
                        && key.code == super::KeyCode::Char('c')
                    {
                        return Err(std::io::ErrorKind::Interrupted.into());
                    }
                    if let Some(key) = super::translate(key) {
                        if app.handle_key(key) == Reaction::Quit || app.quit {
                            return Ok(true);
                        }
                        publish_settings(app, options, delay_ms);
                        redraw.invalidate();
                    }
                }
                Event::Resize(_, _) => {
                    app.invalidate_geometry();
                    redraw.invalidate();
                }
                Event::Mouse(mouse) if mouse.kind == MouseEventKind::Down(MouseButton::Left) => {
                    app.click_header(mouse.column, mouse.row);
                    redraw.invalidate();
                }
                _ => {}
            },
        }
        Ok(false)
    }

    struct TerminalSession {
        terminal: Terminal<CrosstermBackend<std::io::Stdout>>,
        restored: bool,
    }

    impl TerminalSession {
        fn restore(&mut self) -> std::io::Result<()> {
            let raw = terminal::disable_raw_mode();
            let screen = execute!(
                self.terminal.backend_mut(),
                DisableMouseCapture,
                LeaveAlternateScreen
            );
            let cursor = self.terminal.show_cursor();
            self.restored = raw.is_ok() && screen.is_ok() && cursor.is_ok();
            raw.and(screen).and(cursor)
        }
    }

    impl Drop for TerminalSession {
        fn drop(&mut self) {
            if !self.restored {
                let _ = self.restore();
            }
        }
    }

    fn init_terminal() -> std::io::Result<TerminalSession> {
        terminal::enable_raw_mode()?;
        let mut stdout = std::io::stdout();
        if let Err(error) = execute!(stdout, EnterAlternateScreen, EnableMouseCapture) {
            let _ = execute!(stdout, DisableMouseCapture, LeaveAlternateScreen);
            let _ = terminal::disable_raw_mode();
            return Err(error);
        }
        match Terminal::new(CrosstermBackend::new(stdout)) {
            Ok(terminal) => Ok(TerminalSession {
                terminal,
                restored: false,
            }),
            Err(error) => {
                let _ = execute!(std::io::stdout(), DisableMouseCapture, LeaveAlternateScreen);
                let _ = terminal::disable_raw_mode();
                Err(error)
            }
        }
    }

    #[cfg(test)]
    mod sampler_tests {
        use super::*;
        use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        #[test]
        fn idle_rendering_is_initial_plus_adopted_samples_not_poll_count() {
            let mut redraw = Redraw::default();
            redraw.invalidate();
            let mut draws = usize::from(redraw.take());
            let mut app = App::new(gander_config::Config::default());
            let slot: Slot = Arc::new(ArcSwap::from_pointee(None));
            let options = Arc::new(AtomicU64::new(0));
            let delay = Arc::new(AtomicU64::new(10_000));
            for _ in 0..10 {
                publish_sample(
                    &slot,
                    None,
                    Sample {
                        snapshot: SystemSnapshot::default(),
                        rows: Vec::new(),
                    },
                );
                dispatch_event(
                    RuntimeEvent::SampleReady,
                    &mut app,
                    &slot,
                    &options,
                    &delay,
                    &mut redraw,
                )
                .unwrap();
                draws += usize::from(redraw.take());
                for _ in 0..30 {
                    assert!(!redraw.take());
                }
            }
            assert_eq!(draws, 11);
            let history = app.cpu_history.len();
            dispatch_event(
                RuntimeEvent::Input(Ok(Event::Key(KeyEvent::new(
                    KeyCode::Char('I'),
                    KeyModifiers::NONE,
                )))),
                &mut app,
                &slot,
                &options,
                &delay,
                &mut redraw,
            )
            .unwrap();
            assert!(
                redraw.take(),
                "input invalidates immediately even at a ten-second delay"
            );
            assert_eq!(
                app.cpu_history.len(),
                history,
                "repainting does not sample histories"
            );
        }

        #[test]
        fn ctrl_c_interrupts_modals_instead_of_applying_their_f10_action() {
            let mut app = App::new(gander_config::Config::default());
            app.handle_key(crate::app::Key::F(2));
            let slot: Slot = Arc::new(ArcSwap::from_pointee(None));
            let options = Arc::new(AtomicU64::new(0));
            let delay = Arc::new(AtomicU64::new(10_000));
            let error = dispatch_event(
                RuntimeEvent::Input(Ok(Event::Key(KeyEvent::new(
                    KeyCode::Char('c'),
                    KeyModifiers::CONTROL,
                )))),
                &mut app,
                &slot,
                &options,
                &delay,
                &mut Redraw::default(),
            )
            .unwrap_err();
            assert_eq!(error.kind(), std::io::ErrorKind::Interrupted);
            assert_eq!(app.screen, crate::app::Screen::Setup);
        }

        #[test]
        fn visible_time_meters_have_deadlines_without_advancing_cpu_history() {
            let mut config = gander_config::Config {
                layout: "htop".into(),
                ..Default::default()
            };
            config.meters_left = vec![gander_config::MeterConfig::new("Clock", "text")];
            config.meters_right = vec![gander_config::MeterConfig::new("Date", "text")];
            let mut app = App::new(config);
            app.apply_sample(SystemSnapshot::default(), Vec::new());
            assert_eq!(timed_token(&app), Some(crate::meters_impl::local_clock()));
            let history = app.cpu_history.len();
            app.refresh_timed_meters();
            assert_eq!(app.cpu_history.len(), history);
            app.layout = crate::app::LayoutKind::Panels;
            assert_eq!(timed_token(&app), None);
        }

        #[test]
        fn sample_wakes_are_coalesced_without_losing_a_post_swap_sample() {
            let slot: Slot = Arc::new(ArcSwap::from_pointee(None));
            let (sender, receiver) = mpsc::channel();
            for pid in 1..=3 {
                publish_sample(
                    &slot,
                    Some(&sender),
                    Sample {
                        snapshot: SystemSnapshot::default(),
                        rows: vec![Row::default_for_test(pid)],
                    },
                );
            }
            assert_eq!(receiver.try_iter().count(), 1);
            let latest = slot.swap(Arc::new(None));
            assert_eq!(latest.as_ref().as_ref().unwrap().rows[0].pid, 3);
            publish_sample(
                &slot,
                Some(&sender),
                Sample {
                    snapshot: SystemSnapshot::default(),
                    rows: Vec::new(),
                },
            );
            assert!(receiver.recv_timeout(Duration::from_millis(100)).is_ok());
        }

        struct RunningSampler(std::thread::JoinHandle<()>, Arc<AtomicBool>);

        impl RunningSampler {
            fn stop(self) {
                self.1.store(true, Ordering::Relaxed);
                self.0.join().expect("sampler thread");
            }
        }

        fn wait_for_threads(slot: &Slot, expected: bool) -> bool {
            let deadline = Instant::now() + Duration::from_secs(5);
            while Instant::now() < deadline {
                let sample = slot.swap(Arc::new(None));
                if let Some(sample) = sample.as_ref() {
                    if sample
                        .rows
                        .iter()
                        .any(|r| r.pid == std::process::id() && !r.is_thread)
                        && sample.rows.iter().any(|r| r.is_thread) == expected
                    {
                        return true;
                    }
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            false
        }

        #[test]
        fn h_updates_the_running_sampler_in_both_directions() {
            let mut app = App::new(gander_config::Config {
                delay_ms: 100,
                ..Default::default()
            });
            let slot: Slot = Arc::new(ArcSwap::from_pointee(None));
            let delay = Arc::new(AtomicU64::new(100));
            let options = Arc::new(AtomicU64::new(encode_options(app.scan_options())));
            let (handle, stop) = spawn_sampler(slot.clone(), delay.clone(), options.clone());
            let running = RunningSampler(handle, stop);
            let initially_off = wait_for_threads(&slot, false);
            app.handle_key(crate::app::Key::Char('H'));
            publish_settings(&app, &options, &delay);
            let turned_on = wait_for_threads(&slot, true);
            app.handle_key(crate::app::Key::Char('H'));
            publish_settings(&app, &options, &delay);
            let turned_off = wait_for_threads(&slot, false);
            running.stop();
            assert!(initially_off && turned_on && turned_off);
        }

        #[test]
        fn every_scan_option_round_trips() {
            for bits in 0..16 {
                assert_eq!(encode_options(decode_options(bits)), bits);
            }
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "freebsd"))]
pub use platform::run;

/// Non-Linux stub: there is no backend yet, so the TUI cannot start.
#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "freebsd")))]
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

    /// The sampler reads its delay from shared state, so a change has to be
    /// visible to the thread that is already running.
    ///
    /// This pins the mechanism, not the wiring: `+`/`-` used to change only
    /// `app.config.delay_ms` while the sampler kept the `Duration` it was spawned
    /// with, so the speed keys did nothing until a restart.
    #[test]
    fn the_sample_delay_is_shared_and_clamped() {
        use std::sync::atomic::{AtomicU64, Ordering};
        use std::sync::Arc;

        assert_eq!(platform::clamp_delay_ms(0), 100, "never spin the sampler");
        assert_eq!(platform::clamp_delay_ms(50), 100);
        assert_eq!(platform::clamp_delay_ms(1_500), 1_500);
        assert_eq!(platform::clamp_delay_ms(99_999), 10_000);

        let shared = Arc::new(AtomicU64::new(platform::clamp_delay_ms(1_500)));
        let seen_by_sampler = shared.clone();
        shared.store(platform::clamp_delay_ms(100), Ordering::Relaxed);
        assert_eq!(
            seen_by_sampler.load(Ordering::Relaxed),
            100,
            "a change made on the UI thread must reach the sampler"
        );
    }

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
