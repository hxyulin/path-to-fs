use std::ffi::OsStr;
use std::fmt::Write;
use std::os::unix::ffi::OsStrExt;

pub(crate) fn word(value: &OsStr) -> String {
    let bytes = value.as_bytes();
    if !bytes.is_empty()
        && bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || b"_@%+=:,./-".contains(b))
    {
        return String::from_utf8(bytes.to_vec()).expect("shell-safe bytes are ASCII");
    }
    if let Some(text) = value.to_str().filter(|s| !s.chars().any(char::is_control)) {
        return format!("'{}'", text.replace('\'', "'\\''"));
    }

    // ANSI-C quotes preserve non-UTF-8 names and keep terminal control bytes out of output.
    let mut quoted = String::from("$'");
    for &byte in bytes {
        match byte {
            b'\'' => quoted.push_str("\\'"),
            b'\\' => quoted.push_str("\\\\"),
            b' '..=b'~' => quoted.push(char::from(byte)),
            _ => write!(quoted, "\\x{byte:02x}").expect("writing to a String cannot fail"),
        }
    }
    quoted.push('\'');
    quoted
}

#[cfg(test)]
mod tests {
    use super::word;
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    use std::process::Command;

    #[test]
    fn shell_round_trips_arguments_without_interpreting_them() {
        for input in [
            &b"plain-name"[..],
            b"",
            b"a space",
            b"a'quote",
            b"$(exit 33); `exit 34` & *",
            b"line\n\x1b[31m",
            b"non-utf8-\xff",
            "計算機".as_bytes(),
        ] {
            let command = format!("printf '%s' {}", word(OsStr::from_bytes(input)));
            let result = Command::new("/bin/bash")
                .args(["-c", &command])
                .output()
                .expect("run bash to check the suggested command");
            assert!(result.status.success(), "input {input:?}");
            assert_eq!(result.stdout, input, "input {input:?}");
            assert!(result.stderr.is_empty(), "input {input:?}");
        }
    }
}
