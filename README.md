# path-to-fs

Turn `/path/to/ls` into a useful error message.

Tutorials use paths such as `/path/to/program` to mean “replace this with the
actual path.” `path-to-fs` is a Linux FUSE filesystem that catches attempts to
execute those paths, explains the placeholder, and looks for the real program.

```console
$ /path/to/ls --help
/path/to/ls is a placeholder path used in examples.
Replace it with the actual path to the program.

Found:
  /usr/bin/ls

Try:
  /usr/bin/ls --help
```

Matches depend on what is installed. The diagnostic goes to stderr and exits
with status **127**, even when a match is found. It never runs the suggested
program.

## Install

Mounting requires **Linux**, access to `/dev/fuse`, and `fusermount3`. Install
your distribution's FUSE 3 package. On Debian or Ubuntu:

```sh
sudo apt-get install fuse3
```

With Rust 1.85 or newer and a C linker installed:

```sh
cargo install --git https://github.com/hxyulin/path-to-fs --locked --bins
```

This installs both `path-to-fs` and `path-to-fs-helper` in Cargo's binary
directory, usually `~/.cargo/bin`. Keep the two executables together.
Building does not require libfuse development headers.

To build from a checkout instead:

```sh
git clone https://github.com/hxyulin/path-to-fs.git
cd path-to-fs
cargo build --release --locked --bins
```

The executables will be in `target/release/`. Use `./target/release/path-to-fs`
in place of `path-to-fs` in the commands below.

## Try it

Start with an empty directory in your home directory:

```sh
mkdir -p "$HOME/path-to-mount"
path-to-fs mount "$HOME/path-to-mount"
```

The process stays in the foreground. In another terminal:

```sh
"$HOME/path-to-mount/ls" --help
"$HOME/path-to-mount/program-that-is-not-installed"
```

Press **Ctrl-C** in the mounting terminal to unmount. You can also run:

```sh
fusermount3 -u "$HOME/path-to-mount"
```

To see the diagnostic without mounting, including on macOS:

```sh
path-to-fs explain /path/to/ls --help
```

## Mount at /path/to

Create an empty mountpoint owned by your user, then run the daemon as that user:

```sh
sudo install -d -o "$(id -u)" -g "$(id -g)" /path/to
path-to-fs mount /path/to
```

The mountpoint must be empty. FUSE restricts access to the mounting user.
The daemon handles Ctrl-C and SIGTERM by unmounting.

If the helper is elsewhere, pass its location explicitly:

```sh
path-to-fs mount /path/to --helper /absolute/path/to/path-to-fs-helper
```

## How it works

Every filename directly under the mount exposes the same native ELF executable.
The daemon loads the helper into memory before mounting and serves those bytes
as a read-only file. No per-name files are written to disk, and no binaries are
patched during execution.

When Linux executes a file, the helper reads its invocation name from `argv[0]`.
It searches using the caller's `PATH`, `HOME`, working directory, and execution
permissions. The daemon's environment does not determine the suggestions.
Arguments are included in the suggested command using Bash-compatible quoting.

Rebuild and remount to update the helper image.

## Where it searches

The helper checks exact executable names in this order:

1. The caller's `PATH`, in order. Relative and empty entries use the caller's
   working directory.
2. `~/.local/bin`, `~/bin`, `~/.cargo/bin`, `~/.nix-profile/bin`, and
   `~/.local/share/flatpak/exports/bin`.
3. `/usr/local/bin`, `/usr/local/sbin`, `/usr/bin`, `/usr/sbin`, `/bin`, `/sbin`,
   `/opt/bin`, `/opt/homebrew/bin`, `/snap/bin`, `/var/lib/flatpak/exports/bin`,
   `/run/current-system/sw/bin`, and `/nix/var/nix/profiles/default/bin`.

Only regular files with execute permission are suggested. Directory aliases are
deduplicated. Executable symlinks retain their basename, since programs such as
BusyBox use it to select their behavior. The placeholder's directory, descendants,
and symlinks back into them are excluded.

The helper does not scan the disk recursively, search package repositories, or
guess that `python` might mean `python3`.

## Limits

- **One filename under the mount.** `/path/to/ls` works.
  `/path/to/project/bin/ls` fails because `project` is an executable, not a directory.
- **Execution triggers the diagnostic.** `cat /path/to/config` reads ELF bytes.
  It does not produce a configuration-file diagnostic.
- **No directory listing or completion.** `ls /path/to` lists no files because
  the namespace accepts arbitrary names. Direct lookups such as
  `stat /path/to/ls` work.
- **The invocation name follows shell convention.** A custom launcher that
  overrides `argv[0]` can change what the helper searches for.
- **Mounting is Linux-only.** The helper must match the machine's architecture
  and runtime. macOS supports the `explain` command, but not mounting.

## Development and tests

```sh
cargo fmt --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked
cargo doc --locked --no-deps
```

On Linux with `/dev/fuse` and `fusermount3`, include the real mount test:

```sh
cargo test --locked -- --include-ignored
```

The mount test executes native files under multiple names, checks concurrent
callers with different `PATH` values, rejects writes, and checks graceful
unmounting. It uses a temporary directory and cleans up its mount.
The regular tests cover search order, symlinks, missing programs, and argument
quoting. GitHub Actions runs the regular checks on Linux and macOS.

### Test in Docker

On a Linux Docker engine, including OrbStack on macOS:

```sh
docker build -f Dockerfile.test -t path-to-fs-test .
docker run --rm --device /dev/fuse --cap-add SYS_ADMIN \
  --security-opt apparmor=unconfined path-to-fs-test
```

The container runs the checks and real mount test as an ordinary user. The
device and capability allow FUSE mounting inside the container. It also builds
and installs both release executables in the container.

## License

This project's original code and documentation are dedicated to the public
domain under [CC0 1.0 Universal](LICENSE). You may use, copy, modify, and
redistribute them for any purpose, including commercial use, without attribution.
CC0 includes a fallback license where the public-domain waiver is ineffective.

Third-party dependencies retain their own licenses.
