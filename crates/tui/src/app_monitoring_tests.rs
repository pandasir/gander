use super::*;
use gander_core::metrics::{Availability, Metric, MetricAvailability};
use ratatui::{backend::TestBackend, Terminal};

fn row(pid: u32, parent: u32, name: &str) -> Row {
    let mut row = Row::default_for_test(pid);
    row.starttime = pid as u64 * 10;
    row.ppid = parent;
    row.comm.try_push_str(name).unwrap();
    row
}

fn app(rows: Vec<Row>) -> App {
    let mut app = App::new(Config::default());
    app.history_dir = std::env::temp_dir().join(format!(
        "gander-monitoring-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    app.apply_sample(SystemSnapshot::default(), rows);
    app
}

fn draw(app: &mut App, width: u16, height: u16) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    terminal.backend().buffer().clone()
}

fn text(buffer: &Buffer) -> String {
    buffer.content().iter().map(|cell| cell.symbol()).collect()
}

#[test]
fn unavailable_metrics_are_placeholders_in_all_resource_fields() {
    let mut row = row(7, 1, "unreadable");
    row.availability = MetricAvailability::all(Availability::PermissionDenied);
    let mut users = gander_core::users::Users::new();
    for field in [
        Field::PercentCpu,
        Field::PercentNormCpu,
        Field::PercentMem,
        Field::ResMem,
        Field::VirtMem,
        Field::Time,
        Field::Priority,
        Field::Nlwp,
    ] {
        assert_eq!(
            render_field(&row, field, &SystemSnapshot::default(), &mut users),
            "-"
        );
    }
    row.availability.set(Metric::Cpu, Availability::Available);
    assert_eq!(
        render_field(
            &row,
            Field::PercentCpu,
            &SystemSnapshot::default(),
            &mut users
        ),
        "0.0"
    );
}

#[test]
fn inspector_reserves_facts_and_limits_long_command_summary() {
    let mut row = row(7, 1, "long-command");
    row.cmdline = Some("X".repeat(2000).into());
    let mut buffer = Buffer::empty(Rect::new(0, 0, 38, 14));
    crate::layout::draw_inspector(
        buffer.area,
        &mut buffer,
        &Theme::default(),
        Some(&row),
        &SystemSnapshot::default(),
        Some("owner"),
    );
    let output = text(&buffer);
    assert!(output.contains("STATE"));
    assert!(output.contains("CPU"));
    assert!(output.contains("MEM"));
    assert!(output.contains("..."));
    assert!(output.matches('X').count() <= 36 * 4);
}

#[test]
fn full_command_reflows_without_discarding_source_and_uses_full_width() {
    let mut row = row(7, 1, "command");
    row.cmdline = Some(format!("/very/long/path  {}   end\t\n", "argument".repeat(150)).into());
    let expected = crate::command::escaped(row.display_command());
    let mut app = app(vec![row]);
    app.handle_key(Key::Char('w'));
    for (width, height) in [(120, 24), (56, 18), (100, 24), (160, 32)] {
        let output = draw(&mut app, width, height);
        let Screen::Info { lines, .. } = &app.screen else {
            panic!()
        };
        assert_eq!(lines.concat(), expected);
        assert!(
            lines
                .iter()
                .all(|line| unicode_width::UnicodeWidthStr::width(line.as_str())
                    <= width as usize - 2)
        );
        assert_ne!(
            output[(width - 1, 1)].symbol(),
            " ",
            "the modal border reaches the full viewport"
        );
        app.handle_key(Key::Down);
    }
    app.handle_key(Key::Esc);
    assert_eq!(app.selected_row().unwrap().pid, 7);
    assert!(app.command_source.is_none());
}

#[test]
fn sidebar_preferences_survive_automatic_hiding_and_layout_changes() {
    let mut app = app(vec![row(7, 1, "command")]);
    for (width, height) in [
        (160, 32),
        (120, 24),
        (100, 24),
        (80, 24),
        (64, 20),
        (20, 4),
        (1, 1),
    ] {
        draw(&mut app, width, height);
        assert!(app.config.show_inspector);
        assert!(app.config.show_system_panel);
    }
    draw(&mut app, 160, 32);
    app.handle_key(Key::Char('V'));
    assert!(!app.config.show_inspector);
    app.handle_key(Key::Char('B'));
    assert!(!app.config.show_system_panel);
    let output = text(&draw(&mut app, 100, 24));
    assert!(!output.contains("TASKS"));
    app.handle_key(Key::Char('L'));
    app.handle_key(Key::Char('L'));
    assert!(!app.config.show_inspector);
    assert!(!app.config.show_system_panel);
}

#[test]
fn sort_picker_cancel_commit_inversion_and_mouse_share_state() {
    let mut app = app(vec![row(7, 1, "first"), row(8, 1, "second")]);
    app.handle_key(Key::F(6));
    assert!(matches!(app.screen, Screen::SortPicker { .. }));
    app.handle_key(Key::Down);
    app.handle_key(Key::Esc);
    assert_eq!(app.table.sort.field, Field::PercentCpu);
    app.handle_key(Key::F(6));
    app.screen = Screen::SortPicker {
        cursor: Field::all()
            .position(|field| field == Field::ResMem)
            .unwrap(),
    };
    app.handle_key(Key::Enter);
    assert_eq!(app.table.sort.field, Field::ResMem);
    assert_eq!(app.table.sort.dir, Dir::Desc);
    app.handle_key(Key::Char('I'));
    assert_eq!(app.table.sort.dir, Dir::Asc);
    for (width, height) in [(160, 32), (80, 24), (100, 24)] {
        draw(&mut app, width, height);
        let rect = app
            .header_regions
            .iter()
            .find(|(_, field)| *field == Field::PercentCpu)
            .unwrap()
            .0;
        let before = app.table.sort.field;
        app.click_header(rect.x, rect.y);
        assert_eq!(app.table.sort.field, Field::PercentCpu);
        if before != Field::PercentCpu {
            assert_eq!(app.table.sort.dir, Dir::Desc);
        }
        let direction = app.table.sort.dir;
        app.click_header(rect.x, rect.y);
        assert_ne!(app.table.sort.dir, direction);
    }
    app.table.rows[0].cmdline = Some("long-argument".repeat(100).into());
    app.h_offset = 200;
    draw(&mut app, 80, 24);
    let (rect, field) = app.header_regions[0];
    app.click_header(rect.x, rect.y);
    assert_eq!(
        app.table.sort.field, field,
        "horizontal scrolling uses painted hit regions"
    );
    app.layout = LayoutKind::Htop;
    draw(&mut app, 120, 24);
    let (rect, field) = app.header_regions[0];
    app.click_header(rect.x, rect.y);
    assert_eq!(app.table.sort.field, field);
    app.invalidate_geometry();
    let before = app.table.sort.field;
    app.click_header(0, 0);
    assert_eq!(
        app.table.sort.field, before,
        "stale hit regions are discarded"
    );
    app.handle_key(Key::F(9));
    let targets = app.action_targets.clone();
    app.click_header(5, 1);
    assert_eq!(app.action_targets, targets);
}

#[test]
fn tree_folding_tracks_identity_and_keeps_speed_independent() {
    let rows = vec![
        row(1, 0, "root"),
        row(2, 1, "child"),
        row(3, 2, "grandchild"),
        row(4, 1, "sibling"),
    ];
    let mut app = app(rows.clone());
    app.handle_key(Key::Char('t'));
    app.selected = app
        .table
        .view
        .order
        .iter()
        .position(|&i| app.table.rows[i].pid == 2)
        .unwrap();
    let delay = app.config.delay_ms;
    app.handle_key(Key::Char('-'));
    assert_eq!(app.config.delay_ms, delay);
    assert_eq!(app.table.visible(), 3);
    app.handle_key(Key::Char('c'));
    assert_eq!(
        app.process_targets().len(),
        2,
        "tagging includes hidden real descendants"
    );
    app.tags.clear();
    app.filter = Filter::parse("child");
    app.reorder();
    app.filter = None;
    app.reorder();
    assert_eq!(
        app.table.visible(),
        3,
        "filter round trips retain fold state"
    );
    app.apply_sample(SystemSnapshot::default(), rows.clone());
    assert_eq!(app.table.visible(), 3);
    assert_eq!(app.selected_row().unwrap().pid, 2);
    app.handle_key(Key::Char('+'));
    assert_eq!(app.table.visible(), 4);
    app.selected = 2;
    app.handle_key(Key::Char('*'));
    assert_eq!(app.table.visible(), 1);
    assert_eq!(app.selected_row().unwrap().pid, 1);
    app.handle_key(Key::Char('*'));
    assert_eq!(app.table.visible(), 4);
    app.selected = 1;
    app.handle_key(Key::Char('-'));
    let mut reused = rows;
    reused[1].starttime += 1;
    app.apply_sample(SystemSnapshot::default(), reused);
    assert_eq!(app.table.visible(), 4);
    app.handle_key(Key::Char(']'));
    assert!(app.config.delay_ms < delay);
    app.config.delay_ms = 100;
    app.handle_key(Key::Char(']'));
    assert_eq!(app.config.delay_ms, 100);
    app.config.delay_ms = 10_000;
    app.handle_key(Key::Char('['));
    assert_eq!(app.config.delay_ms, 10_000);
}

#[test]
fn starter_tabs_find_hotspots_without_retargeting_selection() {
    let mut first = row(7, 1, "first");
    first.percent_cpu = 90.0;
    first.percent_mem = 1.0;
    let mut second = row(8, 1, "second");
    second.percent_cpu = 1.0;
    second.percent_mem = 10.0;
    let mut app = app(vec![first, second]);
    assert_eq!(app.table.row_at(0).unwrap().pid, 7);
    app.handle_key(Key::Tab);
    assert_eq!(app.config.active_tab().name, "Memory");
    assert_eq!(app.table.row_at(0).unwrap().pid, 8);
    assert_eq!(app.selected_row().unwrap().pid, 7);
    assert!(app.columns().contains(&Field::ResMem));
    app.handle_key(Key::Tab);
    assert_eq!(app.config.active_tab().name, "All");
    assert_eq!(app.table.sort.field, Field::Pid);
}

#[test]
fn search_expands_the_path_to_a_folded_descendant() {
    let mut app = app(vec![
        row(1, 0, "root"),
        row(2, 1, "child"),
        row(3, 2, "grandchild"),
    ]);
    app.handle_key(Key::Char('t'));
    app.handle_key(Key::Char('*'));
    assert_eq!(app.table.visible(), 1);
    app.jump_to_match("grandchild");
    assert_eq!(app.selected_row().unwrap().pid, 3);
}

#[test]
fn filters_remain_visible_after_confirmation_and_no_matches_are_explicit() {
    let mut app = app(vec![row(1, 0, "first"), row(2, 1, "second")]);
    app.handle_key(Key::F(4));
    for ch in "second".chars() {
        app.handle_key(Key::Char(ch));
    }
    app.handle_key(Key::Enter);
    let output = text(&draw(&mut app, 100, 24));
    assert!(output.contains("Filter: second"));
    assert!(output.contains("1/2"));
    app.filter = Filter::parse("no-such-command");
    app.reorder();
    let output = text(&draw(&mut app, 80, 24));
    assert!(output.contains("No matching processes"));
    assert!(output.contains("0/2"));
    assert!(app.process_targets().is_empty());
    app.handle_key(Key::Esc);
    assert_eq!(app.table.visible(), 2);
    let _ = std::fs::remove_dir_all(&app.history_dir);
}

#[test]
fn user_picker_is_searchable_provisional_and_supports_empty_owners() {
    let mut first = row(1, 0, "first");
    first.uid = 1000;
    let mut second = row(2, 0, "second");
    second.uid = 1001;
    let mut app = app(vec![first, second]);
    app.handle_key(Key::Char('u'));
    for ch in "1001".chars() {
        app.handle_key(Key::Char(ch));
    }
    app.handle_key(Key::Esc);
    assert_eq!(app.user_filter, None);
    app.handle_key(Key::Char('u'));
    for ch in "1001".chars() {
        app.handle_key(Key::Char(ch));
    }
    app.handle_key(Key::Enter);
    assert_eq!(app.user_filter, Some(1001));
    assert_eq!(app.table.visible(), 1);
    assert!(text(&draw(&mut app, 100, 24)).contains("User: 1001"));
    app.apply_sample(SystemSnapshot::default(), Vec::new());
    assert_eq!(app.user_filter, Some(1001));
    assert!(app.process_targets().is_empty());
}
