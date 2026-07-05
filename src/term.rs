//! Terminal styling helpers.

use std::io::IsTerminal;

/// Output stream used for color decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stream {
    /// Standard output.
    Stdout,
    /// Standard error.
    Stderr,
}

/// Return whether ANSI color should be emitted for the given stream.
#[must_use]
pub fn color_enabled(stream: Stream) -> bool {
    let is_tty = match stream {
        Stream::Stdout => std::io::stdout().is_terminal(),
        Stream::Stderr => std::io::stderr().is_terminal(),
    };
    color_enabled_for(std::env::var_os("NO_COLOR").is_some(), is_tty)
}

fn color_enabled_for(no_color: bool, is_tty: bool) -> bool {
    !no_color && is_tty
}

/// Style an error label for stderr.
#[must_use]
pub fn error_label(label: &str) -> String {
    if color_enabled(Stream::Stderr) {
        format!("\x1b[31;1m{label}\x1b[0m")
    } else {
        label.to_owned()
    }
}

/// Style a doctor severity label for stdout.
#[must_use]
pub fn doctor_severity(label: &str) -> String {
    if !color_enabled(Stream::Stdout) {
        return label.to_owned();
    }
    let code = match label {
        "error" => "31;1",
        "warning" => "33;1",
        "ok" => "32",
        _ => "36",
    };
    format!("\x1b[{code}m{label}\x1b[0m")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_should_require_tty_and_absent_no_color() {
        assert!(color_enabled_for(false, true));
        assert!(!color_enabled_for(true, true));
        assert!(!color_enabled_for(false, false));
    }
}
