use std::process::Command;

#[test]
fn informational_flags_exit_before_startup() {
    let root = std::env::temp_dir().join(format!("ario-cli-test-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();

    for malformed_config in [false, true] {
        if malformed_config {
            let config_dir = if cfg!(target_os = "macos") {
                root.join("Library/Application Support/ario")
            } else if cfg!(target_os = "windows") {
                root.join("ario/config")
            } else {
                root.join("ario")
            };
            std::fs::create_dir_all(&config_dir).unwrap();
            for name in ["tui.toml", "server.toml"] {
                std::fs::write(config_dir.join(name), "not valid TOML [").unwrap();
            }
        }

        for (binary, name, existing_option) in [
            (env!("CARGO_BIN_EXE_ario"), "ario", "--glyphs"),
            (env!("CARGO_BIN_EXE_ario_daemon"), "ario_daemon", "--host"),
        ] {
            for flag in ["-h", "--help", "-v", "--version"] {
                let output = Command::new(binary)
                    .arg(flag)
                    .env("HOME", &root)
                    .env("USERPROFILE", &root)
                    .env("XDG_CONFIG_HOME", &root)
                    .env("APPDATA", &root)
                    .output()
                    .unwrap();
                assert!(output.status.success(), "{name} {flag}: {output:?}");
                assert!(output.stderr.is_empty(), "{name} {flag}: {output:?}");
                let stdout = String::from_utf8(output.stdout).unwrap();
                let version = format!("{name} {}\n", env!("CARGO_PKG_VERSION"));
                if matches!(flag, "-v" | "--version") {
                    assert_eq!(stdout, version);
                } else {
                    assert!(stdout.starts_with(&version));
                    for expected in ["Usage:", "-h, --help", "-v, --version", existing_option] {
                        assert!(stdout.contains(expected), "{name}: {stdout}");
                    }
                }
            }
        }

        if !malformed_config {
            assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0);
        }
    }

    std::fs::remove_dir_all(root).unwrap();
}
