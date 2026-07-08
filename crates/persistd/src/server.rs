use std::io::{self, Write};
use std::process::ExitCode;

use persist_core::{version_string, PersistError};

pub fn run<I>(args: I) -> ExitCode
where
    I: IntoIterator<Item = String>,
{
    let mut stdout = io::stdout();
    let mut stderr = io::stderr();
    let code = run_with_io(args, &mut stdout, &mut stderr);
    ExitCode::from(code)
}

pub fn run_with_io<I, W, E>(args: I, stdout: &mut W, stderr: &mut E) -> u8
where
    I: IntoIterator<Item = String>,
    W: Write,
    E: Write,
{
    let args = args.into_iter().skip(1).collect::<Vec<_>>();
    let result = match args.first().map(String::as_str) {
        None | Some("-h" | "--help" | "help") => write_help(stdout),
        Some("-V" | "--version" | "version") => writeln!(stdout, "{}", version_string("persistd"))
            .map_err(|source| PersistError::Io {
                operation: "write version",
                source,
            }),
        Some("foreground") => Err(PersistError::not_implemented("persistd foreground runtime")),
        Some(other) => Err(PersistError::invalid_argument(format!(
            "unknown persistd command: {other}"
        ))),
    };

    match result {
        Ok(()) => 0,
        Err(error) => {
            let _ = writeln!(stderr, "persistd: {error}");
            2
        }
    }
}

fn write_help<W: Write>(stdout: &mut W) -> Result<(), PersistError> {
    writeln!(
        stdout,
        "\
PersistShell daemon

Usage:
  persistd <command>

Available now:
  help       Show this help
  version    Show version information

Planned commands:
  foreground
"
    )
    .map_err(|source| PersistError::Io {
        operation: "write help",
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_returns_success() {
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = run_with_io(
            ["persistd".to_string(), "--version".to_string()],
            &mut out,
            &mut err,
        );

        assert_eq!(code, 0);
        assert!(String::from_utf8(out)
            .expect("utf8")
            .starts_with("persistd "));
        assert!(err.is_empty());
    }
}
