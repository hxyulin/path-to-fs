use std::collections::HashSet;
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use rustix::fs::{Access, AtFlags, CWD, accessat};

pub(crate) fn common_directories(home: Option<&Path>) -> Vec<PathBuf> {
    let mut directories = Vec::new();
    if let Some(home) = home {
        for suffix in [
            ".local/bin",
            "bin",
            ".cargo/bin",
            ".nix-profile/bin",
            ".local/share/flatpak/exports/bin",
        ] {
            directories.push(home.join(suffix));
        }
    }
    directories.extend(
        [
            "/usr/local/bin",
            "/usr/local/sbin",
            "/usr/bin",
            "/usr/sbin",
            "/bin",
            "/sbin",
            "/opt/bin",
            "/opt/homebrew/bin",
            "/snap/bin",
            "/var/lib/flatpak/exports/bin",
            "/run/current-system/sw/bin",
            "/nix/var/nix/profiles/default/bin",
        ]
        .into_iter()
        .map(PathBuf::from),
    );
    directories
}

pub(crate) fn find_executables(
    name: &OsStr,
    path: Option<&OsStr>,
    common: &[PathBuf],
    excluded: Option<&Path>,
) -> Vec<PathBuf> {
    let excluded = excluded.and_then(|p| fs::canonicalize(p).ok());
    let path_dirs = path.map(env::split_paths).into_iter().flatten();
    let mut seen = HashSet::new();
    let mut matches = Vec::new();
    for directory in path_dirs.chain(common.iter().cloned()) {
        let directory = if directory.as_os_str().is_empty() {
            Path::new(".")
        } else {
            &directory
        };
        let Ok(directory) = fs::canonicalize(directory) else {
            continue;
        };
        if excluded.as_ref().is_some_and(|p| directory.starts_with(p)) {
            continue;
        }
        let candidate = directory.join(name);
        if !seen.insert(candidate.clone()) {
            continue;
        }
        let Ok(target) = fs::canonicalize(&candidate) else {
            continue;
        };
        if excluded.as_ref().is_some_and(|p| target.starts_with(p)) {
            continue;
        }
        if fs::metadata(&candidate).is_ok_and(|m| m.is_file())
            && accessat(CWD, &candidate, Access::EXEC_OK, AtFlags::EACCESS).is_ok()
        {
            // Keep the executable's basename: resolving its symlink can change argv[0] behavior.
            matches.push(candidate);
        }
    }
    matches
}

#[cfg(test)]
mod tests {
    use super::find_executables;
    use std::env;
    use std::ffi::OsStr;
    use std::fs;
    use std::os::unix::fs::{PermissionsExt, symlink};
    use std::path::{Path, PathBuf};
    use tempfile::TempDir;

    fn executable(directory: &Path, name: &str) -> PathBuf {
        fs::create_dir_all(directory).expect("create executable directory");
        let path = directory.join(name);
        fs::write(&path, b"#!/bin/sh\nexit 99\n").expect("write executable");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755))
            .expect("make file executable");
        path
    }

    #[test]
    fn path_order_wins_and_directory_aliases_are_deduplicated() {
        let temp = TempDir::new().expect("create test directory");
        let first = executable(&temp.path().join("first"), "ls");
        let second = executable(&temp.path().join("second"), "ls");
        let fallback = executable(&temp.path().join("common"), "ls");
        let alias = temp.path().join("alias");
        symlink(first.parent().expect("parent"), &alias).expect("alias directory");
        let path = env::join_paths([
            first.parent().expect("parent"),
            &alias,
            second.parent().expect("parent"),
        ])
        .expect("join PATH");
        let common = vec![fallback.parent().expect("parent").to_path_buf()];
        assert_eq!(
            find_executables(OsStr::new("ls"), Some(&path), &common, None),
            [first, second, fallback].map(|p| p.canonicalize().expect("canonical test file"))
        );
    }

    #[test]
    fn rejects_nonexecutables_directories_and_broken_symlinks() {
        let temp = TempDir::new().expect("create test directory");
        let directories: Vec<_> = ["plain", "directory", "broken"]
            .map(|name| temp.path().join(name))
            .into_iter()
            .collect();
        for dir in &directories {
            fs::create_dir(dir).expect("create directory");
        }
        fs::write(directories[0].join("ls"), b"text").expect("write plain file");
        fs::create_dir(directories[1].join("ls")).expect("create directory candidate");
        symlink("missing", directories[2].join("ls")).expect("broken link");
        let path = env::join_paths(&directories).expect("join PATH");
        assert!(find_executables(OsStr::new("ls"), Some(&path), &[], None).is_empty());
    }

    #[test]
    fn excludes_the_mount_and_symlinks_back_into_it() {
        let temp = TempDir::new().expect("create test directory");
        let mount = temp.path().join("mount");
        let placeholder = executable(&mount, "ls");
        let nested = executable(&mount.join("nested"), "ls");
        let outside = temp.path().join("outside");
        fs::create_dir(&outside).expect("create outside directory");
        symlink(&placeholder, outside.join("ls")).expect("link back into mount");
        let path = env::join_paths([&mount, nested.parent().expect("parent"), &outside])
            .expect("join PATH");
        assert!(find_executables(OsStr::new("ls"), Some(&path), &[], Some(&mount)).is_empty());
    }

    #[test]
    fn preserves_executable_symlink_names() {
        let temp = TempDir::new().expect("create test directory");
        executable(temp.path(), "busybox");
        symlink("busybox", temp.path().join("ls")).expect("create executable alias");
        let dir = temp.path().canonicalize().expect("canonical directory");
        assert_eq!(
            find_executables(OsStr::new("ls"), None, std::slice::from_ref(&dir), None,),
            [dir.join("ls")]
        );
    }

    #[test]
    fn empty_and_relative_path_entries_mean_the_callers_working_directory() {
        let cwd = env::current_dir().expect("read working directory");
        let temp = tempfile::Builder::new()
            .prefix("path-to-test-")
            .tempdir_in(&cwd)
            .expect("create test directory in working directory");
        let name = "unique-path-to-test-executable";
        let candidate = executable(temp.path(), name);
        let relative = temp
            .path()
            .strip_prefix(&cwd)
            .expect("relative test directory");
        let path = env::join_paths([relative, relative]).expect("join PATH");
        assert_eq!(
            find_executables(OsStr::new(name), Some(&path), &[], None),
            [candidate.canonicalize().expect("canonical executable")]
        );

        let local = tempfile::NamedTempFile::new_in(&cwd).expect("create local executable");
        fs::set_permissions(local.path(), fs::Permissions::from_mode(0o755))
            .expect("make local file executable");
        let name = local.path().file_name().expect("local executable name");
        assert_eq!(
            find_executables(name, Some(OsStr::new("")), &[], None),
            [local.path().canonicalize().expect("canonical local file")]
        );
        assert!(find_executables(name, None, &[], None).is_empty());
    }
}
