use std::path::PathBuf;

struct TempDir(PathBuf);

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn bad_config_exits_with_a_diagnostic_without_overwriting_it() {
    let dir = TempDir(std::env::temp_dir().join(format!(
        "gander-cli-config-{}-{}", std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    )));
    std::fs::create_dir(&dir.0).unwrap();
    let path = dir.0.join("config.toml");
    for text in ["delay_ms = [", "tabs = []", "active_tab = 99"] {
        std::fs::write(&path, text).unwrap();
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_gander"))
            .env("GANDER_CONFIG", &path)
            .args(["--json", "--limit", "0"])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.contains("cannot load config"), "{error}");
        assert!(error.contains(path.to_str().unwrap()), "{error}");
        assert!(!error.contains("panicked"), "{error}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
    }
}
