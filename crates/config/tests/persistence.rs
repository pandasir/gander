use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use gander_config::{load, save, Config};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "gander-config-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn config(&self) -> PathBuf {
        self.0.join("config.toml")
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn invalid_configs_are_reported_and_left_untouched() {
    let dir = TempDir::new();
    let path = dir.config();
    for text in ["delay_ms = [", "tabs = []", "active_tab = 99"] {
        std::fs::write(&path, text).unwrap();
        let error = load(&path).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
    }
}

#[test]
fn read_errors_are_not_treated_as_first_run() {
    let dir = TempDir::new();
    assert!(load(&dir.0).is_err());
    assert!(load(&dir.config()).is_ok());
}

#[test]
fn atomic_save_replaces_existing_config_and_cleans_up() {
    let dir = TempDir::new();
    let path = dir.config();
    save(&Config::default(), &path).unwrap();
    let mut config = load(&path).unwrap();
    config.delay_ms = 250;
    save(&config, &path).unwrap();
    assert_eq!(load(&path).unwrap().delay_ms, 250);
    assert_eq!(std::fs::read_dir(&dir.0).unwrap().count(), 1);
}

#[cfg(unix)]
#[test]
fn atomic_save_does_not_truncate_an_open_original_file() {
    use std::io::Read;
    let dir = TempDir::new();
    let path = dir.config();
    save(&Config::default(), &path).unwrap();
    let before = std::fs::read_to_string(&path).unwrap();
    let mut original = std::fs::File::open(&path).unwrap();
    save(
        &Config {
            delay_ms: 500,
            ..Default::default()
        },
        &path,
    )
    .unwrap();
    let mut old_contents = String::new();
    original.read_to_string(&mut old_contents).unwrap();
    assert_eq!(old_contents, before);
    assert_eq!(load(&path).unwrap().delay_ms, 500);
}

#[test]
fn invalid_save_preserves_previous_config() {
    let dir = TempDir::new();
    let path = dir.config();
    save(&Config::default(), &path).unwrap();
    let before = std::fs::read(&path).unwrap();
    let config = Config {
        tabs: vec![],
        ..Default::default()
    };
    assert!(save(&config, &path).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
}

#[test]
fn failed_rename_removes_temporary_file() {
    let dir = TempDir::new();
    let path = dir.config();
    std::fs::create_dir(&path).unwrap();
    std::fs::write(path.join("sentinel"), "untouched").unwrap();
    assert!(save(&Config::default(), &path).is_err());
    assert_eq!(
        std::fs::read_to_string(path.join("sentinel")).unwrap(),
        "untouched"
    );
    assert_eq!(std::fs::read_dir(&dir.0).unwrap().count(), 1);
}

#[cfg(unix)]
#[test]
fn saving_preserves_symlinks_and_file_permissions() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let dir = TempDir::new();
    let path = dir.config();
    let target = dir.0.join("dotfile.toml");
    save(&Config::default(), &target).unwrap();
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o640)).unwrap();
    symlink(&target, &path).unwrap();
    let config = Config {
        delay_ms: 300,
        ..Default::default()
    };
    save(&config, &path).unwrap();
    assert!(std::fs::symlink_metadata(&path)
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(load(&target).unwrap().delay_ms, 300);
    assert_eq!(
        std::fs::metadata(&target).unwrap().permissions().mode() & 0o777,
        0o640
    );
}
