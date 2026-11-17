use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

use tempfile::TempDir;

#[test]
fn diagnostic_finds_path_and_home_installations_without_running_them() {
    let temp = TempDir::new().expect("create test directory");
    let bin = temp.path().join("bin");
    let home = temp.path().join("home");
    let local_bin = home.join(".local/bin");
    fs::create_dir_all(&bin).expect("create bin directory");
    fs::create_dir_all(&local_bin).expect("create home bin directory");
    for directory in [&bin, &local_bin] {
        let executable = directory.join("unique-path-to-test-ls");
        fs::write(&executable, b"#!/bin/sh\nprintf 'unexpected execution'\n")
            .expect("write candidate");
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o755))
            .expect("make candidate executable");
    }
    let result = Command::new(env!("CARGO_BIN_EXE_path-to-fs"))
        .args([
            "explain",
            "/path/to/unique-path-to-test-ls",
            "--precision",
            "two words",
            "$(exit 33); 'quoted'",
            "",
        ])
        .env("PATH", &bin)
        .env("HOME", &home)
        .output()
        .expect("run diagnostic");
    assert_eq!(result.status.code(), Some(127));
    assert!(result.stdout.is_empty(), "diagnostic should use stderr");
    let stderr = String::from_utf8(result.stderr).expect("UTF-8 diagnostic");
    let first = bin
        .canonicalize()
        .expect("canonical bin")
        .join("unique-path-to-test-ls");
    let second = local_bin
        .canonicalize()
        .expect("canonical local bin")
        .join("unique-path-to-test-ls");
    assert!(
        stderr.contains("is a placeholder path used in examples."),
        "{stderr}"
    );
    let first_index = stderr
        .find(&first.display().to_string())
        .expect("PATH match");
    let second_index = stderr
        .find(&second.display().to_string())
        .expect("home match");
    assert!(first_index < second_index, "PATH should win: {stderr}");
    assert!(stderr.contains("--precision 'two words'"), "{stderr}");
    assert!(
        stderr.contains("'$(exit 33); '\\''quoted'\\''' ''"),
        "{stderr}"
    );
    assert!(!stderr.contains("unexpected execution"), "{stderr}");
}

#[test]
fn diagnostic_explains_unknown_names_and_missing_environment() {
    let result = Command::new(env!("CARGO_BIN_EXE_path-to-fs"))
        .args([
            "explain",
            "/path/to/unique-uninstalled-path-to-test-command",
        ])
        .env_remove("PATH")
        .env_remove("HOME")
        .output()
        .expect("run diagnostic");
    assert_eq!(result.status.code(), Some(127));
    assert!(result.stdout.is_empty());
    let stderr = String::from_utf8(result.stderr).expect("UTF-8 diagnostic");
    assert!(
        stderr.contains("No executable named unique-uninstalled-path-to-test-command"),
        "{stderr}"
    );
    assert!(!stderr.contains("Try:"), "{stderr}");
}
