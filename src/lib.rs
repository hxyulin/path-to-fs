//! Diagnostics for executable placeholder paths used in examples and tutorials.

mod search;
mod shell;

use std::env;
use std::ffi::OsString;
use std::io::{self, Write};
use std::path::Path;

/// Exit status used when a placeholder is executed, including when suggestions are found.
pub const PLACEHOLDER_EXIT_CODE: u8 = 127;

/// Explains a placeholder and suggests installed executables using the current process's environment.
///
/// Searches `PATH` first, then common installation directories, without executing any matches.
/// Arguments appear only in the suggested replacement command. Suggestions exclude the
/// placeholder's directory and its descendants. Output uses Bash-compatible shell quoting.
///
/// # Errors
/// Returns an error if `invocation` has no filename or writing the diagnostic fails.
pub fn write_explanation(
    invocation: &Path,
    args: &[OsString],
    mut output: impl Write,
) -> io::Result<()> {
    let name = invocation.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "placeholder path has no filename",
        )
    })?;
    let path = env::var_os("PATH");
    let home = env::var_os("HOME");
    let common = search::common_directories(home.as_deref().map(Path::new));
    let excluded = invocation.parent().filter(|p| !p.as_os_str().is_empty());
    let matches = search::find_executables(name, path.as_deref(), &common, excluded);

    writeln!(
        output,
        "{} is a placeholder path used in examples.",
        shell::word(invocation.as_os_str())
    )?;
    writeln!(output, "Replace it with the actual path to the program.")?;
    if matches.is_empty() {
        writeln!(
            output,
            "\nNo executable named {} was found in PATH or common installation directories.",
            shell::word(name)
        )?;
        writeln!(
            output,
            "Install the program or locate its executable, then use that path."
        )?;
    } else {
        writeln!(output, "\nFound:")?;
        for candidate in &matches {
            writeln!(output, "  {}", shell::word(candidate.as_os_str()))?;
        }
        let mut suggestion = shell::word(matches[0].as_os_str());
        for arg in args {
            suggestion.push(' ');
            suggestion.push_str(&shell::word(arg));
        }
        writeln!(output, "\nTry:\n  {suggestion}")?;
    }
    Ok(())
}
