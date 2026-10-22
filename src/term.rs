//! Terminal styling helpers.

use std::io::IsTerminal;
use std::sync::OnceLock;

use crate::cli::ColorChoice;

/// Output stream used for color decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stream {
    /// Standard output.
    Stdout,
    /// Standard error.
    Stderr,
}

/// Process-wide `--color` choice; unset behaves as `auto`.
static COLOR_CHOICE: OnceLock<ColorChoice> = OnceLock::new();

/// Record the `--color` choice for every styling decision that follows.
///
/// Call it once, right after argument parsing. Later calls are ignored.
pub fn set_color_choice(choice: ColorChoice) {
    let _ = COLOR_CHOICE.set(choice);
}

/// Return whether ANSI color should be emitted for the given stream.
#[must_use]
pub fn color_enabled(stream: Stream) -> bool {
    let is_tty = match stream {
        Stream::Stdout => std::io::stdout().is_terminal(),
        Stream::Stderr => std::io::stderr().is_terminal(),
    };
    color_enabled_for_choice(
        COLOR_CHOICE.get().copied().unwrap_or(ColorChoice::Auto),
        std::env::var_os("NO_COLOR").is_some(),
        std::env::var_os("TERM").as_deref() == Some(std::ffi::OsStr::new("dumb")),
        is_tty,
    )
}

fn color_enabled_for_choice(
    choice: ColorChoice,
    no_color: bool,
    term_dumb: bool,
    is_tty: bool,
) -> bool {
    match choice {
        ColorChoice::Always => true,
        ColorChoice::Never => false,
        ColorChoice::Auto => color_enabled_for(no_color, term_dumb, is_tty),
    }
}

fn color_enabled_for(no_color: bool, term_dumb: bool, is_tty: bool) -> bool {
    !no_color && !term_dumb && is_tty
}

/// Style the lead label of the next action (`Next`) for stdout in bold.
#[must_use]
pub fn next_label(label: &str) -> String {
    bold_for(color_enabled(Stream::Stdout), label)
}

fn bold_for(enabled: bool, label: &str) -> String {
    if !enabled || label.is_empty() || !label.bytes().all(is_safe_label_byte) {
        return label.to_owned();
    }
    format!("\x1b[1m{label}\x1b[0m")
}

/// Style an error label for stderr.
#[must_use]
pub fn error_label(label: &str) -> String {
    emphasize(Stream::Stderr, label)
}

/// Style a doctor severity label for stdout.
#[must_use]
pub fn doctor_severity(label: &str) -> String {
    emphasize(Stream::Stdout, label)
}

/// Style a fixed materialization status for stdout.
#[must_use]
pub fn operation_status(label: &str) -> String {
    emphasize(Stream::Stdout, label)
}

/// Escape control characters in untrusted text before emitting it to a human terminal.
#[must_use]
pub fn terminal_safe_text(value: &str) -> String {
    value.chars().fold(String::new(), |mut escaped, character| {
        if character.is_control() {
            escaped.extend(character.escape_default());
        } else {
            escaped.push(character);
        }
        escaped
    })
}

fn emphasize(stream: Stream, label: &str) -> String {
    emphasize_for(color_enabled(stream), label)
}

fn emphasize_for(enabled: bool, label: &str) -> String {
    if !enabled || !label.bytes().all(is_safe_label_byte) {
        return label.to_owned();
    }
    let code = match label.trim() {
        "error" => Some("31;1"),
        "warning" | "blocked" | "conflict" => Some("33;1"),
        "ok" | "created" | "applied" | "existing" | "noop" => Some("32"),
        _ => None,
    };
    code.map_or_else(
        || label.to_owned(),
        |code| format!("\x1b[{code}m{label}\x1b[0m"),
    )
}

const fn is_safe_label_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b' ')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_should_require_tty_and_absent_no_color_or_dumb_terminal() {
        assert!(color_enabled_for(false, false, true));
        assert!(!color_enabled_for(true, false, true));
        assert!(!color_enabled_for(false, true, true));
        assert!(!color_enabled_for(false, false, false));
    }

    #[test]
    fn color_choice_should_override_the_environment_only_when_explicit() {
        assert!(color_enabled_for_choice(
            ColorChoice::Always,
            true,
            true,
            false
        ));
        assert!(!color_enabled_for_choice(
            ColorChoice::Never,
            false,
            false,
            true
        ));
        assert_eq!(
            color_enabled_for_choice(ColorChoice::Auto, true, false, true),
            color_enabled_for(true, false, true)
        );
        assert_eq!(
            color_enabled_for_choice(ColorChoice::Auto, false, false, true),
            color_enabled_for(false, false, true)
        );
    }

    #[test]
    fn bold_should_style_only_safe_labels_when_enabled() {
        assert_eq!(bold_for(true, "Next"), "\x1b[1mNext\x1b[0m");
        assert_eq!(bold_for(false, "Next"), "Next");
        assert_eq!(bold_for(true, "\x1b[31mNext"), "\x1b[31mNext");
        assert_eq!(bold_for(true, ""), "");
    }

    #[test]
    fn emphasis_should_style_fixed_statuses_without_styling_control_input() {
        assert_eq!(emphasize_for(true, "error"), "\x1b[31;1merror\x1b[0m");
        assert_eq!(emphasize_for(true, "warning"), "\x1b[33;1mwarning\x1b[0m");
        assert_eq!(emphasize_for(true, "ok"), "\x1b[32mok\x1b[0m");
        assert_eq!(emphasize_for(true, "blocked"), "\x1b[33;1mblocked\x1b[0m");
        assert_eq!(emphasize_for(true, "existing"), "\x1b[32mexisting\x1b[0m");
        assert_eq!(emphasize_for(true, "\x1b[31merror"), "\x1b[31merror");
        assert_eq!(emphasize_for(false, "error"), "error");
    }

    #[test]
    fn terminal_safe_text_should_escape_controls_without_hiding_unicode() {
        assert_eq!(
            terminal_safe_text("über\u{1b}[2J-skill"),
            "über\\u{1b}[2J-skill"
        );
    }
}
