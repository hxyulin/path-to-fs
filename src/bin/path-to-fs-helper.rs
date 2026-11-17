use std::env;
use std::io;
use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = env::args_os();
    let Some(invocation) = args.next() else {
        eprintln!("path-to-fs: the executable invocation name is missing");
        return ExitCode::from(path_to_fs::PLACEHOLDER_EXIT_CODE);
    };
    if let Err(error) = path_to_fs::write_explanation(
        Path::new(&invocation),
        &args.collect::<Vec<_>>(),
        io::stderr().lock(),
    ) {
        eprintln!("path-to-fs: {error}");
    }
    ExitCode::from(path_to_fs::PLACEHOLDER_EXIT_CODE)
}
