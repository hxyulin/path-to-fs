#![cfg(target_os = "linux")]

use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use rustix::process::{Pid, Signal, kill_process};
use tempfile::TempDir;

struct Mount {
    process: Child,
    path: PathBuf,
}

impl Drop for Mount {
    fn drop(&mut self) {
        // A failed assertion must not leave a stale mount or a blocked FUSE worker.
        let _ = Command::new("fusermount3")
            .args(["-uz"])
            .arg(&self.path)
            .output();
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}

fn make_candidate(directory: &Path, name: &str, marker: &Path) {
    fs::create_dir_all(directory).expect("create bin directory");
    let executable = directory.join(name);
    fs::write(
        &executable,
        format!("#!/bin/sh\ntouch '{}'\n", marker.display()),
    )
    .expect("write candidate");
    fs::set_permissions(executable, fs::Permissions::from_mode(0o755))
        .expect("make candidate executable");
}

#[test]
#[ignore = "requires Linux /dev/fuse and fusermount3; mounts a temporary directory"]
fn native_execution_on_a_real_mount() {
    let temp = TempDir::new().expect("create test directory");
    let path = temp.path().join("mount");
    fs::create_dir(&path).expect("create mountpoint");
    let log = temp.path().join("mount.log");
    let process = Command::new(env!("CARGO_BIN_EXE_path-to-fs"))
        .arg("mount")
        .arg(&path)
        .arg("--helper")
        .arg(env!("CARGO_BIN_EXE_path-to-fs-helper"))
        .stdout(Stdio::null())
        .stderr(File::create(&log).expect("create mount log"))
        .spawn()
        .expect("start filesystem");
    let mut mount = Mount { process, path };
    let deadline = Instant::now() + Duration::from_secs(10);
    while !mount.path.join("ready").is_file() {
        assert!(
            mount.process.try_wait().expect("check daemon").is_none(),
            "daemon exited: {}",
            fs::read_to_string(&log).unwrap_or_default()
        );
        assert!(
            Instant::now() < deadline,
            "mount timed out: {}",
            fs::read_to_string(&log).unwrap_or_default()
        );
        thread::sleep(Duration::from_millis(20));
    }

    let image = fs::read(env!("CARGO_BIN_EXE_path-to-fs-helper")).expect("read helper image");
    assert_eq!(
        fs::read(mount.path.join("ls")).expect("read virtual executable"),
        image
    );
    assert_eq!(
        fs::read(mount.path.join(".hidden-command")).expect("read hidden executable"),
        image
    );
    assert_eq!(fs::read_dir(&mount.path).expect("list mount").count(), 0);
    assert!(fs::write(mount.path.join("ls"), b"overwrite").is_err());
    assert!(fs::write(mount.path.join("new-file"), b"create").is_err());
    assert!(fs::metadata(mount.path.join("directory/command")).is_err());
    let mut file = File::open(mount.path.join("ls")).expect("open virtual executable");
    file.seek(SeekFrom::End(100)).expect("seek past EOF");
    let mut tail = Vec::new();
    file.read_to_end(&mut tail).expect("read past EOF");
    assert!(tail.is_empty());
    drop(file);

    let marker = temp.path().join("must-not-execute");
    let name = "unique-path-to-test-ls";
    let bin1 = temp.path().join("bin1");
    let bin2 = temp.path().join("bin2");
    make_candidate(&bin1, name, &marker);
    make_candidate(&bin2, name, &marker);
    let alias_dir = temp.path().join("alias");
    fs::create_dir(&alias_dir).expect("create alias directory");
    symlink(mount.path.join(name), alias_dir.join(name)).expect("link into mount");
    thread::scope(|scope| {
        for index in 0..8 {
            let mountpoint = &mount.path;
            let bin = if index % 2 == 0 { &bin1 } else { &bin2 };
            let alias = &alias_dir;
            scope.spawn(move || {
                let path = std::env::join_paths([mountpoint, alias, bin]).expect("join PATH");
                let result = Command::new(mountpoint.join(name))
                    .args(["--precision", "two words", "$(touch must-not-execute)"])
                    .env("PATH", path)
                    .output()
                    .expect("execute virtual native binary");
                assert_eq!(result.status.code(), Some(127));
                assert!(result.stdout.is_empty());
                let stderr = String::from_utf8(result.stderr).expect("UTF-8 diagnostic");
                let expected = bin.join(name).display().to_string();
                assert!(stderr.contains(&expected), "caller PATH not used: {stderr}");
                assert!(
                    stderr.contains("--precision 'two words' '$(touch must-not-execute)'"),
                    "{stderr}"
                );
                let found = stderr.split("Found:\n").nth(1).expect("found section");
                assert!(
                    !found.contains(&mountpoint.display().to_string()),
                    "suggested the mount: {stderr}"
                );
                assert!(
                    !found.contains(&alias.display().to_string()),
                    "suggested an alias of the mount: {stderr}"
                );
            });
        }
        for name in [
            "unknown-command-one",
            "unknown-command-two",
            "a'quote",
            "non-utf8-not-needed",
        ] {
            let mountpoint = &mount.path;
            scope.spawn(move || {
                let result = Command::new(mountpoint.join(name))
                    .env_remove("PATH")
                    .output()
                    .expect("execute unknown name");
                assert_eq!(result.status.code(), Some(127));
                let stderr = String::from_utf8(result.stderr).expect("UTF-8 diagnostic");
                assert!(
                    stderr.contains("is a placeholder path used in examples."),
                    "{stderr}"
                );
                assert!(stderr.contains("No executable named"), "{stderr}");
            });
        }
    });
    assert!(!marker.exists(), "a suggested executable ran");

    let pid = Pid::from_raw(mount.process.id() as i32).expect("daemon PID");
    kill_process(pid, Signal::TERM).expect("request graceful shutdown");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = mount.process.try_wait().expect("check shutdown") {
            assert!(
                status.success(),
                "daemon failed: {}",
                fs::read_to_string(&log).unwrap_or_default()
            );
            break;
        }
        assert!(Instant::now() < deadline, "daemon did not stop");
        thread::sleep(Duration::from_millis(20));
    }
    assert!(
        !mount.path.join("ready").exists(),
        "filesystem remains mounted"
    );
}
