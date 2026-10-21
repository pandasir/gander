use super::*;

fn row(pid: u32, starttime: u64, cpu: f32) -> Row {
    let mut row = Row::default_for_test(pid);
    row.starttime = starttime;
    row.percent_cpu = cpu;
    row
}

fn app(rows: Vec<Row>) -> App {
    let mut app = App::new(Config::default());
    app.apply_sample(SystemSnapshot::default(), rows);
    app
}

#[test]
fn selection_follows_identity_when_cpu_order_changes() {
    let mut app = app(vec![row(7, 10, 90.0), row(8, 20, 10.0)]);
    app.table.sort = table::Sort {
        field: Field::PercentCpu,
        dir: Dir::Desc,
    };
    app.reorder();
    app.selected = 1;
    app.apply_sample(
        SystemSnapshot::default(),
        vec![row(7, 10, 5.0), row(8, 20, 95.0)],
    );
    assert_eq!(app.selected_row().unwrap().pid, 8);
    assert_eq!(app.selected, 0);
}

#[test]
fn selection_distinguishes_threads_sharing_a_pid() {
    let mut first = row(7, 10, 90.0);
    first.is_thread = true;
    first.thread_id = Some(100);
    let mut second = first.clone();
    second.thread_id = Some(200);
    let mut app = App::new(Config {
        show_userland_threads: true,
        ..Default::default()
    });
    app.apply_sample(
        SystemSnapshot::default(),
        vec![first.clone(), second.clone()],
    );
    app.selected = 1;
    app.apply_sample(SystemSnapshot::default(), vec![second, first]);
    assert_eq!(app.selected_row().unwrap().thread_id, Some(200));
}

#[test]
fn tags_expire_when_process_exits_or_pid_is_reused() {
    let mut app = app(vec![row(7, 10, 0.0), row(8, 20, 0.0)]);
    app.toggle_tag();
    app.selected = 1;
    app.toggle_tag();
    app.apply_sample(
        SystemSnapshot::default(),
        vec![row(7, 11, 0.0), row(8, 20, 0.0)],
    );
    assert_eq!(
        app.process_targets(),
        vec![ProcessTarget {
            pid: 8,
            starttime: 20
        }]
    );
    app.apply_sample(SystemSnapshot::default(), vec![row(7, 11, 0.0)]);
    assert!(app.tags.is_empty());
}

#[test]
fn signal_picker_keeps_its_original_targets_after_selection_disappears() {
    let mut app = app(vec![row(7, 10, 0.0), row(8, 20, 0.0)]);
    app.handle_key(Key::F(9));
    app.apply_sample(SystemSnapshot::default(), vec![row(8, 20, 0.0)]);
    assert_eq!(app.selected_row().unwrap().pid, 8);
    assert_eq!(
        app.action_targets,
        vec![ProcessTarget {
            pid: 7,
            starttime: 10
        }]
    );
}

#[test]
fn bulk_signal_picker_keeps_tags_even_when_new_samples_prune_them() {
    let mut app = app(vec![row(7, 10, 0.0), row(8, 20, 0.0)]);
    app.toggle_tag();
    app.selected = 1;
    app.toggle_tag();
    app.handle_key(Key::F(9));
    app.apply_sample(SystemSnapshot::default(), vec![row(7, 11, 0.0)]);
    assert!(app.tags.is_empty());
    assert_eq!(
        app.action_targets,
        vec![
            ProcessTarget {
                pid: 7,
                starttime: 10
            },
            ProcessTarget {
                pid: 8,
                starttime: 20
            },
        ]
    );
}

#[test]
fn empty_programmatic_config_is_repaired_before_use() {
    let app = App::new(Config {
        tabs: vec![],
        active_tab: 99,
        ..Default::default()
    });
    app.config.validate().unwrap();
    assert_eq!(app.config.active_tab, 0);
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "freebsd"))]
#[test]
fn stale_target_cannot_be_signalled_reniced_or_applied_from_a_panel() {
    struct Child(std::process::Child);
    impl Drop for Child {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let mut child = Child(
        std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap(),
    );
    let target = ProcessTarget::capture(child.0.id()).unwrap();
    let mut app = app(vec![row(target.pid, target.starttime.wrapping_add(1), 0.0)]);
    app.handle_key(Key::F(9));
    app.handle_key(Key::Enter);
    assert!(app.status.as_deref().unwrap().contains("identity changed"));
    assert!(child.0.try_wait().unwrap().is_none());
    app.handle_key(Key::F(8));
    assert!(app.status.as_deref().unwrap().contains("identity changed"));
    app.handle_key(Key::Char('a'));
    assert!(app.verify_dialog_target(target.pid).is_err());
    app.handle_key(Key::Char('S'));
    assert!(app.status.as_deref().unwrap().contains("identity changed"));
    app.handle_key(Key::Esc);
    app.handle_key(Key::Char('i'));
    app.handle_key(Key::Char('S'));
    assert!(app.status.as_deref().unwrap().contains("identity changed"));
}
