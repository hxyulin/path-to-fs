#[cfg(target_os = "linux")]
mod filesystem;

use std::ffi::OsString;
use std::io;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(version, about = "Explain placeholder executable paths from tutorials")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Mount the placeholder filesystem in the foreground (Linux only).
    Mount {
        /// An existing, empty directory. Create /path/to separately if desired.
        mountpoint: PathBuf,
        /// Native helper image. Defaults to path-to-fs-helper beside this binary.
        #[arg(long)]
        helper: Option<PathBuf>,
    },
    /// Try the diagnostic without mounting a filesystem.
    Explain {
        /// The placeholder executable path, such as /path/to/ls.
        placeholder: PathBuf,
        /// Original arguments, included in the suggested command.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<OsString>,
    },
}

fn run(cli: Cli) -> Result<ExitCode> {
    match cli.command {
        Command::Mount { mountpoint, helper } => {
            #[cfg(target_os = "linux")]
            filesystem::mount(&mountpoint, helper.as_deref())?;
            #[cfg(not(target_os = "linux"))]
            {
                let _ = (mountpoint, helper);
                anyhow::bail!(
                    "mounting is supported on Linux only; use `explain` to try the diagnostic"
                );
            }
            #[cfg(target_os = "linux")]
            Ok(ExitCode::SUCCESS)
        }
        Command::Explain { placeholder, args } => {
            path_to_fs::write_explanation(&placeholder, &args, io::stderr().lock())?;
            Ok(ExitCode::from(path_to_fs::PLACEHOLDER_EXIT_CODE))
        }
    }
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(status) => status,
        Err(error) => {
            eprintln!("path-to-fs: {error:#}");
            ExitCode::FAILURE
        }
    }
}
